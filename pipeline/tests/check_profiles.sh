#!/usr/bin/env bash
# Checks what each profile resolves to, with `nextflow config`, so an edit that loses a
# profile's executor, container engine or queue is caught without a cluster or an AWS account.
#
#   bash pipeline/tests/check_profiles.sh
#
# Needs nextflow on PATH; the awsbatch profile loads the nf-amazon plugin, which Nextflow
# downloads once on first use. `nextflow config` takes no --params, and params from a later
# -c file do not reach the profiles, so the checks that need params include the profile's
# conf/<profile>.config from a scratch config that sets them first. `nextflow config` does
# not check option names either, so each profile is also parsed by `nextflow run -preview`
# (no task runs), which warns about an unknown option.
set -euo pipefail

pipeline=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export NXF_DISABLE_CHECK_LATEST=true   # no update check: this script needs no network
failures=0

# resolve NAME DIR [PROFILE]: the flat config of DIR (with PROFILE) into $scratch/NAME.txt.
# Run in $scratch so .nextflow.log lands there; an error or a warning fails.
resolve() {
    local name=$1 dir=$2 profile=${3:-}
    local args=(config "$dir" -o flat)
    [[ -n $profile ]] && args+=(-profile "$profile")
    if ! (cd "$scratch" && nextflow "${args[@]}" > "$name.txt" 2> "$name.err"); then
        echo "FAIL $name: nextflow config exited non-zero:" >&2
        cat "$scratch/$name.err" >&2
        failures=$((failures + 1))
    elif grep -q WARN "$scratch/$name.err"; then
        echo "FAIL $name: nextflow config warned:" >&2
        cat "$scratch/$name.err" >&2
        failures=$((failures + 1))
    fi
}

# has NAME LINE / lacks NAME TEXT: LINE is (or TEXT is not) in NAME's flat config.
has() {
    if grep -qxF -- "$2" "$scratch/$1.txt"; then
        echo "ok   $1: $2"
    else
        echo "FAIL $1: no line \`$2\`" >&2
        failures=$((failures + 1))
    fi
}
lacks() {
    if grep -qF -- "$2" "$scratch/$1.txt"; then
        echo "FAIL $1: has \`$(grep -F -- "$2" "$scratch/$1.txt" | head -1)\`" >&2
        failures=$((failures + 1))
    else
        echo "ok   $1: no $2"
    fi
}

# with_params NAME PROFILE PARAMS...: a scratch project that sets PARAMS, then includes
# conf/PROFILE.config, resolved into NAME.
with_params() {
    local name=$1 profile=$2
    shift 2
    mkdir -p "$scratch/$name.project"
    {
        printf 'params {\n'
        printf '    %s\n' "$@"
        printf '}\n'
        printf "includeConfig '%s/conf/%s.config'\n" "$pipeline" "$profile"
    } > "$scratch/$name.project/nextflow.config"
    resolve "$name" "$scratch/$name.project"
}

resolve local "$pipeline" local
has   local "process.executor = 'local'"
has   local "docker.enabled = true"
has   local "process.container = 'clingen-index:0.1.0'"

resolve slurm "$pipeline" slurm
has   slurm "process.executor = 'slurm'"
has   slurm "apptainer.enabled = true"
has   slurm "apptainer.autoMounts = true"
has   slurm "process.container = 'docker://clingen-index:0.1.0'"

resolve awsbatch "$pipeline" awsbatch
has   awsbatch "process.executor = 'awsbatch'"
has   awsbatch "aws.region = 'us-east-1'"
lacks awsbatch "cliPath"

resolve local_host "$pipeline" local,host
has   local_host "process.executor = 'local'"
has   local_host "process.container = null"
has   local_host "docker.enabled = false"
lacks local_host "process.container = '"

# slurm with partitions: network tasks go to --network_queue, or to --slurm_queue without it.
with_params slurm_queues slurm \
    "container = 'ghcr.io/you/clingen-index:0.1.0'" "slurm_queue = 'normal'" "network_queue = 'transfer'"
has   slurm_queues "process.queue = 'normal'"
has   slurm_queues "process.'withLabel:network'.queue = 'transfer'"
has   slurm_queues "process.container = 'docker://ghcr.io/you/clingen-index:0.1.0'"

with_params slurm_one_queue slurm \
    "container = 'clingen-index:0.1.0'" "slurm_queue = 'normal'" "network_queue = null"
has   slurm_one_queue "process.'withLabel:network'.queue = 'normal'"

# An image with a scheme, or an Apptainer image file, is used as given.
with_params slurm_scheme slurm \
    "container = 'oras://ghcr.io/you/clingen-index:0.1.0'" "slurm_queue = null" "network_queue = null"
has   slurm_scheme "process.container = 'oras://ghcr.io/you/clingen-index:0.1.0'"

with_params slurm_sif slurm \
    "container = '/shared/images/clingen-index.sif'" "slurm_queue = null" "network_queue = null"
has   slurm_sif "process.container = '/shared/images/clingen-index.sif'"

# awsbatch takes its queue, image and region from the params.
with_params awsbatch_params awsbatch \
    "container = '123456789012.dkr.ecr.eu-west-1.amazonaws.com/clingen-index:0.1.0'" \
    "aws_queue = 'clingen-spot'" "aws_region = 'eu-west-1'"
has   awsbatch_params "process.queue = 'clingen-spot'"
has   awsbatch_params "process.container = '123456789012.dkr.ecr.eu-west-1.amazonaws.com/clingen-index:0.1.0'"
has   awsbatch_params "aws.region = 'eu-west-1'"

# Every profile's options are ones Nextflow knows. The options are checked before the script
# runs; awsbatch then stops at its first process for want of an S3 work directory, which
# this offline check does not give it.
for profile in local slurm awsbatch local,host; do
    status=0
    (cd "$scratch" && nextflow run "$pipeline" -profile "$profile" -preview -ansi-log false \
        --chrom 21 --outdir "$scratch/preview" > "preview.txt" 2>&1) || status=$?
    expected_stop=false
    [[ $profile == awsbatch ]] && grep -q 'an S3 bucket must be provided' "$scratch/preview.txt" && expected_stop=true
    if grep -q 'Unrecognized config option' "$scratch/preview.txt"; then
        echo "FAIL $profile: $(grep 'Unrecognized config option' "$scratch/preview.txt" | head -1)" >&2
        failures=$((failures + 1))
    elif ((status != 0)) && [[ $expected_stop == false ]]; then
        echo "FAIL $profile: nextflow run -preview failed:" >&2
        cat "$scratch/preview.txt" >&2
        failures=$((failures + 1))
    else
        echo "ok   $profile: every config option is known"
    fi
done

if ((failures > 0)); then
    echo "$failures profile check(s) failed" >&2
    exit 1
fi
echo "all profile checks passed"
