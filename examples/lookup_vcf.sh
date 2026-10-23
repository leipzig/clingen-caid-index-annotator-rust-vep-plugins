#!/usr/bin/env bash
# Look up CA IDs and canonical HGVS for every biallelic record in a GRCh38 VCF
# using the ClinGen Allele Registry lookup endpoint (read-only, no login).
# Usage: examples/lookup_vcf.sh <in.vcf> <out_dir>
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
vcf="$1"
out="$2"
mkdir -p "$out"

# CHROM POS REF ALT with Registry contig names (chr21 -> 21, chrM -> MT).
grep -v '^#' "$vcf" \
  | awk -F'\t' -v OFS='\t' '{ c = $1; sub(/^chr/, "", c); if (c == "M") c = "MT"; print c, $2, $4, $5 }' \
  > "$out/sites.tsv"

python3 -I "$here/harvest/query_registry.py" "$out/sites.tsv" "$out/chunks" >&2

# Join results back to the VCF IDs for display.
paste <(grep -v '^#' "$vcf" | cut -f3) "$out"/chunks/chunk_*.tsv \
  | awk -F'\t' -v OFS='\t' 'BEGIN { print "ID", "CHROM", "POS", "REF", "ALT", "CAID", "HGVS_GRCh38" }
                            { print $1, $2, $3, $4, $5, ($6 == "" ? "-" : $6), ($7 == "" ? "-" : $7) }'
