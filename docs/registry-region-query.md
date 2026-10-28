# Enumerating the Registry by region: what the query returns, and why it is not paged

The regeneration pipeline fills the index by asking the ClinGen Allele
Registry for every allele it holds in a window of the genome, rather than
looking up sites one by one. This page records what that query does,
measured against the live Registry on 2026-10-24, and why
`clingen-index registry enumerate` splits a crowded window instead of
paging through it.

## The query

```
GET https://reg.genome.network/alleles?refseq=<NC_ accession>&begin=<b>&end=<e>
    &fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome+genomicAlleles.coordinates&limit=<L>
```

It is read-only and needs no login. Each allele comes back with its CA ID and
its HGVS on every reference it is placed on: the GRCh38 `NC_` accession,
GenBank `CM…`, GRCh37 `NC_…`. The pipeline keeps only the HGVS on the
window's own GRCh38 accession, which is the index key. The coordinates give,
for each genome, the allele's bases and the reference's over the same
stretch (`{"start": 3104, "end": 3106, "allele": "AC", "referenceAllele":
"AC"}`); they are what tells the reference alleles below apart.

## What a window returns

- **Alleles lying wholly inside `(begin, end]`**: start after `begin`, end at
  or before `end`. In 50 random 1 kb windows on chr21, 40 of the 14,134
  alleles our corpus holds there did not come back, and every one crosses a
  window edge: 17 start exactly on the window's first base and 23 are
  deletions that run past its end. `regions` therefore widens every window
  by a 200 bp flank on each side, and an allele longer than that which
  crosses a window edge is still missed.
- **Reference alleles too.** About a fifth to a third of what comes back is
  reference-only (`NC_000021.9:g.5030577G=`). Some are not written `=`: the
  Registry also holds reference alleles as a `delins` that puts back the very
  bases it removes. On 2026-10-24 the window NC_012920.1 (3009, 5731] held 13,
  among them `NC_012920.1:m.3105_3106delinsAC` (`CA2499563283`, where the rCRS
  reads `AC`) and `m.5131_5133delinsTAA`, mostly at ClinVar indel sites. The
  50 kb APP window below held 1,369 (`reference=19769 written=39267` instead
  of `reference=18400 written=40636`). The inserted bases of every one match
  the chr21 reference, and none of the 15,528,839 CA IDs harvested from
  ClinVar and gnomAD is among them. Only the coordinates give them away,
  their `allele` equal to their `referenceAllele`, so `enumerate` asks for
  them and treats such an allele as a reference allele whatever its HGVS. No
  annotation tool ever looks one up, so `enumerate` drops them by default
  and only counts them (`reference=` on stderr). `--keep-reference` writes
  them as ordinary `HGVS<TAB>CAID` rows instead (`written=` then includes
  them), and the pipeline's `--include_reference` sets it for a whole run.
- **No hidden cap was seen.** A 50 kb window in APP returned all 57,093
  alleles in one request without `limit`, and 59,036 with `limit=200000`.

## What a request costs

About 0.71 s of fixed time per request plus about 0.10 ms per allele, on one
stream (chr21, APP region, 2026-10-24):

| Window | Alleles | Time | Alleles/s |
|---|---|---|---|
| 1 kb | 1,502 | 0.86 s | 1,752 |
| 5 kb | 5,667 | 1.27 s | 4,466 |
| 20 kb | 22,487 | 3.15 s | 7,129 |
| 50 kb | 57,093 | 6.57 s | 8,695 |

Asking for fewer fields barely helps: dropping `externalRecords` gave 964
instead of 907 alleles/s on the same ten exon windows. The fixed cost is
what matters, which is why `regions` merges exons that lie within
`--merge-gap` (5 kb) of each other into windows of up to `--max-window`
(50 kb): protein-coding exons ±20 bp take 60,328 requests that way instead
of 254,894, about 31 hours instead of about 5 days.

## Paging with `skip` loses alleles

The query also takes `skip`, so the obvious way through a window holding
more than `limit` alleles is to ask again with `skip=<limit>`. On the live
Registry that silently loses alleles.

Window `NC_000021.9` (25,906,000, 25,956,000], in APP:

| Fetch | Requests | Rows written | Unique rows |
|---|---|---|---|
| `limit=50000`, then `skip=50000` | 2 | 40,636 | 40,623 |
| `limit=200000` | 1 | 40,636 | 40,636 |

Both fetches saw 59,036 alleles (18,400 of them reference-only). The paged
one returned 13 alleles twice and 13 others not at all, among them
`NC_000021.9:g.25948006_25948025dup` (`CA1021403343`) and
`NC_000021.9:g.25948014_25948025del` (`CA748243349`). Nothing in the
response says so.

The cause is the order the Registry returns alleles in. It is not by HGVS
start: one response had 1,317 places where the start position goes down and
13,463 pairs of neighbours at the same position. The missing alleles were
dups and dels sharing one end position, so it looks like the alleles are
ordered by an internal coordinate and ties are broken differently from one
request to the next. A page boundary inside such a tie then repeats some
alleles and skips others. Paging by position instead of `skip` would hit the
same ties.

## What `enumerate` does instead

It never sends `skip`. Each window is asked for once, with
`limit=--page-size` (200,000 by default). A response holding exactly `limit`
alleles is treated as possibly incomplete. It is thrown away, the window's
span (b, e] is split at its midpoint m into (b, m + o] and (m − o, e], where
o is `--split-overlap` (1,000 by default), and each half is asked for the
same way, until every answer comes back short. The two halves share 2·o
bases, and rows that both return are written once. A span that is still
full but cannot get any smaller stops the run (exit 2) rather than writing
a window it knows is incomplete. `enumerate` reports `requests=` and
`splits=` on stderr.

The cost is the same kind as the window-edge limit above: an allele longer
than 2·o (2,000 bases by default) that crosses an internal split point is
missed. With protein-coding exon windows of at most 50 kb, a split should be
rare: the densest window measured so far (the 50 kb APP one) holds 59,036
alleles, under the default page size.

### Checked against the same window (2026-10-24)

With the change in place (`clingen-index` at `505a978`), the APP window gives
the same alleles whether or not it has to split:

| Fetch | stderr tally | Rows | Unique |
|---|---|---|---|
| `--page-size 50000` (forces a split) | `windows=1 requests=3 splits=1 alleles=59036 reference=18400 no_grch38_hgvs=0 written=40636` | 40,636 | 40,636 |
| default (`--page-size 200000`) | `windows=1 requests=1 splits=0 alleles=59036 reference=18400 no_grch38_hgvs=0 written=40636` | 40,636 | 40,636 |

The two outputs hold exactly the same rows (`comm -3` prints nothing), and
both contain the two alleles the skip-paged fetch lost. The tests
`enumerate_splits_a_full_window`,
`enumerate_splits_a_full_half_again_first_half_first` and
`enumerate_fails_when_a_full_window_cannot_shrink`
(`crates/clingen-index/tests/registry.rs`) pin the behaviour without the
network.

## Reproducing

With `clingen-index` built (`cargo build --release -p clingen-index`):

```bash
printf '21\tNC_000021.9\t25906000\t25956000\t25906000\t25956000\n' > w.tsv
target/release/clingen-index registry enumerate --windows w.tsv --out split.tsv --page-size 50000
target/release/clingen-index registry enumerate --windows w.tsv --out single.tsv
sort -u split.tsv | wc -l; sort -u single.tsv | wc -l
comm -3 <(sort -u split.tsv) <(sort -u single.tsv) | wc -l   # 0: the same alleles
```

The Registry gains alleles over time, so the counts will grow; the two runs
should still agree with each other.
