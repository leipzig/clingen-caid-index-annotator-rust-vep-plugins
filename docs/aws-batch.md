# Running the regeneration pipeline on AWS Batch

The pipeline (`pipeline/`, a Nextflow workflow) can run every task as an AWS
Batch job: the work directory lives in S3, the task image in ECR, and the
instances come and go with the queue. This guide sets up that environment with
one CloudFormation template, builds and pushes the image, and shows how to run
the 31-hour protein-coding exon run from a small launcher machine.

**Status: nothing here has been deployed.** The template passes
`cfn-lint` 1.57.2, and `bash pipeline/tests/check_profiles.sh` checks what the
`awsbatch` profile resolves to, but no AWS account was used. Expect to adjust
something on the first deployment. The commands are written to run as they
stand once the values in angle brackets are filled in.

## What the stack creates

`pipeline/aws/clingen-batch.yaml` creates:

| Resource | What it is for |
|---|---|
| S3 bucket | The work directory (`work/`) and the results (`results/`). Objects under `work/` expire after `WorkExpirationDays` (30). Nothing else expires. |
| ECR repository | Holds the task image. Untagged images expire after 14 days. |
| Instance role and profile | What each Batch instance, and so each task, runs as. It can read and write this bucket and nothing else in S3. It carries the two managed policies ECS and ECR need: `AmazonEC2ContainerServiceforEC2Role` and `AmazonEC2ContainerRegistryReadOnly`. |
| Launch template | A gp3 root volume of `RootVolumeGiB` (200) GiB, and instance metadata set to IMDSv2 only with a hop limit of 2. |
| Compute environment | Managed, `SPOT` or `EC2` (on-demand) as `ComputeType`, instance types `optimal` by default, at most `MaxvCpus` (64) vCPUs, in the subnets and security groups you give it. |
| Job queue | The queue you pass as `--aws_queue`. |

The root volume is larger than the AMI's 30 GiB because it holds the Docker
images and every task's scratch files. The sort steps need room for their
temporary files, and `VALIDATE` unpacks one chromosome's whole harvest into its
work directory, which is several GB for chr1 with gnomAD.

The hop limit of 2 is there because tasks run in containers: with the default of
1, a container cannot reach the instance metadata service, and the `aws` CLI in
the task then has no credentials.

## Prerequisites

- An AWS account, and credentials that can create the stack's resources:
  CloudFormation, S3, ECR, IAM roles and instance profiles, EC2 launch templates
  and Batch. `--capabilities CAPABILITY_IAM` in the deploy command acknowledges
  that the stack creates an IAM role.
- AWS CLI v2 and Docker (or another builder that speaks the Docker CLI) on your
  workstation.
