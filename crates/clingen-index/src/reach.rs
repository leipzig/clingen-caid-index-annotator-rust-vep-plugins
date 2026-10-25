//! `clingen-index reach`: prove that every key of an index is found again
//! from a VCF record.
//!
//! A key is only worth holding if a lookup can land on it. For each key,
//! [`hgvs_to_vcf`] turns its HGVS back into a VCF record with the genome,
//! `Annotator::lookup` canonicalises that record and looks it up exactly as
//! the annotation adapters do, and the key's own CA ID must come back. A key
//! that fails names a variant the adapters would annotate with the wrong ID
//! or none: the Registry spelled it in a way `canonical_hgvs` does not, or
//! the index pairs it with another allele's ID.

use std::io::{self, Write};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::Args;
use clingen_allele::{Annotator, CaIndex, Lookup, Status, format_caid, parse_caid};

use crate::hgvs_class::Class;
use crate::hgvs_vcf::{Parsed, hgvs_to_vcf};

/// Alleles longer than this many bases are cut short in example lines.
const SHOWN_BASES: usize = 20;

/// `reach`'s command line.
#[derive(Args)]
pub(crate) struct ReachArgs {
    /// Index directory to check (`caid.fst` and `manifest.json`).
    #[arg(long, value_name = "DIR")]
    index: PathBuf,
    /// vareffect genome binary (`genome.bin`). Default: the genome the
    /// index's manifest names.
    #[arg(long, value_name = "BIN")]
    genome: Option<PathBuf>,
    /// Check only the keys that start with this, e.g. `NC_012920.1:` for one
    /// contig's. Default: every key.
    #[arg(long, value_name = "P")]
    prefix: Option<String>,
    /// How many keys to show on stderr for each outcome other than
    /// `reachable`.
    #[arg(long, value_name = "N", default_value_t = 5)]
    examples: usize,
}

/// How checking one key ended.
///
/// Declaration order is the report order; the discriminants index
/// [`Report::outcomes`] and [`Report::examples`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The key's record found the key's own CA ID.
    Reachable,
    /// The key's record found another CA ID.
    WrongCa,
    /// The key's record canonicalises to an HGVS the index does not hold: the
    /// Registry wrote the allele differently from `canonical_hgvs`.
    NotFound,
    /// The genome disagrees with the REF the key names.
    RefMismatch,
    /// The lookup refused the key's record.
    Unsupported,
    /// A reference allele (`…=`, or a `delins` whose bases are the
    /// reference's), which no VCF record names.
    Reference,
    /// A key [`hgvs_to_vcf`] cannot convert, among them one whose record would
    /// hold a genome base other than ACGT (the `N` at rCRS 3107), which no
    /// lookup takes.
    Unparseable,
}

impl Outcome {
    /// Report order.
    const ALL: [Outcome; 7] = [
        Outcome::Reachable,
        Outcome::WrongCa,
        Outcome::NotFound,
        Outcome::RefMismatch,
        Outcome::Unsupported,
        Outcome::Reference,
        Outcome::Unparseable,
    ];

    /// The label the report and the example lines use.
    fn name(self) -> &'static str {
        match self {
            Outcome::Reachable => "reachable",
            Outcome::WrongCa => "wrong_ca",
            Outcome::NotFound => "not_found",
            Outcome::RefMismatch => "ref_mismatch",
            Outcome::Unsupported => "unsupported",
            Outcome::Reference => "reference",
            Outcome::Unparseable => "unparseable",
        }
    }

    /// Whether the outcome fails the run: the key names a variant, and its
    /// VCF record does not find it under its own ID.
    ///
    /// Reference alleles and forms `canonical_hgvs` never writes are only
    /// reported: no record the adapters key can name them, so the index
    /// holds them without any lookup landing on them.
    fn fails(self) -> bool {
        matches!(
            self,
            Outcome::WrongCa | Outcome::NotFound | Outcome::RefMismatch | Outcome::Unsupported
        )
    }
}

