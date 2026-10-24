//! `clingen-index validate`: compare the canonical HGVS `clingen-allele`
//! computes from the genome with the Registry's own string for each row.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clingen_allele::{HgvsResult, canonical_hgvs};
use vareffect::FastaReader;

use crate::ValidateArgs;
use crate::chunks::{Row, Verdict, chunk_files, read_rows};
use crate::hgvs_class::Class;

/// Compared and matching row counts, indexed like [`Class::ALL`].
#[derive(Default)]
struct Tally {
    total: [u64; Class::ALL.len()],
    matched: [u64; Class::ALL.len()],
}

impl Tally {
    fn record(&mut self, class: Class, matched: bool) {
        self.total[class as usize] += 1;
        self.matched[class as usize] += u64::from(matched);
    }

    fn substitution_mismatches(&self) -> u64 {
        let i = Class::Substitution as usize;
        self.total[i] - self.matched[i]
    }

    /// Whether any class, not only substitutions, has a compared row we did
    /// not reproduce.
    fn any_mismatches(&self) -> bool {
        self.total.iter().zip(&self.matched).any(|(t, m)| t != m)
    }

    fn write_table(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "class\ttotal\tmatch\tpct")?;
        for class in Class::ALL {
            let (total, matched) = (self.total[class as usize], self.matched[class as usize]);
            let pct = if total == 0 {
                0.0
            } else {
                matched as f64 * 100.0 / total as f64
            };
            writeln!(out, "{}\t{total}\t{matched}\t{pct:.1}", class.name())?;
        }
        Ok(())
    }
}

/// Our canonical HGVS for the row, or a label saying why we have none.
/// The bool is whether it equals the Registry's string.
fn ours(row: &Row, pos1: u64, genome: &FastaReader) -> (String, bool) {
    match canonical_hgvs(
        row.chrom,
        pos1,
        row.ref_allele.as_bytes(),
        row.alt_allele.as_bytes(),
        genome,
    ) {
        Ok(HgvsResult::Hgvs(hgvs)) => {
            let matched = hgvs == row.hgvs38;
            (hgvs, matched)
        }
        Ok(HgvsResult::RefMismatch) => ("RefMismatch".to_owned(), false),
        Ok(HgvsResult::Unsupported) => ("Unsupported".to_owned(), false),
        // Keep the message on one TSV field.
        Err(e) => (format!("Error:{e}").replace(['\t', '\r', '\n'], " "), false),
    }
}

/// Run `validate`: compare every eligible row, write the mismatches to
/// `args.mismatches`, print the per-class table on stdout, and exit 1 when a
/// substitution mismatches (a mismatch in any class with `--strict`).
pub(crate) fn validate(args: ValidateArgs) -> Result<ExitCode> {
    let files = chunk_files(&args.chunks.chunks)?;
    let genome = FastaReader::open(&args.genome)
        .with_context(|| format!("opening genome {}", args.genome.display()))?;
    let mismatch_file = File::create(&args.mismatches)
        .with_context(|| format!("creating {}", args.mismatches.display()))?;
    let mut mismatches = BufWriter::new(mismatch_file);
    writeln!(mismatches, "CHROM\tPOS\tREF\tALT\tCLASS\tREGISTRY\tOURS")?;

    let mut tally = Tally::default();
    let (mut compared, mut skipped) = (0u64, 0u64);
    let read = read_rows(&files, |row| {
        // Nothing to compare against on a Registry error or without an HGVS.
        if row.caid.starts_with("ERROR:") || row.hgvs38.is_empty() {
            skipped += 1;
            return Ok(Verdict::Continue);
        }
        let Ok(pos1) = row.pos.parse::<u64>() else {
            return Ok(Verdict::Malformed("POS is not a number"));
        };

        let class = Class::of(row.hgvs38);
        let (ours, matched) = ours(row, pos1, &genome);
        tally.record(class, matched);
        if !matched {
            writeln!(
                mismatches,
                "{}\t{}\t{}\t{}\t{}\t{}\t{ours}",
                row.chrom,
                row.pos,
                row.ref_allele,
                row.alt_allele,
                class.name(),
                row.hgvs38,
            )
            .with_context(|| format!("writing {}", args.mismatches.display()))?;
        }

        compared += 1;
        Ok(if args.sample == Some(compared) {
            Verdict::Stop
        } else {
            Verdict::Continue
        })
    })?;
    mismatches
        .flush()
        .with_context(|| format!("writing {}", args.mismatches.display()))?;
    read.malformed.report();

    eprintln!(
        "rows={} compared={compared} skipped={skipped} malformed={}",
        read.rows, read.malformed.count
    );
    tally
        .write_table(&mut io::stdout().lock())
        .context("writing the class table to stdout")?;

    let failed = if args.strict {
        tally.any_mismatches()
    } else {
        tally.substitution_mismatches() > 0
    };
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
