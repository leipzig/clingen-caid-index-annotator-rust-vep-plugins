#!/usr/bin/env bash
# Build the chr21-only vareffect genome binary used by the conformance tests
# and by the chr21 index: download UCSC hg38 chr21 and write
# <out_dir>/chr21.bin and <out_dir>/chr21.bin.idx (contig chr21, build GRCh38).
# Usage: scripts/build_chr21_genome.sh <out_dir>
# Needs curl, gunzip and cargo on PATH.
set -euo pipefail
if [ "$#" -ne 1 ]; then
  echo "usage: $0 <out_dir>" >&2
  exit 2
fi
url="https://hgdownload.soe.ucsc.edu/goldenPath/hg38/chromosomes/chr21.fa.gz"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$1"
out_dir="$(cd "$1" && pwd)"

# The download is untrusted data: it gets a new, empty directory of its own
# and is only ever read, never executed.
download_dir="$(mktemp -d "$out_dir/download.XXXXXX")"
curl -sS --fail --retry 5 -o "$download_dir/chr21.fa.gz" "$url"
echo "downloaded $url"
shasum -a 256 "$download_dir/chr21.fa.gz"
gunzip -c "$download_dir/chr21.fa.gz" > "$download_dir/chr21.fa"

cargo run --release --quiet --manifest-path "$repo/Cargo.toml" \
  -p clingen-allele --example fasta_to_bin -- \
  "$download_dir/chr21.fa" "$out_dir/chr21.bin"
rm -r "$download_dir"
ls -l "$out_dir/chr21.bin" "$out_dir/chr21.bin.idx"