/// What became of one key: its outcome, the record it converted to (or why
/// it did not), and the lookup of that record, if there was one.
struct Checked {
    outcome: Outcome,
    parsed: Parsed,
    lookup: Option<Lookup>,
}

/// Convert `key` to a VCF record, look the record up, and compare the CA ID
/// it finds with the key's own, `caid`.
fn check(key: &[u8], caid: u64, annotator: &Annotator) -> Checked {
    let unconverted = |outcome, parsed| Checked {
        outcome,
        parsed,
        lookup: None,
    };
    // Every writer of the index takes keys as `&str`, but the FST holds bytes.
    let Ok(key) = std::str::from_utf8(key) else {
        return unconverted(
            Outcome::Unparseable,
            Parsed::Unparseable("key is not UTF-8"),
        );
    };
    let parsed = hgvs_to_vcf(key, |chrom, start1, end1| {
        annotator.ref_seq(chrom, start1, end1)
    });
    let record = match &parsed {
        Parsed::Vcf(record) => record,
        Parsed::Reference => return unconverted(Outcome::Reference, parsed),
        Parsed::Unparseable(_) => return unconverted(Outcome::Unparseable, parsed),
    };
    let lookup = annotator.lookup(
        record.chrom,
        record.pos1,
        &record.ref_allele,
        &record.alt_allele,
    );
    let outcome = match lookup.status {
        // As numbers: the lookup prints the ID padded to six digits.
        Status::Found if lookup.caid.as_deref().and_then(parse_caid) == Some(caid) => {
            Outcome::Reachable
        }
        Status::Found => Outcome::WrongCa,
        Status::NotInIndex => Outcome::NotFound,
        Status::RefMismatch => Outcome::RefMismatch,
        Status::Unsupported => Outcome::Unsupported,
    };
    Checked {
        outcome,
        parsed,
        lookup: Some(lookup),
    }
}

/// The example line for a key that was not reachable: the outcome, the key
/// and its ID, then the record it converted to (or why it did not) and, for
/// `wrong_ca` and `not_found`, what the lookup found or keyed instead.
fn describe(key: &str, caid: u64, checked: &Checked) -> String {
    let mut line = format!("{}: {key} {}", checked.outcome.name(), format_caid(caid));
    match &checked.parsed {
        Parsed::Vcf(record) => line.push_str(&format!(
            " vcf={}:{}:{}:{}",
            record.chrom,
            record.pos1,
            show(&record.ref_allele),
            show(&record.alt_allele)
        )),
        Parsed::Reference => {}
        Parsed::Unparseable(reason) => line.push_str(&format!(" ({reason})")),
    }
    let lookup = checked.lookup.as_ref();
    let instead = match checked.outcome {
        Outcome::WrongCa => lookup
            .and_then(|l| l.caid.as_deref())
            .map(|id| ("found", id)),
        Outcome::NotFound => lookup
            .and_then(|l| l.hgvs.as_deref())
            .map(|hgvs| ("ours", hgvs)),
        _ => None,
    };
    if let Some((label, value)) = instead {
        line.push_str(&format!(" {label}={value}"));
    }
    line
}

/// An allele for an example line, cut short when long: a large deletion's
/// REF can run to millions of bases.
fn show(allele: &[u8]) -> String {
    if allele.len() <= SHOWN_BASES {
        String::from_utf8_lossy(allele).into_owned()
    } else {
        format!(
            "{}...({} bp)",
            String::from_utf8_lossy(&allele[..SHOWN_BASES]),
            allele.len()
        )
    }
}

/// The counts `reach` prints and the example lines it keeps.
struct Report {
    keys: u64,
    /// Keys per outcome, indexed like [`Outcome::ALL`].
    outcomes: [u64; Outcome::ALL.len()],
    /// Keys and reachable keys per class, indexed like [`Class::ALL`].
    class_keys: [u64; Class::ALL.len()],
    class_reachable: [u64; Class::ALL.len()],
    /// Example lines per outcome, indexed like [`Outcome::ALL`]; never any
    /// for `reachable`.
    examples: [Vec<String>; Outcome::ALL.len()],
    max_examples: usize,
}

