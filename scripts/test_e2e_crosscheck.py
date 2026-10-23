#!/usr/bin/env python3
"""Tests for scripts/e2e_crosscheck.py, run as a subprocess on small fixtures.

Usage: python3 -I scripts/test_e2e_crosscheck.py

Standard library only (Python >= 3.9).
"""
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "e2e_crosscheck.py")

# Harvest rows: CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37, no header.
HARVEST = [
    "21\t5030578\tC\tT\tCA749862939\tNC_000021.9:g.5030578C>T\t",
    "21\t13189782\tC\tCA\tCA636937543\tNC_000021.9:g.13189788dup\t",
    "21\t33000166\tTA\tT\t\t\t",
    "21\t33000200\tG\tA\tCA1\tNC_000021.9:g.33000200G>A\t",
    "21\t33000200\tG\tGT\tCA2\tNC_000021.9:g.33000200_33000201insT\t",
]

EMPTY_KNOWN = "# Known differences\n\n```known-differences\n```\n"

VEP_HEADER = (
    "##fileformat=VCFv4.2\n"
    '##INFO=<ID=CSQ,Number=.,Type=String,Description="Consequence annotations '
    'from Ensembl VEP. Format: Allele|Consequence|ClinGen_HGVSg|ClinGen_CAID|IMPACT">\n'
    "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n"
)

FASTVEP_HEADER = (
    "##fileformat=VCFv4.2\n"
    '##INFO=<ID=FV_CLINGEN,Number=.,Type=String,Description="fastVEP ClinGen '
    'Allele Registry annotations. Format: ALLELE|CAID|HGVSG">\n'
    "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n"
)


def record(chrom, pos, ref, alt, info):
    return f"{chrom}\t{pos}\t.\t{ref}\t{alt}\t.\tPASS\t{info}\n"


def csq(allele, caid, hgvs="h"):
    """One CSQ entry in VEP_HEADER's Format order."""
    return f"{allele}|intergenic_variant|{hgvs}|{caid}|MODIFIER"


