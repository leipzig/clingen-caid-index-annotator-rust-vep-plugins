//! Convert the UCSC hg38 `chr21.fa` into a chr21-only vareffect genome binary.
//!
//! ```text
//! cargo run --release -p clingen-allele --example fasta_to_bin -- <chr21.fa> <out.bin>
//! ```
//!
//! Writes `<out.bin>` and `<out.bin>.idx` with one contig, `chr21`, and build
//! `GRCh38`. The FASTA must be uncompressed and hold exactly one record named
//! `chr21`; soft-masked (lowercase) bases are uppercased by
//! `write_genome_binary`. `scripts/build_chr21_genome.sh` downloads the FASTA
//! and runs this.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use vareffect::fasta::write_genome_binary;

const CONTIG: &str = "chr21";
const BUILD: &str = "GRCh38";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let [fasta, bin] = <[OsString; 2]>::try_from(args)
        .map_err(|_| "usage: fasta_to_bin <chr21.fa> <out.bin>".to_owned())?;
    let (fasta, bin) = (PathBuf::from(fasta), PathBuf::from(bin));
    let mut idx = bin.clone().into_os_string();
    idx.push(".idx");
    let idx = PathBuf::from(idx);

    let text = std::fs::read(&fasta).map_err(|e| format!("reading {}: {e}", fasta.display()))?;
    let seq = single_record(&text, CONTIG).map_err(|e| format!("{}: {e}", fasta.display()))?;
    write_genome_binary(&[(CONTIG, &seq)], BUILD, &bin, &idx)
        .map_err(|e| format!("writing {}: {e}", bin.display()))?;

    eprintln!(
        "wrote {CONTIG} ({} bases, build {BUILD}) to {} and {}",
        seq.len(),
        bin.display(),
        idx.display()
    );
    Ok(())
}

/// The sequence of a FASTA holding exactly one record, named `name`.
fn single_record(text: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut lines = text
        .split(|&b| b == b'\n')
        .map(<[u8]>::trim_ascii)
        .filter(|line| !line.is_empty());

    let header = lines.next().ok_or("empty FASTA")?;
    let header_name = header
        .strip_prefix(b">")
        .and_then(|h| h.split(u8::is_ascii_whitespace).next())
        .ok_or("first line is not a FASTA header")?;
    if header_name != name.as_bytes() {
        return Err(format!(
            "record is {:?}, expected {name:?}",
            String::from_utf8_lossy(header_name)
        ));
    }

    let mut seq = Vec::with_capacity(text.len());
    for line in lines {
        if line.starts_with(b">") {
            return Err(format!("more than one record (expected only {name:?})"));
        }
        seq.extend_from_slice(line);
    }
    if seq.is_empty() {
        return Err(format!("record {name:?} has no sequence"));
    }
    Ok(seq)
}