impl Report {
    fn new(max_examples: usize) -> Self {
        Self {
            keys: 0,
            outcomes: [0; Outcome::ALL.len()],
            class_keys: [0; Class::ALL.len()],
            class_reachable: [0; Class::ALL.len()],
            examples: Default::default(),
            max_examples,
        }
    }

    fn record(&mut self, class: Class, outcome: Outcome) {
        self.keys += 1;
        self.outcomes[outcome as usize] += 1;
        self.class_keys[class as usize] += 1;
        self.class_reachable[class as usize] += u64::from(outcome == Outcome::Reachable);
    }

    /// Whether a key with this outcome should be shown; checked before its
    /// line is built, so a run of millions of keys builds only a few.
    fn wants_example(&self, outcome: Outcome) -> bool {
        outcome != Outcome::Reachable && self.examples[outcome as usize].len() < self.max_examples
    }

    fn add_example(&mut self, outcome: Outcome, line: String) {
        self.examples[outcome as usize].push(line);
    }

    fn failed(&self) -> bool {
        Outcome::ALL
            .into_iter()
            .any(|outcome| outcome.fails() && self.outcomes[outcome as usize] > 0)
    }

    /// `keys`, a line per outcome, then the per-class table.
    fn write_counts(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "keys\t{}", self.keys)?;
        for outcome in Outcome::ALL {
            writeln!(
                out,
                "{}\t{}",
                outcome.name(),
                self.outcomes[outcome as usize]
            )?;
        }
        writeln!(out, "class\tkeys\treachable")?;
        for class in Class::ALL {
            let i = class as usize;
            writeln!(
                out,
                "{}\t{}\t{}",
                class.name(),
                self.class_keys[i],
                self.class_reachable[i]
            )?;
        }
        Ok(())
    }

    /// The kept example lines, outcome by outcome in report order.
    fn write_examples(&self, out: &mut impl Write) -> io::Result<()> {
        for line in self.examples.iter().flatten() {
            writeln!(out, "{line}")?;
        }
        Ok(())
    }
}

/// Run `reach`: check every key of `args.index` (under `args.prefix`), print
/// the counts on stdout and examples on stderr, and exit 1 when a key that
/// names a variant is not found under its own ID.
pub(crate) fn reach(args: ReachArgs) -> Result<ExitCode> {
    // Opened first, so a missing or broken index is reported as such rather
    // than as a manifest that names no genome.
    let index = CaIndex::open(&args.index)
        .with_context(|| format!("opening index {}", args.index.display()))?;
    let genome = match args.genome {
        Some(genome) => genome,
        None => Annotator::genome_path_from_manifest(&args.index).ok_or_else(|| {
            anyhow!(
                "{} names no genome; pass --genome",
                args.index.join("manifest.json").display()
            )
        })?,
    };
    let annotator = Annotator::open(&args.index, &genome).with_context(|| {
        format!(
            "opening index {} with genome {}",
            args.index.display(),
            genome.display()
        )
    })?;

    let mut report = Report::new(args.examples);
    let prefix = args.prefix.unwrap_or_default();
    index.visit_prefix(prefix.as_bytes(), |key, caid| {
        let checked = check(key, caid, &annotator);
        let key = String::from_utf8_lossy(key);
        report.record(Class::of(&key), checked.outcome);
        if report.wants_example(checked.outcome) {
            report.add_example(checked.outcome, describe(&key, caid, &checked));
        }
        ControlFlow::Continue(())
    });

    report
        .write_examples(&mut io::stderr().lock())
        .context("writing examples to stderr")?;
    report
        .write_counts(&mut io::stdout().lock())
        .context("writing the counts to stdout")?;
    Ok(if report.failed() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
