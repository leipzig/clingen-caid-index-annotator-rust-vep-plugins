#!/usr/bin/env python3
"""Cross-check a tool's ClinGen annotations against the Registry harvest.

Usage:
    python3 -I scripts/e2e_crosscheck.py --harvest <chunks_dir> \
        --vcf-out <annotated.vcf> --tool vep-rs|fastvep \
        [--known-differences docs/known-differences.md]

<annotated.vcf> is the VCF a tool wrote for alleles taken from the harvest:
- vep-rs (`--vcf --plugin ClinGen,...`): the CSQ subfield `ClinGen_CAID`,
  located by the CSQ header's `Format:` list. A record's CSQ entries are
  collected per ALT: all of them for a one-ALT record, and by VEP's `Allele`
  notation for a multi-ALT one.
- fastVEP: INFO `FV_CLINGEN`, one `ALLELE|CAID|HGVSG` entry per ALT.

The harvest is every chunk_*.tsv in <chunks_dir> (name order; no header,
columns CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37). It is streamed, and
reading stops once every VCF allele has been met. Only alleles the harvest
gives a CA ID are checked, and the rows in the `known-differences` block of
the known-differences file are exempt, as in the conformance test. A checked
allele agrees when every tool entry for it carries the harvest's CA ID, is
missing when none carries a CA ID, and disagrees otherwise.

Prints `checked=N agree=N disagree=N missing=N` on stdout, and on stderr the
first disagreements and misses and a count of the alleles not checked. Exit
status: 0, or 1 if anything disagrees or is missing, or 2 on unusable input.
Contigs are compared without a `chr` prefix. Standard library only
(Python >= 3.9).
"""
import argparse
import os
import re
import sys

CAID = re.compile(r"CA[0-9]+")
POS = re.compile(r"[0-9]+")
SHOWN = 10
DEFAULT_KNOWN = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                             os.pardir, "docs", "known-differences.md")
FENCE = "```known-differences"


