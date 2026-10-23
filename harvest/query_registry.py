#!/usr/bin/env python3
"""Look up CA IDs and canonical HGVS for a sites TSV via the ClinGen Allele Registry.

Read-only: uses POST /alleles (lookup). Never PUT, which would register alleles.

Input:  TSV of CHROM POS REF ALT (bare contig names, GRCh38).
Output: one TSV per chunk in <out_dir>, columns:
        CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37
        CAID is empty when the Registry has no allele registered.
Chunks already written are skipped, so the script can be re-run to resume.
"""
import argparse
import json
import os
import sys
import time
import urllib.request

URL = ("https://reg.genome.network/alleles?file=vcf"
       "&fields=none+@id+genomicAlleles.referenceGenome+genomicAlleles.hgvs")
CA_PREFIX = "http://reg.genome.network/allele/"

# GRCh38 contig lengths; the Registry rejects data lines without a ##contig header.
CONTIG_LENGTHS = {
    "1": 248956422, "2": 242193529, "3": 198295559, "4": 190214555,
    "5": 181538259, "6": 170805979, "7": 159345973, "8": 145138636,
    "9": 138394717, "10": 133797422, "11": 135086622, "12": 133275309,
    "13": 114364328, "14": 107043718, "15": 101991189, "16": 90338345,
    "17": 83257441, "18": 80373285, "19": 58617616, "20": 64444167,
    "21": 46709983, "22": 50818468, "X": 156040895, "Y": 57227415,
    "MT": 16569,
}


def build_vcf(rows):
    chroms = sorted({r[0] for r in rows})
    header = ["##fileformat=VCFv4.2"]
    header += [f"##contig=<ID={c},length={CONTIG_LENGTHS[c]},assembly=GRCh38>"
               for c in chroms]
    header.append("#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO")
    body = [f"{c}\t{p}\t.\t{ref}\t{alt}\t.\t.\t." for c, p, ref, alt in rows]
    return ("\n".join(header + body) + "\n").encode()


def post(payload, retries=6):
    for attempt in range(retries):
        try:
            req = urllib.request.Request(URL, data=payload, method="POST")
            with urllib.request.urlopen(req, timeout=1800) as resp:
                return json.load(resp)
        except Exception as e:  # network errors, 5xx, truncated JSON
            wait = 30 * 2 ** attempt
            print(f"  request failed ({e}); retry in {wait}s", file=sys.stderr)
            time.sleep(wait)
    raise RuntimeError("giving up after repeated failures")


def hgvs_for(allele, genome):
    for g in allele.get("genomicAlleles", []):
        if g.get("referenceGenome") == genome:
            return next((h for h in g.get("hgvs", []) if h.startswith("NC_")), "")
    return ""


def process_chunk(rows, out_path):
    results = post(build_vcf(rows))
    if not isinstance(results, list) or len(results) != len(rows):
        raise RuntimeError(f"expected {len(rows)} results, got "
                           f"{len(results) if isinstance(results, list) else results}")
    stats = {"registered": 0, "unregistered": 0, "error": 0}
    with open(out_path + ".partial", "w") as out:
        for (c, p, ref, alt), a in zip(rows, results):
            if "errorType" in a:
                stats["error"] += 1
                out.write(f"{c}\t{p}\t{ref}\t{alt}\tERROR:{a['errorType']}\t\t\n")
                continue
            at = a.get("@id", "")
            caid = at[len(CA_PREFIX):] if at.startswith(CA_PREFIX) else ""
            stats["registered" if caid else "unregistered"] += 1
            out.write(f"{c}\t{p}\t{ref}\t{alt}\t{caid}\t"
                      f"{hgvs_for(a, 'GRCh38')}\t{hgvs_for(a, 'GRCh37')}\n")
    os.rename(out_path + ".partial", out_path)
    return stats


def chunks(path, size):
    rows = []
    with open(path) as f:
        for line in f:
            rows.append(tuple(line.rstrip("\n").split("\t")))
            if len(rows) == size:
                yield rows
                rows = []
    if rows:
        yield rows


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sites")
    ap.add_argument("out_dir")
    ap.add_argument("--chunk-size", type=int, default=250_000)
    args = ap.parse_args()
    os.makedirs(args.out_dir, exist_ok=True)

    for i, rows in enumerate(chunks(args.sites, args.chunk_size)):
        out_path = os.path.join(args.out_dir, f"chunk_{i:05d}.tsv")
        if os.path.exists(out_path):
            continue
        t0 = time.time()
        stats = process_chunk(rows, out_path)
        print(f"chunk {i}: {len(rows)} sites in {time.time() - t0:.0f}s {stats}",
              flush=True)


if __name__ == "__main__":
    main()
