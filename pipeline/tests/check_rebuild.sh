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
# A --genome given is recorded in manifest.json by its absolute path on the launcher, and build
# refuses a --genome that does not exist. Where that path cannot be made, GENOME_STANDIN=<file>
# rebuilds with that file in its place (build records the path and does not read the file):
# caid.fst must still match byte for byte, and manifest.json once the rebuilt genome value is put
# back to the recorded one. The last line says which of the two checks were made.
#
# clingen-index is taken from PATH; CLINGEN_INDEX overrides it with a command (split on spaces),
# e.g. CLINGEN_INDEX='apptainer exec clingen-index.sif clingen-index' on a cluster, where the
# temporary directory (TMPDIR) and <outdir> must be visible inside the container. The binary
# that built the index is named by metadata.json's tool.clingen_index_sha256, not by its version
# string (the crate's, not raised with each change to crates/); tool.note says how to build it.
# A clingen-index from PATH is compared with that sha256 and the result printed; another build
# may still give the same bytes, and the md5s decide. Needs jq, gzip and md5sum (or md5); the
# sha256 comparison also needs openssl, and is left out without it.

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
standin=
if [[ $(jq -r '.rebuild.genome.relative' "$meta") == true ]]; then
    if [[ -n ${GENOME_STANDIN:-} ]]; then
        echo "check_rebuild: note: GENOME_STANDIN unused: the genome is the built one in $out/genome/" >&2
    fi
    mkdir "$work/genome"
    for f in GRCh38.bin GRCh38.bin.idx; do
        if [[ ! -f $out/genome/$f ]]; then
            echo "check_rebuild: $out/genome/$f: no such file; the manifest records the genome as $recorded" >&2
            exit 2
        fi
        ln -s "$out/genome/$f" "$work/genome/$f"
    done
elif [[ -n $recorded && -n ${GENOME_STANDIN:-} ]]; then
    if [[ ! -f $GENOME_STANDIN ]]; then
        echo "check_rebuild: GENOME_STANDIN=$GENOME_STANDIN: no such file" >&2
        exit 2
    fi
    standin=$GENOME_STANDIN
elif [[ -n $recorded && ! -e $recorded ]]; then
    echo "check_rebuild: the manifest records the genome $recorded, which does not exist here; build refuses a" >&2
    echo "missing --genome (any file at that path will do: build records the path and does not read it)." >&2
    echo "Set GENOME_STANDIN=<any file> to rebuild with that file instead and check manifest.json apart from" >&2
    echo "its genome value." >&2
    exit 2
fi

# The rebuild command, word by word from metadata.json, with argv[0] (clingen-index) as given.
# With GENOME_STANDIN, the word after --genome (the recorded path) is replaced by the stand-in.
args=()
prev=
while IFS= read -r a; do
    if [[ $prev == --genome && -n $standin ]]; then a=$standin; fi
    args+=("$a")
    prev=$a
done < <(jq -r '.rebuild.argv[1:][]' "$meta")
echo "check_rebuild: in $work:" >&2
echo "  ${tool[*]} ${args[*]}" >&2
version=$("${tool[@]}" --version)
recorded_version=$(jq -r '.tool.clingen_index_version' "$meta")
if [[ $version != "$recorded_version" ]]; then
    echo "check_rebuild: note: ${tool[*]} is '$version'; the run's was '$recorded_version'" >&2
fi
recorded_sha=$(jq -r '.tool.clingen_index_sha256 // empty' "$meta")
if [[ ${#tool[@]} -eq 1 && -n $recorded_sha ]] && command -v openssl > /dev/null; then
    sha=$(openssl dgst -sha256 -r "$(command -v "${tool[0]}")" | awk '{ print $1 }')
    if [[ $sha == "$recorded_sha" ]]; then
        echo "check_rebuild: ${tool[0]} is the binary that built the index (sha256 $sha)" >&2
    else
        echo "check_rebuild: note: ${tool[0]} has sha256 $sha; the run's ($(jq -r '.tool.platform' "$meta")) had $recorded_sha" >&2
    fi
fi
if ! (cd "$work" && "${tool[@]}" "${args[@]}"); then
    echo "check_rebuild: the rebuild command failed in $work" >&2
    exit 2
fi

# With a stand-in genome, the rebuilt manifest's genome value (the stand-in as build recorded
# it, its directory resolved) is put back to the recorded one, byte for byte as JSON strings, so
# every other byte of manifest.json is still compared.
manifest_check="manifest.json byte for byte"
if [[ -n $standin ]]; then
    rebuilt_genome=$(jq -r '.genome' "$work/index/manifest.json")
    mv "$work/index/manifest.json" "$work/manifest.standin.json"
    # Through the environment, as awk -v would read backslash escapes in the values.
    FROM="\"genome\": $(jq -n --arg g "$rebuilt_genome" '$g')" TO="\"genome\": $(jq -n --arg g "$recorded" '$g')" awk '
        { i = index($0, ENVIRON["FROM"]); if (i) { $0 = substr($0, 1, i - 1) ENVIRON["TO"] substr($0, i + length(ENVIRON["FROM"])) } print }
    ' "$work/manifest.standin.json" > "$work/index/manifest.json"
    manifest_check="manifest.json byte for byte apart from its genome value (rebuilt with GENOME_STANDIN=$standin, which build recorded as $rebuilt_genome)"
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
if [[ -n $standin ]]; then
    echo "check_rebuild: $out/index rebuilt from keys/ with a stand-in genome ($n shard(s))" >&2
else
    echo "check_rebuild: $out/index rebuilt from keys/ byte for byte ($n shard(s))" >&2
fi
echo "check_rebuild: checked caid.fst byte for byte; $manifest_check" >&2