- Subnets with internet egress and a security group for the instances. Tasks call
  the ClinGen Registry, NCBI, UCSC and, for gnomAD, Google Cloud Storage (the
  file listing and md5 sums always come from there) and also the AWS mirror with
  `--gnomad_mirror aws`, all over HTTPS. Subnets of your default VPC do: they
  hand out public IPs, so no NAT gateway is needed, and a security group with no
  inbound rules is enough. Private subnets work too, behind a NAT gateway,
  which costs money (see [Costs](#costs)).
- Spot capacity if you use `ComputeType=SPOT`. A new account's quota for Spot
  vCPUs can be small; check Service Quotas if jobs sit in `RUNNABLE`.
- The region. This guide uses `us-east-1`, because the AWS copy of gnomAD is
  there (see [gnomAD on AWS](#gnomad-on-aws)). The template works in any region.

The commands below use these shell variables; set them once:

```bash
REGION=us-east-1
STACK=clingen-batch
```

## 1. Deploy the stack

Find subnets and a security group. For the default VPC:

```bash
VPC=$(aws ec2 describe-vpcs --region "$REGION" --filters Name=is-default,Values=true \
  --query 'Vpcs[0].VpcId' --output text)
SUBNETS=$(aws ec2 describe-subnets --region "$REGION" \
  --filters Name=vpc-id,Values="$VPC" Name=default-for-az,Values=true \
  --query 'Subnets[].SubnetId' --output text | tr '\t' ',')
SG=$(aws ec2 describe-security-groups --region "$REGION" \
  --filters Name=vpc-id,Values="$VPC" Name=group-name,Values=default \
  --query 'SecurityGroups[0].GroupId' --output text)
```

Lint and deploy:

```bash
cfn-lint pipeline/aws/clingen-batch.yaml

aws cloudformation deploy --region "$REGION" --stack-name "$STACK" \
  --template-file pipeline/aws/clingen-batch.yaml --capabilities CAPABILITY_IAM \
  --parameter-overrides Subnets="$SUBNETS" SecurityGroupIds="$SG" \
    ComputeType=SPOT MaxvCpus=64 RootVolumeGiB=200
```

(`cfn-lint` comes with `pipeline/environment.yml`.) The parameters and their
defaults:

| Parameter | Default | Notes |
|---|---|---|
| `Subnets`, `SecurityGroupIds` | required | See the prerequisites. |
| `ComputeType` | `SPOT` | `SPOT` allocates with `SPOT_PRICE_CAPACITY_OPTIMIZED`, `EC2` (on-demand) with `BEST_FIT_PROGRESSIVE`. |
| `InstanceTypes` | `optimal` | `optimal` is x86-64 (m6i, c6i, r6i, c7i). For an arm64 image use `default_arm64`. All types must match the image's architecture. |
| `MaxvCpus` | 64 | `ENUMERATE` and `LOOKUP` run one task at a time by design, so extra vCPUs help only the other steps. |
| `RootVolumeGiB` | 200 | See above. |
| `WorkExpirationDays` | 30 | See [Resuming](#resuming-and-the-work-expiry). |
| `BucketName` | empty | Empty lets CloudFormation pick a name. Do not change it on a live stack: that replaces the bucket. |
| `RepositoryName`, `QueueName` | `clingen-index`, `clingen` | |

On a new account the compute environment can report that the EC2 Spot
service-linked role is missing. Create it once with
`aws iam create-service-linked-role --aws-service-name spot.amazonaws.com`.

Read the three outputs back. The launcher needs them too, so keep them:

```bash
output() {
  aws cloudformation describe-stacks --region "$REGION" --stack-name "$STACK" \
    --query "Stacks[0].Outputs[?OutputKey=='$1'].OutputValue" --output text
}
BUCKET=$(output BucketName)
QUEUE=$(output JobQueueName)
REPO=$(output RepositoryUri)      # <account>.dkr.ecr.<region>.amazonaws.com/clingen-index
echo "$BUCKET $QUEUE $REPO"
aws batch describe-job-queues --region "$REGION" --job-queues "$QUEUE" \
  --query 'jobQueues[0].[state,status]' --output text      # expect: ENABLED VALID
```

## 2. Build and push the image

The image is built from the repository's `Dockerfile`: Debian bookworm slim
with the `clingen-index` binary, AWS CLI v2 (2.37.12) and the few tools the
tasks call. It has no Python and no entrypoint, so Nextflow's own bash launcher
runs. The base images and the AWS CLI are pinned by digest and checksum; the
Debian packages are not, so the image is built from pinned base and tools but
two builds can differ.

The AWS CLI is in the image because Nextflow's `awsbatch` executor has each
task copy its files to and from S3 with it. Do not set `aws.batch.cliPath`:
Nextflow would then mount the instance's directory over the image's. The
profile leaves it unset on purpose.

The image architecture must match the compute environment's instances:

```bash
aws ecr get-login-password --region "$REGION" | docker login --username AWS --password-stdin "${REPO%%/*}"

# x86-64 instances (the default InstanceTypes=optimal), from any machine with buildx:
docker buildx build --platform linux/amd64 -t "$REPO:0.1.0" --push .
# or arm64 (Graviton) instances, deployed with InstanceTypes=default_arm64:
docker buildx build --platform linux/arm64 -t "$REPO:0.1.0" --push .
```

Without buildx, the plain `docker build` makes an image for the architecture of
the machine it runs on. On an x86-64 host:

```bash
docker build -t "$REPO:0.1.0" . && docker push "$REPO:0.1.0"
```

On an arm64 host (an Apple-silicon Mac, a Graviton instance) the same
command makes an arm64 image; deploy the stack with
`InstanceTypes=default_arm64` (Graviton instances) so the instances and the
image agree. Building for the other architecture needs buildx and emulation.
Buildx was not available where this guide was written, so the `--platform`
builds are shown but were not run.

Two notes for building on a Mac with Colima: only `$HOME` is shared with the
VM, so build from a checkout under `$HOME`; and the build downloads the AWS CLI
and Debian packages, so it needs the network. The Dockerfile's comments explain
its choices; `.dockerignore` lets only what it copies (the Cargo files and
`crates/`) into the build context, so `data/`, `target/` and run outputs stay out.

## 3. The launcher

The launcher is the machine that runs `nextflow run`. It does not do the
work: it submits Batch jobs, watches them and publishes the results. It has to
stay up for the whole run, because Nextflow keeps its state for `-resume` in a
`.nextflow/` directory in the launch directory.

### Permissions

The launcher needs its own permissions to submit jobs and to use the bucket.
This is the Nextflow AWS documentation's list, with the S3 part narrowed to the
stack's bucket. The ECR statement is in that list, although the launcher never
pulls the image; if you want less, drop it and add it back only if Nextflow
reports an access error.

```bash
ACCOUNT=$(aws sts get-caller-identity --query Account --output text)

cat > launcher-policy.json <<EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "Batch", "Effect": "Allow", "Resource": "*", "Action": [
      "batch:CancelJob", "batch:DescribeComputeEnvironments", "batch:DescribeJobDefinitions",
      "batch:DescribeJobQueues", "batch:DescribeJobs", "batch:ListJobs", "batch:RegisterJobDefinition",
      "batch:SubmitJob", "batch:TagResource", "batch:TerminateJob"]},
    {"Sid": "Ec2EcsDescribe", "Effect": "Allow", "Resource": "*", "Action": [
      "ec2:DescribeInstanceAttribute", "ec2:DescribeInstances", "ec2:DescribeInstanceStatus",
      "ec2:DescribeInstanceTypes", "ecs:DescribeContainerInstances", "ecs:DescribeTasks"]},
    {"Sid": "EcrRead", "Effect": "Allow", "Resource": "*", "Action": [
      "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:DescribeImages", "ecr:DescribeImageScanFindings",
      "ecr:DescribeRepositories", "ecr:GetAuthorizationToken", "ecr:GetDownloadUrlForLayer", "ecr:GetLifecyclePolicy",
      "ecr:GetLifecyclePolicyPreview", "ecr:GetRepositoryPolicy", "ecr:ListImages", "ecr:ListTagsForResource"]},
    {"Sid": "WorkBucket", "Effect": "Allow", "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::$BUCKET"},
    {"Sid": "WorkObjects", "Effect": "Allow", "Action": [
      "s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:PutObjectTagging", "s3:AbortMultipartUpload"],
      "Resource": "arn:aws:s3:::$BUCKET/*"},
    {"Sid": "FailedJobLogs", "Effect": "Allow", "Action": ["logs:GetLogEvents"],
      "Resource": "arn:aws:logs:$REGION:$ACCOUNT:log-group:/aws/batch/job:*"}
  ]
}
EOF
```

`s3:ListBucket` is not in the Nextflow list but its S3 client calls it, and
`logs:GetLogEvents` lets Nextflow print the log of a failed job. If you run
with `aws.batch.jobRole` or `executionRole` set, the launcher also needs
`iam:PassRole` for them; this pipeline sets neither.

If you run the launcher from your own machine, attach this policy (or equivalent
rights) to your user. For a 31-hour run, give it to an instance role:

```bash
aws iam create-role --role-name clingen-launcher --assume-role-policy-document \
  '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
aws iam put-role-policy --role-name clingen-launcher --policy-name nextflow-launcher \
  --policy-document file://launcher-policy.json
aws iam create-instance-profile --instance-profile-name clingen-launcher
aws iam add-role-to-instance-profile --instance-profile-name clingen-launcher --role-name clingen-launcher
```

A new instance profile can take a few seconds to be usable; if launching an
instance with it fails, wait and retry.

### Where to run it

**A small EC2 instance, with the run in `tmux`.** This is the simplest
arrangement. Launch an Amazon Linux 2023 instance with the `clingen-launcher`
instance profile, in a subnet with internet access, with IMDSv2 required. A
`t3.medium` (2 vCPU, 4 GiB) is a safe size; Nextflow's memory use over a run
this long was not measured, so a smaller one may do. It needs the plugin
downloads (`nf-schema`, `nf-amazon`) on its first run, so it needs the
internet too. Reach it with SSH or Session Manager, as you prefer.

Copy the `pipeline/` directory to it (`git clone` if you have pushed the
repository, otherwise
`rsync -a --exclude .nf-test --exclude work --exclude '.nextflow*' pipeline/ <instance>:pipeline/`,
which leaves out what local test runs wrote), then install Java and Nextflow
from the repository's pinned environment. The header of
`pipeline/environment.yml` explains the micromamba settings, and why Java must
be put on `PATH` by hand:

```bash
sudo dnf install -y tmux
curl -L https://micro.mamba.pm/api/micromamba/linux-64/latest | tar -xj bin/micromamba
./bin/micromamba create -y -p "$HOME/clingen-nf" -f pipeline/environment.yml
export JAVA_HOME="$HOME/clingen-nf/lib/jvm" PATH="$HOME/clingen-nf/bin:$JAVA_HOME/bin:$PATH"
nextflow -version                       # 26.04.7
```

Set `REGION`, `BUCKET`, `QUEUE` and `REPO` on the instance to the values above
(the instance has no permission to read the stack), and start the run
inside `tmux` so that it survives a dropped connection:

```bash
tmux new -s clingen
# ... the nextflow run command from step 4; detach with Ctrl-b d, come back with:
tmux attach -t clingen
```

If the instance is rebooted, the run stops with it. Start the instance again, run
the same `nextflow run` command in the same directory, and `-resume` carries on
from the tasks that finished. Stop the instance rather than terminating it
between sessions, since the launch directory is on its disk.

**A Batch head job.** Nextflow can also run as a Batch job of its own, which
leaves nothing for you to keep alive. This guide does not set it up, and it was
not tested. It would need what the stack does not create: a job role that
carries the launcher policy, an image that has Java and Nextflow (the task image
does not), and an on-demand queue, so that a Spot reclaim cannot end the run.
A second stack with `ComputeType=EC2` gives one if you also override
`RepositoryName` and `QueueName`: the defaults (`clingen-index`, `clingen`)
belong to the first stack, and the deploy fails on them. That stack also
creates a bucket and a repository of its own, which the head job would not
use. The launch directory of a job is discarded when it ends, so `-resume`
after a failure needs Nextflow's cloud cache (`cloudcache.enabled` and
`cloudcache.path` pointing into the bucket); check the Nextflow documentation
for it before relying on it.

## 4. Run

First a small run, to prove the plumbing. Chromosome MT takes minutes and a few
cents. It downloads about a gigabyte (the hg38 genome, UCSC's gene tables and
ClinVar) and makes a few read-only calls to the Registry:

```bash
nextflow run pipeline -profile awsbatch --chrom MT --regions genes \
  --aws_queue "$QUEUE" --container "$REPO:0.1.0" \
  -work-dir "s3://$BUCKET/work" --outdir "s3://$BUCKET/results/mt"
```

The first jobs take a few minutes to start while Batch brings up an instance.
Then the full run: the protein-coding exons of every chromosome, with ClinVar.

```bash
nextflow run pipeline -profile awsbatch --chrom all --regions pc_exons \
  --aws_queue "$QUEUE" --container "$REPO:0.1.0" \
  -work-dir "s3://$BUCKET/work" --outdir "s3://$BUCKET/results/pc_exons" -resume
```

- `-work-dir` must be in S3 and must name a sub-path (`.../work`), not the
  bucket root. The instance role can reach only this stack's bucket, so the work
  directory has to be in it.
- `--chrom all` with `--regions pc_exons` is about 60,328 Registry requests. The
  Registry steps (`ENUMERATE` and `LOOKUP`) run one task at a time each, by
  design (the two can overlap), so the run takes roughly **31 hours** on the
  one `ENUMERATE` stream whatever `MaxvCpus` is. The launcher must stay up
  that long.
- `-resume` continues this regeneration after a stop: Nextflow reuses every
  task that finished and runs only the rest; it does nothing on a first run.
  It does not start a new regeneration. Next month, the same command from the
  same launch directory would reuse the `latest` ClinVar and UCSC downloads
  and every `ENUMERATE` batch still in `work/`, and publish last month's index
  again. For a new regeneration (newer ClinVar, gnomAD or UCSC data, alleles
  registered since), run from a fresh launch directory, or without `-resume`.
  A new `-work-dir` alone is not enough: `-resume` finds the earlier tasks
  through `.nextflow/` in the launch directory, wherever their files are.
- `nextflow run pipeline --help` lists the parameters; `--showHidden` adds the
  hidden ones (task sizes such as `--page_size`, and the endpoints).

Watch it with `tail -f .nextflow.log` on the launcher, or
`aws batch list-jobs --region "$REGION" --job-queue "$QUEUE" --job-status RUNNING`.
If a Spot instance is reclaimed, Batch runs the tasks it was running again, from
the start, up to three attempts each (`maxSpotAttempts` in the profile). If your
stack is not in `us-east-1`, add `--aws_region <region>` to the command: the
profile's default is `us-east-1`.

### The genome: leave `--genome` out

Do not pass `--genome` on AWS Batch. A genome you supply must be a local or
shared path that the `BUILD_INDEX` task can open by the name the launcher gave
it, and a task on Batch sees neither your workstation nor an `s3://` URL; with an
`s3://` work directory the run refuses any `--genome` when it starts, rather than
fail after the whole enumeration. Without it, the `BUILD_GENOME`
task downloads hg38 from UCSC and builds it, and the run publishes it under
`<outdir>/genome/`. The manifest in `index/` records it as a path relative to the
index, so keep `index/` and `genome/` side by side when you move them.

### gnomAD on AWS

Only the gnomAD datasets use this. By default the pipeline reads ClinVar only;
add `--datasets clinvar,gnomad_genomes` (or `gnomad_exomes`) to look up gnomAD's
sites too. `--gnomad_mirror aws` then downloads gnomAD from the AWS Open Data
copy (`gnomad-public-us-east-1`) instead of Google Cloud Storage. In
`us-east-1` the transfer from S3 to the instance is free (a NAT gateway would still
charge for the data it processes); from another region it is billed as
cross-region transfer, so keep the default `gcs` there. The files are fetched
over HTTPS, so the instance role needs no access to that bucket. The file listing
and the md5 sums that are checked still come from Google Cloud Storage, so the
instances must reach it as well as the AWS mirror. A full gnomAD genomes
run is far larger than the exon run: chr21 alone has 10,958,898 sites in gnomAD
v4.1, so the whole genome is on the order of hundreds of millions of lookups. The
cost estimate below does not cover it.

### Results

When the run ends, `s3://$BUCKET/results/pc_exons/` holds:

- `index/`: `caid.fst` and `manifest.json`, the index itself;
- `genome/`: the genome the index was built against;
- `harvest/<chrom>.tsv.gz`: the Registry's answers for every site, by chromosome; pass the directory
  to a later run as `--cache` to skip the Registry for sites already answered
  (a local directory is the tested case; an `s3://` one is not);
- `reports/`: `summary.md` states the result of the publish gate and the
  parameters, `coverage.tsv` has the counts in long format (columns `chrom`,
  `source`, `measure`, `value`), and `sources.tsv`, `skipped.tsv.gz`,
  `validate_<chrom>.txt` and `reach_<chrom>.txt` back them up;
- `pipeline_info/`: Nextflow's trace, timeline and report.

`index/` is published only if every check passed (or you gave
`--allow_mismatches`). `FINALIZE`, the step that publishes it, waits for the
report, so `reports/` is there even when the gate stops the run; a failure of
the report step itself also blocks publishing.

Fetch it:

```bash
aws s3 sync "s3://$BUCKET/results/pc_exons" pc_exons
less pc_exons/reports/summary.md
```

### Resuming and the work expiry

`-resume` reuses the tasks whose work files are still in `s3://$BUCKET/work/`.
The bucket expires objects under `work/` after `WorkExpirationDays` (30)
days from when they were written, so a resume within a month of the failure is
complete and one after that re-runs what expired, including the Registry steps.
If you expect a longer gap, raise the parameter by redeploying the stack with
`WorkExpirationDays=<n>`. Nothing under `results/` expires.

A resumed run's manifest keeps the dates of the answers it reuses: its
`registry_pc_exons_<date>` source is the UTC day the earliest `ENUMERATE` task ran,
and its `harvest_date` the UTC day of the earliest `ENUMERATE` or `LOOKUP` task;
each task records its day, and a task `-resume` reuses keeps it. A 31-hour run
that crosses midnight UTC keeps the day it began, whether or not it is stopped
and resumed. The submit times in `pipeline_info/trace.txt` show when each task
was submitted.

Resuming is for finishing one regeneration. Within the work expiry, a `-resume` from
the same launch directory also reuses the downloads and the `ENUMERATE` batches of
the earlier run (see the notes under [4. Run](#4-run)), so start a new regeneration
from a fresh launch directory, or without `-resume`.

## Costs

These are estimates, not measurements: nothing here has run on AWS. They are
for one full `--chrom all --regions pc_exons` run with ClinVar, about 36 hours
from start to finish, in `us-east-1`, with approximate us-east-1 on-demand list
prices at the time of writing (they change, so check the AWS pricing pages), and
Spot at about a third of the on-demand price (Spot prices move). Batch places a
job only on an instance whose memory, less what ECS reserves for itself, covers
what the job asks: a 4-GiB `ENUMERATE` job does not fit a 4-GiB c6i.large, nor a
16-GiB sort job a 16-GiB m6i.xlarge.

| Item | Assumption | Estimate |
|---|---|---|
| Registry steps | One 2-vCPU, 8-GiB instance (m6i.large, $0.096/h) for 31 h, holding the 4-GiB `ENUMERATE` and 2-GiB `LOOKUP` tasks | about $1 on Spot, $3 on-demand |
| Sort and build steps | 4-vCPU, 16-GiB tasks on r6i.xlarge ($0.252/h) or m6i.2xlarge ($0.384/h), 6 h in all (a guess) | about $0.5 to $0.8 on Spot, $1.5 to $2.3 on-demand |
| Root volumes | 200 GiB gp3 at $0.08 per GiB-month, up to three instances for 36 h | under $3 |
| Launcher | t3.medium ($0.0416/h) for 36 h | about $1.5 |
| NAT gateway | None in public subnets. In private subnets: $0.045/h plus $0.045 per GB processed | about $1.6 plus data |
| S3 | 20 GB under `work/` for a month, at $0.023 per GB-month, plus the results | under $1 |
| ECR | A 0.5-GB image at $0.10 per GB-month | cents |
| AWS Batch | No charge of its own | none |

On these assumptions a full run comes to something like **$5 to $12**. The
largest unknowns are the sort steps' time and the volume of data in `work/`, and
a full gnomAD run is a different order of magnitude. Budget alarms are cheap
insurance; set one before the long run. The 36 hours of launcher, volumes and
(if present) NAT gateway do not depend on Spot.

## Fusion instead of the AWS CLI in the image

The Nextflow AWS documentation recommends Wave and Fusion over copying files
with the AWS CLI, and calls the CLI requirement legacy. With Fusion, tasks see
S3 as a file system, so neither the image nor the AMI needs the CLI. This
pipeline does not use it: Fusion's client is added to the container by Wave, a
service run by Seqera, and the pipeline's tests and this template were made for
the CLI route. To try it, set `fusion.enabled = true` and `wave.enabled = true`
in a config file given with `-c`, and read Nextflow's AWS documentation first;
that route was not tested.

## Troubleshooting

- **Jobs stay `RUNNABLE`.** Batch cannot start an instance. Look at the compute
  environment (`aws batch describe-compute-environments`) for a status reason,
  then check the vCPU quota, that the subnets have internet access, and that
  `InstanceTypes` matches the image's architecture.
- **Nextflow says a task "terminated for an unknown reason".** Nextflow's
  documentation names two common causes: a container with a non-standard
  entrypoint, which keeps its launcher from running, and an `aws` CLI that does
  not run. This image has no entrypoint and carries the CLI, so check that
  `--container` points to it.
- **`AccessDenied` from S3 in a task.** The instance role covers only this
  stack's bucket. Check `-work-dir`.
- **A task cannot find credentials.** The launch template sets IMDSv2 with a hop
  limit of 2. A replaced launch template needs the same.

## Teardown

The bucket and the repository are deleted with the stack only if they are empty,
so the stack cannot take your results with it by accident. Copy what you want
first, then empty both:

```bash
aws s3 sync "s3://$BUCKET/results" results-from-aws           # keep the results
aws s3 rm "s3://$BUCKET" --recursive

# Every image, untagged ones too: `docker buildx build --push` stores an image index and
# attestation manifests beside the image. A call takes at most 100 ids, and an index has
# to go before the images it lists (a pass may print ImageReferencedByManifestList for
# those, and a later one deletes them), so repeat until the repository is empty.
REPO_NAME=${REPO#*/}                                          # the stack's RepositoryName
for pass in 1 2 3 4 5 6 7 8 9 10; do
  IDS=$(aws ecr list-images --region "$REGION" --repository-name "$REPO_NAME" \
    --max-items 100 --query 'imageIds' --output json)
  [ "$IDS" = "[]" ] && break
  aws ecr batch-delete-image --region "$REGION" --repository-name "$REPO_NAME" \
    --image-ids "$IDS" --query 'failures[].failureCode' --output text
done
aws ecr list-images --region "$REGION" --repository-name "$REPO_NAME" \
  --query 'length(imageIds)'                                  # expect 0

aws cloudformation delete-stack --region "$REGION" --stack-name "$STACK"
aws cloudformation wait stack-delete-complete --region "$REGION" --stack-name "$STACK"
```

The stack's queue and compute environment go with it, so a run still going when
you delete the stack fails. Not in the stack, and yours to remove:

- the launcher instance and the `clingen-launcher` role and instance profile
  (`aws iam remove-role-from-instance-profile`, `delete-instance-profile`,
  `delete-role-policy`, `delete-role`);
- the job definitions Nextflow registered, one per image, named `nf-` and the
  image's name with `.`, `/` and `:` as `-`. They stay `ACTIVE`, though they
  cost nothing; to remove them:

  ```bash
  for def in $(aws batch describe-job-definitions --region "$REGION" --status ACTIVE \
      --query "jobDefinitions[?starts_with(jobDefinitionName, 'nf-') && contains(jobDefinitionName, '${REPO_NAME//\//-}')].jobDefinitionArn" \
      --output text); do
    aws batch deregister-job-definition --region "$REGION" --job-definition "$def"
  done
  ```

- the task logs, in the CloudWatch log group `/aws/batch/job`. Every Batch job in
  the account and region logs there by default, so delete the group
  (`aws logs delete-log-group --region "$REGION" --log-group-name /aws/batch/job`)
  only if nothing else in this account uses AWS Batch in that region. Otherwise
  delete this pipeline's log streams, whose names start with its job
  definitions':

  ```bash
  for stream in $(aws logs describe-log-streams --region "$REGION" --log-group-name /aws/batch/job \
      --log-stream-name-prefix nf- \
      --query "logStreams[?contains(logStreamName, '${REPO_NAME//\//-}')].logStreamName" --output text); do
    aws logs delete-log-stream --region "$REGION" --log-group-name /aws/batch/job --log-stream-name "$stream"
  done
  ```