class CrossCheck(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.harvest = os.path.join(self.dir, "chunks")
        os.mkdir(self.harvest)
        # Two chunks, so the harvest is read across files in name order.
        self.write("chunks/chunk_00000.tsv", "\n".join(HARVEST[:2]) + "\n")
        self.write("chunks/chunk_00001.tsv", "\n".join(HARVEST[2:]) + "\n")
        self.known = self.write("known.md", EMPTY_KNOWN)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, name, text):
        path = os.path.join(self.dir, name)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        return path

    def run_check(self, tool, vcf_text, known=None):
        vcf = self.write("out.vcf", vcf_text)
        return subprocess.run(
            [sys.executable, "-I", SCRIPT, "--harvest", self.harvest,
             "--vcf-out", vcf, "--tool", tool,
             "--known-differences", known or self.known],
            capture_output=True, text=True)

    def assert_result(self, proc, line, code):
        self.assertEqual(proc.stdout, line + "\n", proc.stderr)
        self.assertEqual(proc.returncode, code, proc.stderr)

    # vep-rs

    def test_vep_rs_all_agree(self):
        vcf = VEP_HEADER + "".join([
            # Several CSQ entries per record, all with the same CA ID.
            record("21", 5030578, "C", "T",
                   "CSQ=" + ",".join([csq("T", "CA749862939")] * 3)),
            # VEP's trimmed allele notation for an insertion.
            record("21", 13189782, "C", "CA", "CSQ=" + csq("A", "CA636937543")),
        ])
        self.assert_result(self.run_check("vep-rs", vcf),
                           "checked=2 agree=2 disagree=0 missing=0", 0)

    def test_vep_rs_counts_disagree_and_missing(self):
        vcf = VEP_HEADER + "".join([
            record("21", 5030578, "C", "T", "CSQ=" + csq("T", "CA9")),
            record("21", 13189782, "C", "CA", "CSQ=" + csq("A", "")),
        ])
        proc = self.run_check("vep-rs", vcf)
        self.assert_result(proc, "checked=2 agree=0 disagree=1 missing=1", 1)
        self.assertIn("21 5030578 C T", proc.stderr)
        self.assertIn("CA9", proc.stderr)
        self.assertIn("21 13189782 C CA", proc.stderr)

    def test_vep_rs_record_without_csq_is_missing(self):
        vcf = VEP_HEADER + record("21", 5030578, "C", "T", ".")
        self.assert_result(self.run_check("vep-rs", vcf),
                           "checked=1 agree=0 disagree=0 missing=1", 1)

    def test_vep_rs_conflicting_entries_disagree(self):
        vcf = VEP_HEADER + record(
            "21", 5030578, "C", "T",
            "CSQ=" + csq("T", "CA749862939") + "," + csq("T", ""))
        self.assert_result(self.run_check("vep-rs", vcf),
                           "checked=1 agree=0 disagree=1 missing=0", 1)

    def test_vep_rs_multi_allelic_maps_alts_by_vep_allele(self):
        # An indel record, but A does not share REF's first base, so VEP
        # chops nothing and the CSQ alleles are the ALTs as written.
        vcf = VEP_HEADER + record(
            "21", 33000200, "G", "A,GT",
            "CSQ=" + csq("GT", "CA2") + "," + csq("A", "CA1"))
        self.assert_result(self.run_check("vep-rs", vcf),
                           "checked=2 agree=2 disagree=0 missing=0", 0)

    def test_vep_rs_multi_allelic_chopped_alleles(self):
        # Every allele shares A, so VEP chops it: AC/A,ACC reads C/-,CC.
        self.write("chunks/chunk_00002.tsv",
                   "21\t40000000\tAC\tA\tCA3\tx\t\n"
                   "21\t40000000\tAC\tACC\tCA4\tx\t\n")
        vcf = VEP_HEADER + record(
            "21", 40000000, "AC", "A,ACC",
            "CSQ=" + csq("-", "CA3") + "," + csq("CC", "CA4"))
        self.assert_result(self.run_check("vep-rs", vcf),
                           "checked=2 agree=2 disagree=0 missing=0", 0)

    def test_vep_rs_needs_the_clingen_csq_field(self):
        header = VEP_HEADER.replace("|ClinGen_CAID", "")
        proc = self.run_check("vep-rs", header + record("21", 5030578, "C", "T", "."))
        self.assertEqual(proc.returncode, 2)
        self.assertEqual(proc.stdout, "")
        self.assertIn("ClinGen_CAID", proc.stderr)

    # fastVEP

    def test_fastvep_all_agree(self):
        vcf = FASTVEP_HEADER + "".join([
            record("21", 5030578, "C", "T",
                   "DP=3;FV_CLINGEN=T|CA749862939|NC_000021.9:g.5030578C>T"),
            record("chr21", 13189782, "C", "CA",
                   "FV_CLINGEN=CA|CA636937543|NC_000021.9:g.13189788dup"),
            record("21", 33000200, "G", "A,GT",
                   "FV_CLINGEN=A|CA1|x,GT|CA2|y"),
        ])
        self.assert_result(self.run_check("fastvep", vcf),
                           "checked=4 agree=4 disagree=0 missing=0", 0)

    def test_fastvep_counts_disagree_and_missing(self):
        vcf = FASTVEP_HEADER + "".join([
            record("21", 5030578, "C", "T", "FV_CLINGEN=T|CA8|h"),
            record("21", 13189782, "C", "CA", "FV_CLINGEN=CA||h"),
            # One ALT of two has no entry.
            record("21", 33000200, "G", "A,GT", "FV_CLINGEN=A|CA1|x"),
        ])
        proc = self.run_check("fastvep", vcf)
        self.assert_result(proc, "checked=4 agree=1 disagree=1 missing=2", 1)
        self.assertIn("21 33000200 G GT", proc.stderr)

    def test_fastvep_needs_the_fv_clingen_header(self):
        header = FASTVEP_HEADER.replace("FV_CLINGEN", "FV_OTHER")
        proc = self.run_check("fastvep", header + record("21", 5030578, "C", "T", "."))
        self.assertEqual(proc.returncode, 2)
        self.assertIn("FV_CLINGEN", proc.stderr)

    # What is checked

    def test_only_registered_harvest_rows_are_checked(self):
        vcf = FASTVEP_HEADER + "".join([
            # The harvest has no CA ID for it, so it is not checked.
            record("21", 33000166, "TA", "T", "FV_CLINGEN=T||h"),
            # Not in the harvest at all.
            record("21", 9999999, "A", "G", "."),
            record("21", 5030578, "C", "T", "FV_CLINGEN=T|CA749862939|h"),
        ])
        proc = self.run_check("fastvep", vcf)
        self.assert_result(proc, "checked=1 agree=1 disagree=0 missing=0", 0)
        self.assertIn("not_in_harvest=1", proc.stderr)
        self.assertIn("no_caid=1", proc.stderr)

    def test_known_differences_are_exempt(self):
        known = self.write("known2.md", "```known-differences\n21 5030578 C T\n\n```\n")
        vcf = FASTVEP_HEADER + "".join([
            record("21", 5030578, "C", "T", "FV_CLINGEN=T|CA8|h"),
            record("21", 13189782, "C", "CA", "FV_CLINGEN=CA|CA636937543|h"),
        ])
        proc = self.run_check("fastvep", vcf, known)
        self.assert_result(proc, "checked=1 agree=1 disagree=0 missing=0", 0)
        self.assertIn("excluded=1", proc.stderr)

    def test_malformed_known_differences_fail(self):
        vcf = FASTVEP_HEADER + record("21", 5030578, "C", "T", "FV_CLINGEN=T|CA749862939|h")
        for text in ["no block here\n",
                     "```known-differences\n21 1 A\n```\n",
                     "```known-differences\n21 0 A G\n```\n",
                     "```known-differences\n21 1 A G\n21 1 A G\n```\n",
                     "```known-differences\n```\n```known-differences\n```\n",
                     "```known-differences\n21 1 A G\n"]:
            with self.subTest(text=text):
                proc = self.run_check("fastvep", vcf, self.write("bad.md", text))
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertEqual(proc.stdout, "")
                self.assertIn("known-differences", proc.stderr)

    def test_default_known_differences_is_the_repo_file(self):
        vcf = self.write("out.vcf", FASTVEP_HEADER + record(
            "21", 5030578, "C", "T", "FV_CLINGEN=T|CA749862939|h"))
        proc = subprocess.run(
            [sys.executable, "-I", SCRIPT, "--harvest", self.harvest,
             "--vcf-out", vcf, "--tool", "fastvep"],
            capture_output=True, text=True)
        self.assert_result(proc, "checked=1 agree=1 disagree=0 missing=0", 0)

    def test_repeated_vcf_allele_fails(self):
        line = record("21", 5030578, "C", "T", "FV_CLINGEN=T|CA749862939|h")
        proc = self.run_check("fastvep", FASTVEP_HEADER + line + line)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("21 5030578 C T", proc.stderr)


if __name__ == "__main__":
    unittest.main()
