# Known differences from the ClinGen Allele Registry

`clingen-allele` keys each variant by the HGVS that `canonical_hgvs` computes
from the genome, and that key must be byte-identical to the Registry's GRCh38
`NC_` HGVS. This file records every pattern where the two differ, why, and
which conformance-sample rows are exempt because of it.

**On chr21 there are none.** All 10,958,898 harvested rows match the
Registry's HGVS, so the exclusion list below is empty and
`crates/clingen-allele/tests/hgvs_examples.rs` needed no new rows.

## chr21 validation (2026-10-23)

- **Harvest:** gnomAD v4.1 genomes chr21 sites, looked up on 2026-10-23 with
  `POST https://reg.genome.network/alleles` (`harvest/query_registry.py`):
  10,958,898 rows in 44 chunks, all registered (0 unregistered, 0 errors),
  1,642,087 of them indels (REF and ALT lengths differ).
- **Genome:** UCSC hg38 `chr21.fa.gz`, converted by
  `scripts/build_chr21_genome.sh data/genome_chr21` into
  `data/genome_chr21/chr21.bin` (46,709,983 bytes).
- **Command** (release build, 4.4 s wall clock):

  ```bash
  cargo run -p clingen-index --release -- validate --chunks data/chr21_chunks \
      --genome data/genome_chr21/chr21.bin --mismatches data/chr21_mismatches.tsv
  ```

- **Output** (exit 0; the mismatch TSV holds only its header line):

  ```
  rows=10958898 compared=10958898 skipped=0 malformed=0
  class	total	match	pct
  substitution	9316811	9316811	100.0
  del	848421	848421	100.0
  dup	300711	300711	100.0
  ins	492955	492955	100.0
  delins	0	0	0.0
  m	0	0	0.0
  other	0	0	0.0
  ```

Mismatches per class, from the `total` and `match` columns (the `pct` column
rounds, so it can read 100.0 with mismatches left):

| class | rows | mismatches |
|---|---:|---:|
| substitution | 9,316,811 | 0 |
| del | 848,421 | 0 |
| dup | 300,711 | 0 |
| ins | 492,955 | 0 |
| delins | 0 | 0 |
| m | 0 | 0 |
| other | 0 | 0 |

Classes come from the Registry's HGVS. The chr21 harvest has no `m` rows
(those are chrM), and no `delins` or `other` rows: every chr21 site is a
substitution or a plain deletion, duplication or insertion.

## Patterns

None so far. When a harvest shows a mismatch pattern, it is either fixed in
`canonical_hgvs`'s post-processing (with a new `hgvs_examples.rs` row whose
expectation is copied from the harvest) or described here: the pattern, why
it is not fixed, its count per class, and at most three example rows. Every
sample row it affects then goes in the exclusion list.

## Exclusion list

The fenced block below, with info string `known-differences`, is read by
`crates/clingen-allele/tests/conformance.rs`, and the end-to-end check
`scripts/e2e_crosscheck.py` reads it the same way.
Each line is `CHROM POS REF ALT`, whitespace-separated, and names one row of
the conformance sample that is exempt from the Registry comparison (the
end-to-end check exempts the same alleles in a tool's output). Blank
lines are ignored. A malformed or repeated line, or a missing or second
block, fails the reader; the conformance test also fails on a listed row that
is not in the sample or that matches the Registry, so the list stays minimal.

```known-differences
```

## The conformance sample

`crates/clingen-allele/tests/data/chr21_sample.tsv` is 10,000 rows of the
harvest above, unchanged and in the harvest's header-less seven-column format.
It was drawn with Python 3.9.6 by

```bash
python3 -I scripts/sample_harvest.py data/chr21_chunks crates/clingen-allele/tests/data/chr21_sample.tsv
```

with seed 21; re-running it on the same harvest writes the identical file
(SHA-1 `81b37b4772fb2ee0c56a9a3368e6dad8a987e091`). It is stratified:

- all 42 harvest indels whose REF and ALT end in the same base, which a VCF
  left-aligner would move (all deletions of 226 to 330 bases in repeats;
  gnomAD writes the rest of its indels left-aligned already);
- the other rows by class: 7,000 substitutions, 1,528 del, 542 dup and
  888 ins.

By class that is 7,000 substitutions, 1,570 del, 542 dup and 888 ins, so
3,000 indels.

`cargo test -p clingen-allele --test conformance --release` with
`CLINGEN_CHR21_GENOME` set to `chr21.bin` (2026-10-23):

- `sample_matches_registry`: 10,000 rows compared, 0 excluded, all equal to
  the Registry's HGVS.
- `representation_invariance`: each of the 3,000 indels, left-padded,
  right-padded and fully left-aligned (which moved the 42 rows above), gives
  the same HGVS as the row itself.
