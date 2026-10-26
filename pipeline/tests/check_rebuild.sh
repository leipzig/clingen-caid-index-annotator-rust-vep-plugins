#!/usr/bin/env bash
# Rebuilds a published index from its key shards and checks that the bytes are the same.
#
#   bash pipeline/tests/check_rebuild.sh <outdir>
#
# <outdir> is a run's --outdir (a local copy of it for an s3:// one): index/ with caid.fst,
# manifest.json and metadata.json, keys/ with the shards, and genome/ when the pipeline built
# the genome. In a new temporary directory it gunzips keys/ into keys/, links the built genome
# into genome/ (a real directory, as build records the genome relative to index/ after resolving
# its directory), runs metadata.json's rebuild command, and compares:
#   - each shard's md5 and line count, packed and unpacked, with metadata.json's outputs.keys;
#   - the rebuilt caid.fst and manifest.json with metadata.json's rebuild.expected_md5 and with
#     the published <outdir>/index/ files.
# It names every difference and exits 1 if there is one (2 for a usage or setup error).
#
# clingen-index is taken from PATH; CLINGEN_INDEX overrides it with a command (split on spaces),
# e.g. CLINGEN_INDEX='apptainer exec clingen-index.sif clingen-index' on a cluster, where the
# temporary directory (TMPDIR) and <outdir> must be visible inside the container. Use the
# release that metadata.json's tool.clingen_index_version names. Needs jq, gzip and md5sum (or md5).

set -euo pipefail

usage() {
    echo "usage: $0 <outdir>" >&2
    exit 2
}
[[ $# -eq 1 ]] || usage
out=$1
meta="$out/index/metadata.json"
if [[ ! -f $meta ]]; then
    echo "check_rebuild: $meta: no such file (is $out a run's --outdir, published by a pipeline with metadata.json?)" >&2
    exit 2
fi
if ! command -v jq > /dev/null; then
    echo "check_rebuild: needs jq to read $meta" >&2
    exit 2
fi
out=$(cd "$out" && pwd -P)
meta="$out/index/metadata.json"

read -r -a tool <<< "${CLINGEN_INDEX:-clingen-index}"
if ! command -v "${tool[0]}" > /dev/null; then
    echo "check_rebuild: ${tool[0]}: not found (put clingen-index on PATH or set CLINGEN_INDEX)" >&2
    exit 2
fi

md5of() {
    if command -v md5sum > /dev/null; then md5sum "$1" | awk '{ print $1 }'; else md5 -q "$1"; fi
}

format=$(jq -r '.format_version' "$meta")
if [[ $format != 1 ]]; then
    echo "check_rebuild: $meta has format_version $format; this script reads format_version 1" >&2
    exit 2
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/check_rebuild.XXXXXX")
cleanup() {
    if [[ -n ${work:-} && -d $work ]]; then rm -rf "$work"; fi
}
trap cleanup EXIT

failures=()
fail() {
    failures+=("$1")
    echo "DIFFERS: $1" >&2
}

# The shards, as metadata.json lists them.
mkdir "$work/keys"
n=$(jq '.outputs.keys | length' "$meta")
if [[ $n -eq 0 ]]; then
    fail "metadata.json lists no shard in outputs.keys"
fi
for ((i = 0; i < n; i++)); do
    file=$(jq -r ".outputs.keys[$i].file" "$meta")
    gz="$out/keys/$file"
    tsv="$work/keys/${file%.gz}"
    if [[ ! -f $gz ]]; then
        fail "keys/$file: listed in metadata.json, missing from $out/keys/"
        continue
    fi
    want=$(jq -r ".outputs.keys[$i].md5" "$meta")
    got=$(md5of "$gz")
    [[ $got == "$want" ]] || fail "keys/$file: md5 $got, metadata.json says $want"
    gzip -dc "$gz" > "$tsv"
    want=$(jq -r ".outputs.keys[$i].tsv_md5" "$meta")
    got=$(md5of "$tsv")
    [[ $got == "$want" ]] || fail "keys/$file unpacked: md5 $got, metadata.json says $want"
    want=$(jq -r ".outputs.keys[$i].lines" "$meta")
    got=$(wc -l < "$tsv" | tr -d ' ')
    [[ $got == "$want" ]] || fail "keys/$file unpacked: $got lines, metadata.json says $want"
done
extra=$(cd "$out/keys" && find . -maxdepth 1 -type f -name '*.tsv.gz' | sed 's|^\./||' | LC_ALL=C sort |
    while IFS= read -r f; do jq -e --arg f "$f" '.outputs.keys | map(.file) | index($f) != null' "$meta" > /dev/null || echo "$f"; done)
if [[ -n $extra ]]; then
    fail "keys/ holds shard(s) metadata.json does not list: $(echo "$extra" | tr '\n' ' ')"
fi

# The genome: a built one is linked into a real genome/ beside index/; a given one must exist at
# the absolute path the manifest records.
recorded=$(jq -r '.rebuild.genome.recorded // empty' "$meta")
if [[ $(jq -r '.rebuild.genome.relative' "$meta") == true ]]; then
    mkdir "$work/genome"
    for f in GRCh38.bin GRCh38.bin.idx; do
        if [[ ! -f $out/genome/$f ]]; then
            echo "check_rebuild: $out/genome/$f: no such file; the manifest records the genome as $recorded" >&2
            exit 2
        fi
        ln -s "$out/genome/$f" "$work/genome/$f"
    done
elif [[ -n $recorded && ! -e $recorded ]]; then
    echo "check_rebuild: the manifest records the genome $recorded, which does not exist here; build refuses a" >&2
    echo "missing --genome (any file at that path will do: build records the path and does not read it)" >&2
    exit 2
fi

# The rebuild command, word by word from metadata.json, with argv[0] (clingen-index) as given.
args=()
while IFS= read -r a; do args+=("$a"); done < <(jq -r '.rebuild.argv[1:][]' "$meta")
echo "check_rebuild: in $work:" >&2
echo "  ${tool[*]} $(jq -r '.rebuild.command' "$meta" | cut -d' ' -f2-)" >&2
version=$("${tool[@]}" --version)
recorded_version=$(jq -r '.tool.clingen_index_version' "$meta")
if [[ $version != "$recorded_version" ]]; then
    echo "check_rebuild: note: ${tool[*]} is '$version'; the run's was '$recorded_version'" >&2
fi
if ! (cd "$work" && "${tool[@]}" "${args[@]}"); then
    echo "check_rebuild: the rebuild command failed in $work" >&2
    exit 2
fi

for f in caid.fst manifest.json; do
    got=$(md5of "$work/index/$f")
    want=$(jq -r --arg f "$f" '.rebuild.expected_md5[$f]' "$meta")
    published=$(md5of "$out/index/$f")
    [[ $got == "$want" ]] || fail "rebuilt $f: md5 $got, metadata.json's rebuild.expected_md5 says $want"
    [[ $got == "$published" ]] || fail "rebuilt $f: md5 $got, published $out/index/$f has $published"
    echo "$f	rebuilt $got	expected $want	published $published" >&2
done

if [[ ${#failures[@]} -gt 0 ]]; then
    echo "check_rebuild: ${#failures[@]} difference(s) in $out:" >&2
    printf '  %s\n' "${failures[@]}" >&2
    exit 1
fi
echo "check_rebuild: $out/index rebuilt from keys/ byte for byte ($n shard(s))" >&2
