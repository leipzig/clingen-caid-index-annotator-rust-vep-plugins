#!/usr/bin/env python3
"""Draw the conformance sample from a harvest.

Usage: python3 -I scripts/sample_harvest.py <chunks_dir> <out.tsv>

Reads every chunk_*.tsv in <chunks_dir> (filename order; no header, columns
CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37) and writes SAMPLE_ROWS
registered rows to <out.tsv>, unchanged, header-less and in harvest order.

The sample is stratified:
- "unaligned": indels that a VCF left-aligner would move, because REF and
  ALT end in the same base. They are rare and are what the conformance
  test's left-aligned form exercises, so all of them are taken (up to
  MAX_UNALIGNED).
- the other rows, by the edit class of the Registry's HGVS, using the same
  rule as `clingen-index validate`. Every class present gets at least one
  row; the non-substitution classes share what is left of
  NON_SUBSTITUTION_ROWS in proportion to their size, and substitutions fill
  the rest.

The draw is deterministic (random.Random(SEED)) for a given harvest.
Standard library only (Python >= 3.9).
"""
import os
import random
import re
import sys

SAMPLE_ROWS = 10_000
NON_SUBSTITUTION_ROWS = 3_000
MAX_UNALIGNED = 500
MIN_INDELS = 2_000
SEED = 21

# Report order of `clingen-index validate`.
CLASSES = ["substitution", "del", "dup", "ins", "delins", "m", "other"]
UNALIGNED = "unaligned"
STRATA = [UNALIGNED] + CLASSES
CAID = re.compile(r"CA[0-9]+")


def edit_class(hgvs):
    """The class `clingen-index validate` gives a Registry HGVS string."""
    if ":m." in hgvs:
        return "m"
    if ">" in hgvs:
        return "substitution"
    if "delins" in hgvs:
        return "delins"
    if hgvs.endswith("dup"):
        return "dup"
    if hgvs.endswith("del"):
        return "del"
    if "ins" in hgvs:
        return "ins"
    return "other"


def stratum(ref, alt, cls):
    """UNALIGNED for an indel whose REF and ALT end in the same base, else cls."""
    if ref and alt and len(ref) != len(alt) and ref[-1] == alt[-1]:
        return UNALIGNED
    return cls


def chunk_files(chunks_dir):
    names = sorted(n for n in os.listdir(chunks_dir)
                   if n.startswith("chunk_") and n.endswith(".tsv"))
    if not names:
        sys.exit(f"error: no chunk_*.tsv files in {chunks_dir}")
    return [os.path.join(chunks_dir, n) for n in names]


def registered_rows(files):
    """Yield (line, class, stratum) per registered row; exit on a malformed one."""
    for path in files:
        with open(path, encoding="utf-8", newline="\n") as f:
            for line_no, line in enumerate(f, 1):
                cols = line.rstrip("\n").split("\t")
                if len(cols) != 7:
                    sys.exit(f"error: {path}:{line_no}: expected 7 columns, got {len(cols)}")
                caid, hgvs38 = cols[4], cols[5]
                if CAID.fullmatch(caid) and hgvs38:
                    cls = edit_class(hgvs38)
                    yield line, cls, stratum(cols[2], cols[3], cls)


def quotas(counts):
    """Rows to draw per stratum."""
    quota = {s: 0 for s in STRATA}
    quota[UNALIGNED] = min(counts[UNALIGNED], MAX_UNALIGNED)
    others = [c for c in CLASSES if c != "substitution" and counts[c] > 0]
    total = sum(counts[c] for c in others)
    budget = min(NON_SUBSTITUTION_ROWS - quota[UNALIGNED], total)
    # Floor of each class's proportional share of the budget (at least 1),
    # then the rows lost to rounding, largest remainder first.
    for c in others:
        quota[c] = max(1, budget * counts[c] // total)
    for c in sorted(others, key=lambda c: -(budget * counts[c] % total)):
        if sum(quota[o] for o in others) >= budget:
            break
        if quota[c] < counts[c]:
            quota[c] += 1
    quota["substitution"] = SAMPLE_ROWS - sum(quota.values())
    if not 0 < quota["substitution"] <= counts["substitution"]:
        sys.exit(f"error: need {quota['substitution']} substitutions, "
                 f"harvest has {counts['substitution']}")
    return quota


def main(argv):
    if len(argv) != 3:
        sys.exit("usage: python3 -I scripts/sample_harvest.py <chunks_dir> <out.tsv>")
    chunks_dir, out_path = argv[1], argv[2]
    files = chunk_files(chunks_dir)

    counts = {s: 0 for s in STRATA}
    class_counts = {c: 0 for c in CLASSES}
    for _, cls, st in registered_rows(files):
        counts[st] += 1
        class_counts[cls] += 1
    quota = quotas(counts)

    rng = random.Random(SEED)
    chosen = {s: set(rng.sample(range(counts[s]), quota[s])) for s in STRATA}

    seen = {s: 0 for s in STRATA}
    drawn = {s: 0 for s in STRATA}
    class_drawn = {c: 0 for c in CLASSES}
    indels = 0
    partial = out_path + ".partial"
    with open(partial, "w", encoding="utf-8", newline="\n") as out:
        for line, cls, st in registered_rows(files):
            if seen[st] in chosen[st]:
                out.write(line if line.endswith("\n") else line + "\n")
                drawn[st] += 1
                class_drawn[cls] += 1
                cols = line.split("\t")
                indels += len(cols[2]) != len(cols[3])
            seen[st] += 1

    problem = None
    if seen != counts or sum(drawn.values()) != SAMPLE_ROWS:
        problem = "the harvest changed while sampling"
    elif indels < MIN_INDELS:
        problem = f"only {indels} indels in the sample, need {MIN_INDELS}"
    elif any(class_counts[c] and not class_drawn[c] for c in CLASSES):
        problem = "a class present in the harvest is missing from the sample"
    if problem:
        os.remove(partial)
        sys.exit(f"error: {problem}")
    os.replace(partial, out_path)

    print("stratum\tharvest\tsample", file=sys.stderr)
    for s in STRATA:
        print(f"{s}\t{counts[s]}\t{drawn[s]}", file=sys.stderr)
    print("class\tharvest\tsample", file=sys.stderr)
    for c in CLASSES:
        print(f"{c}\t{class_counts[c]}\t{class_drawn[c]}", file=sys.stderr)
    print(f"wrote {SAMPLE_ROWS} rows ({indels} indels) to {out_path}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv)
