# clingen

Offline ClinGen Allele Registry annotation for
[vep-rs](https://github.com/natera-open-source/vep-rs) and
[fastVEP](https://github.com/Huang-lab/fastVEP).

The [ClinGen Allele Registry](https://reg.clinicalgenome.org) gives every
genetic variant a canonical allele identifier, the **CA ID** (e.g.
`CA120560`). This project:

1. **Harvests** CA IDs and canonical HGVS from the Registry once, using its
   read-only lookup API, for a defined set of sites (starting with gnomAD v4.1).
2. **Indexes** them locally as an HGVS → CA ID map.
3. **Annotates** VCFs inside vep-rs and fastVEP from that index, with **no
   network calls**. Each variant is first converted to the Registry's canonical
   genomic HGVS by [vareffect](https://docs.rs/vareffect), then looked up.

The key is HGVS rather than CHROM/POS/REF/ALT because HGVS is normalized. The
same indel can be written several ways in a VCF (left-aligned, right-aligned,
padded), but it has exactly one canonical HGVS, and so one CA ID.

## What you can do with CA IDs

- **Name a variant the same way everywhere.** One CA ID covers the allele on
  GRCh38, on GRCh37, and in its transcript (`c.`) forms. Protein-level changes
  get their own `PA` IDs, linked from the CA record. The Registry returns the
  equivalent forms together (e.g. `NC_000021.9:g.5030578C>T` and
  `NC_000021.8:g.…` for the same CA ID). Join datasets from different
  assemblies, callers and normalizers on one key.
- **Deduplicate variant calls.** Two VCF records that describe the same allele
  differently get the same CA ID. See `ins_left` and `ins_right` in the recipe
  below.
- **Reach ClinGen curation.** Expert-panel classifications in the
  [ClinGen Evidence Repository](https://erepo.clinicalgenome.org) and evidence in
  the ClinGen Linked Data Hub are indexed by CA ID. Look up
  `https://reg.clinicalgenome.org/allele/<CAID>` for the allele's record.
- **Cross-reference other databases.** Registry records link a CA ID to dbSNP
  rsIDs, ClinVar variation IDs, gnomAD and other external records. That makes a
  CA ID a convenient bridge between resources that each use their own
  identifier.
- **Share data without ambiguity.** Exchanging CA IDs instead of raw VCF lines
  removes the "which normalization did you use?" question between labs.

## Status

| Piece | State |
|---|---|
| Harvester (`harvest/`): Registry lookup for a sites list | **works**; the recipe below runs it |
| gnomAD v4.1 chr21 harvest | in progress |
| `clingen-allele` crate (vareffect HGVS + local index) | planned |
| `clingen-index` CLI (`build`, `validate`) | planned |
| vep-rs plugin `--plugin ClinGen` | planned |
| fastVEP provider (`FV_CLINGEN`) | planned |

Design: [`docs/superpowers/specs/2026-10-23-clingen-allele-plugin-design.md`](docs/superpowers/specs/2026-10-23-clingen-allele-plugin-design.md).
Implementation plan: [`docs/superpowers/plans/2026-10-23-clingen-allele-plugin.md`](docs/superpowers/plans/2026-10-23-clingen-allele-plugin.md).

## Recipe: look up a toy VCF

This recipe uses the harvester's lookup against the live Registry, so it needs
network access. That is the same call the harvester makes to build the
offline index. Requirements: `bash`, `curl`, `awk` and Python ≥ 3.9 (standard
library only).

[`examples/toy.vcf`](examples/toy.vcf) (GRCh38):

```
##fileformat=VCFv4.2
##reference=GRCh38
##contig=<ID=chr21,length=46709983>
##contig=<ID=chrM,length=16569>
#CHROM	POS	ID	REF	ALT	QUAL	FILTER	INFO
chr21	5030578	snv	C	T	.	PASS	.
chr21	13189782	ins_left	C	CA	.	PASS	.
chr21	13189788	ins_right	A	AA	.	PASS	.
chr21	33000166	del_novel	TA	T	.	PASS	.
chrM	3243	melas	A	G	.	PASS	.
chr21	10000000	bad_ref	A	G	.	PASS	.
```

Run:

```bash
examples/lookup_vcf.sh examples/toy.vcf /tmp/toy_out | column -t -s $'\t'
```

Result (2026-10-23):

```
chunk 0: 6 sites in 0s {'registered': 4, 'unregistered': 1, 'error': 1}
ID         CHROM  POS       REF  ALT  CAID                            HGVS_GRCh38
snv        21     5030578   C    T    CA749862939                     NC_000021.9:g.5030578C>T
ins_left   21     13189782  C    CA   CA636937543                     NC_000021.9:g.13189788dup
ins_right  21     13189788  A    AA   CA636937543                     NC_000021.9:g.13189788dup
del_novel  21     33000166  TA   T    -                               NC_000021.9:g.33000172del
melas      MT     3243      A    G    CA120560                        NC_012920.1:m.3243A>G
bad_ref    21     10000000  A    G    ERROR:IncorrectReferenceAllele  -
```

What each row shows:

- **`snv`**: a registered SNV gets its CA ID and canonical HGVS.
- **`ins_left` / `ins_right`**: the same one-base insertion in a run of A's,
  written left-aligned and right-aligned. Both become
  `NC_000021.9:g.13189788dup` (shifted 3′ and written as a duplication, as HGVS
  requires) and both get `CA636937543`.
- **`del_novel`**: a valid deletion the Registry has never registered. The
  canonical HGVS is still computed, but there is no CA ID. Lookups never
  register new alleles.
- **`melas`**: mitochondrial m.3243A>G, the classic MELAS variant. The
  mitochondrial genome uses the `m.` prefix.
- **`bad_ref`**: REF doesn't match GRCh38 (the reference base is `C`). The
  Registry rejects the line, and the planned plugins will leave it
  unannotated.

Full output, including the GRCh37 HGVS column, is in
`/tmp/toy_out/chunks/chunk_00000.tsv`.

## Planned: annotating a VCF offline

These commands are from the implementation plan and **do not work yet**:

```bash
# one-time: vareffect's GRCh38 genome binary (~3.1 GB) and the CA index
vareffect-cli setup ...                      # see vareffect docs
clingen-index build --chunks data/chr21_chunks --out data/index_chr21

# vep-rs (built with --features clingen)
vep -i toy.vcf --vcf -o out.vcf --plugin ClinGen,index=data/index_chr21,genome=GRCh38.bin
#   → CSQ subfields ClinGen_CAID, ClinGen_HGVSg

# fastVEP (built with --features clingen)
fastvep annotate -i toy.vcf -o out.vcf --sa-dir sa/ --clingen-genome GRCh38.bin
#   → INFO FV_CLINGEN=ALLELE|CAID|HGVSG
```

The offline results must match the recipe's CAID and HGVS columns; that
agreement is one of the plan's acceptance tests.

## Harvesting

```bash
harvest/extract_gnomad_sites.sh 21 data/gnomad41_chr21_sites.tsv     # stream gnomAD, keep CHROM POS REF ALT
python3 -I harvest/query_registry.py data/gnomad41_chr21_sites.tsv data/chr21_chunks
```

`query_registry.py`:

- only calls `POST /alleles` (lookup) and never registers alleles;
- sends 250k sites per request;
- retries with backoff;
- skips chunks already written, so re-running it resumes;
- writes one TSV per chunk with columns
  `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`.

The Registry is a free public service. Please contact ClinGen before
harvesting beyond gnomAD-scale site lists.

## Layout

```
harvest/     Registry harvester (Python, standard library only)
examples/    toy VCF and lookup recipe
docs/        design spec and implementation plan
data/ logs/  harvest output (gitignored)
```
