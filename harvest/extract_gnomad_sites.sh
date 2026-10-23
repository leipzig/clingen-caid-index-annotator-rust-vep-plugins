#!/usr/bin/env bash
# Stream a gnomAD v4.1 genomes sites VCF and keep only CHROM POS REF ALT,
# with "chr" stripped (the Registry wants bare contig names).
# Usage: extract_gnomad_sites.sh <chrom> <out.tsv>
set -euo pipefail
chrom="$1"
out="$2"
url="https://storage.googleapis.com/gcp-public-data--gnomad/release/4.1/vcf/genomes/gnomad.genomes.v4.1.sites.chr${chrom}.vcf.bgz"

curl -sS --fail --retry 5 "$url" \
  | gunzip -c \
  | awk -F'\t' -v OFS='\t' '!/^#/ { sub(/^chr/, "", $1); print $1, $2, $4, $5 }' \
  > "$out.partial"
mv "$out.partial" "$out"
echo "wrote $(wc -l < "$out") sites to $out"