def die(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(2)


def contig(chrom):
    return chrom[3:] if chrom.startswith("chr") else chrom


def position(text, where):
    if not POS.fullmatch(text) or int(text) == 0:
        die(f"{where}: bad POS {text!r}")
    return int(text)


def read_text(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def known_differences(path):
    """The (CHROM, POS, REF, ALT) lines of the one known-differences block."""
    alleles = set()
    blocks = 0
    lines = enumerate(read_text(path).splitlines(), 1)
    for _, line in lines:
        if line.strip() != FENCE:
            continue
        blocks += 1
        closed = False
        for line_no, line in lines:
            line = line.strip()
            if line == "```":
                closed = True
                break
            if not line:
                continue
            where = f"{path}:{line_no}: known-differences"
            fields = line.split()
            if len(fields) != 4:
                die(f"{where}: expected CHROM POS REF ALT, got {line!r}")
            chrom, pos, ref, alt = fields
            allele = (contig(chrom), position(pos, where), ref, alt)
            if allele in alleles:
                die(f"{where}: {line!r} is listed twice")
            alleles.add(allele)
        if not closed:
            die(f"{path}: the known-differences block is not closed")
    if blocks != 1:
        die(f"{path}: expected exactly one {FENCE} block, found {blocks}")
    return alleles


def info_value(info, key):
    prefix = key + "="
    for field in info.split(";"):
        if field.startswith(prefix):
            return field[len(prefix):]
    return None


def vep_alleles(ref, alts):
    """The CSQ `Allele` of each ALT of a multi-ALT record.

    VEP's anchor chop (Parser/VCF.pm, and vep-rs's `trim_multi_alleles`):
    when any ALT's length differs from REF's and REF and every ALT without a
    `*` share the first base, that base is chopped, an emptied allele reading
    `-`. `*` alleles are kept as written.
    """
    indel = any(len(alt) != len(ref) for alt in alts)
    anchored = indel and all(alt[:1].upper() == ref[:1].upper()
                             for alt in alts if "*" not in alt)
    return [alt if "*" in alt else (alt[1:] if anchored else alt) or "-"
            for alt in alts]


class VepRs:
    """CA IDs from the `ClinGen_CAID` CSQ subfield of a vep-rs VCF."""

    def __init__(self):
        self.fields = None

    def header(self, line):
        if not line.startswith("##INFO=<ID=CSQ,"):
            return
        _, found, rest = line.partition("Format: ")
        names = rest.split('"', 1)[0].split("|") if found else []
        if "Allele" in names and "ClinGen_CAID" in names:
            self.fields = (len(names), names.index("Allele"), names.index("ClinGen_CAID"))

    def check_header(self, path):
        if self.fields is None:
            die(f"{path}: no CSQ header with Allele and ClinGen_CAID in its Format; "
                "was vep run with --vcf and --plugin ClinGen?")

    def caids(self, ref, alts, info, where):
        """The CA ID of each of the tool's entries, per ALT ("" for none)."""
        per_alt = [[] for _ in alts]
        value = info_value(info, "CSQ")
        if value is None:
            return per_alt
        width, allele_at, caid_at = self.fields
        index = {a: i for i, a in enumerate(vep_alleles(ref, alts))}
        for entry in value.split(","):
            parts = entry.split("|")
            if len(parts) != width:
                die(f"{where}: CSQ entry {entry!r} has {len(parts)} fields, "
                    f"the header {width}")
            if len(alts) == 1:
                alt = 0
            elif parts[allele_at] in index:
                alt = index[parts[allele_at]]
            else:
                die(f"{where}: CSQ Allele {parts[allele_at]!r} matches no ALT")
            per_alt[alt].append(parts[caid_at])
        return per_alt


class FastVep:
    """CA IDs from the `FV_CLINGEN` INFO field of a fastVEP VCF."""

    def __init__(self):
        self.seen = False

    def header(self, line):
        if line.startswith("##INFO=<ID=FV_CLINGEN,") and "Format: ALLELE|CAID|HGVSG" in line:
            self.seen = True

    def check_header(self, path):
        if not self.seen:
            die(f"{path}: no FV_CLINGEN header with Format ALLELE|CAID|HGVSG; "
                "was the ClinGen index under --sa-dir?")

    def caids(self, ref, alts, info, where):
        per_alt = [[] for _ in alts]
        value = info_value(info, "FV_CLINGEN")
        if value is None:
            return per_alt
        index = {a: i for i, a in enumerate(alts)}
        for entry in value.split(","):
            parts = entry.split("|")
            if len(parts) != 3:
                die(f"{where}: FV_CLINGEN entry {entry!r} is not ALLELE|CAID|HGVSG")
            if parts[0] not in index:
                die(f"{where}: FV_CLINGEN allele {parts[0]!r} matches no ALT")
            per_alt[index[parts[0]]].append(parts[1])
        return per_alt


TOOLS = {"vep-rs": VepRs, "fastvep": FastVep}


def tool_calls(path, tool):
    """{(CHROM, POS, REF, ALT): the CA IDs of the tool's entries for it}."""
    calls = {}
    in_header = True
    with open(path, encoding="utf-8") as f:
        for line_no, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if line.startswith("##"):
                tool.header(line)
                continue
            if line.startswith("#") or not line:
                continue
            if in_header:
                tool.check_header(path)
                in_header = False
            where = f"{path}:{line_no}"
            cols = line.split("\t")
            if len(cols) < 8:
                die(f"{where}: expected at least 8 columns, got {len(cols)}")
            chrom, pos, ref, alts = contig(cols[0]), position(cols[1], where), cols[3], cols[4]
            alts = alts.split(",")
            for alt, caids in zip(alts, tool.caids(ref, alts, cols[7], where)):
                allele = (chrom, pos, ref, alt)
                if allele in calls:
                    die(f"{where}: allele {' '.join(map(str, allele))} appears twice")
                calls[allele] = caids
    if in_header:
        tool.check_header(path)
    return calls


def harvest_rows(chunks_dir):
    """Yield (where, columns) for every harvest row, in chunk-name order."""
    names = sorted(n for n in os.listdir(chunks_dir)
                   if n.startswith("chunk_") and n.endswith(".tsv"))
    if not names:
        die(f"no chunk_*.tsv files in {chunks_dir}")
    for name in names:
        path = os.path.join(chunks_dir, name)
        with open(path, encoding="utf-8", newline="\n") as f:
            for line_no, line in enumerate(f, 1):
                cols = line.rstrip("\n").split("\t")
                if len(cols) != 7:
                    die(f"{path}:{line_no}: expected 7 columns, got {len(cols)}")
                yield f"{path}:{line_no}", cols


def main(argv):
    parser = argparse.ArgumentParser(
        description="Cross-check a tool's ClinGen CA IDs against the Registry harvest.")
    parser.add_argument("--harvest", required=True, help="directory of chunk_*.tsv")
    parser.add_argument("--vcf-out", required=True, help="the VCF the tool wrote")
    parser.add_argument("--tool", required=True, choices=sorted(TOOLS))
    parser.add_argument("--known-differences", default=DEFAULT_KNOWN,
                        help="markdown file with the known-differences block "
                             "(default: docs/known-differences.md)")
    args = parser.parse_args(argv[1:])

    known = known_differences(args.known_differences)
    pending = tool_calls(args.vcf_out, TOOLS[args.tool]())
    vcf_alleles = len(pending)

    counts = {"checked": 0, "agree": 0, "disagree": 0, "missing": 0}
    excluded = no_caid = 0
    problems = []
    for where, cols in harvest_rows(args.harvest):
        if not pending:
            break
        allele = (contig(cols[0]), position(cols[1], where), cols[2], cols[3])
        caids = pending.pop(allele, None)
        if caids is None:
            continue
        if allele in known:
            excluded += 1
            continue
        expected = cols[4]
        if not CAID.fullmatch(expected):
            no_caid += 1
            continue
        counts["checked"] += 1
        if not any(caids):
            outcome = "missing"
        elif set(caids) == {expected}:
            outcome = "agree"
        else:
            outcome = "disagree"
        counts[outcome] += 1
        if outcome != "agree":
            got = ",".join(c or "-" for c in caids) or "no entry"
            problems.append(f"{outcome} {' '.join(map(str, allele))}: "
                            f"harvest {expected}, {args.tool} {got}")

    for line in problems[:SHOWN]:
        print(line, file=sys.stderr)
    if len(problems) > SHOWN:
        print(f"... and {len(problems) - SHOWN} more", file=sys.stderr)
    print(f"vcf_alleles={vcf_alleles} not_in_harvest={len(pending)} "
          f"no_caid={no_caid} excluded={excluded}", file=sys.stderr)
    print(" ".join(f"{k}={v}" for k, v in counts.items()))
    return 1 if counts["disagree"] or counts["missing"] else 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except (OSError, UnicodeDecodeError) as e:
        die(str(e))
