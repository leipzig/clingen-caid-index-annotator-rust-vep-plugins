//! Test support: a tiny synthetic GRCh38 genome built from real sequence.
//!
//! Enabled by the `test-support` cargo feature so that this crate's own tests
//! and downstream crates share one fixture. The reference windows in
//! `tests/data/windows.fa` are real hg38 sequence fetched once from the UCSC
//! REST API and committed; nothing here touches the network.

use std::path::Path;

use tempfile::TempDir;
use vareffect::FastaReader;
use vareffect::fasta::write_genome_binary;

/// The committed reference windows, as FASTA text.
///
/// Headers are `>chr:start-end` with 1-based inclusive coordinates, e.g.
/// `>chr21:33000001-33000300`.
pub const WINDOWS_FA: &str = include_str!("../tests/data/windows.fa");

/// Length of the synthetic `chr21` (long enough to hold every window).
pub const CHR21_LEN: usize = 33_000_400;

/// Length of the synthetic `chrM` (the rCRS, `NC_012920.1`).
pub const CHRM_LEN: usize = 16_569;

/// One reference window from [`WINDOWS_FA`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// UCSC contig name, e.g. `chr21`.
    pub chrom: String,
    /// 1-based position of the first base.
    pub start1: u64,
    /// 1-based position of the last base (inclusive).
    pub end1: u64,
    /// Uppercase `ACGT` bases, `end1 - start1 + 1` of them.
    pub seq: Vec<u8>,
}

impl Window {
    /// The 1-based reference base at `pos1`, or `None` outside the window.
    pub fn base_at(&self, pos1: u64) -> Option<u8> {
        let offset = pos1.checked_sub(self.start1)?;
        self.seq.get(usize::try_from(offset).ok()?).copied()
    }
}

/// Parse [`WINDOWS_FA`].
///
/// # Panics
///
/// If the committed fixture is malformed.
pub fn windows() -> Vec<Window> {
    let mut out: Vec<Window> = Vec::new();
    for line in WINDOWS_FA.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(header) = line.strip_prefix('>') {
            let (chrom, range) = header.split_once(':').expect("header is chrom:start-end");
            let (start1, end1) = range.split_once('-').expect("header is chrom:start-end");
            out.push(Window {
                chrom: chrom.to_string(),
                start1: start1.parse().expect("numeric window start"),
                end1: end1.parse().expect("numeric window end"),
                seq: Vec::new(),
            });
        } else {
            let window = out.last_mut().expect("sequence line after a header");
            window
                .seq
                .extend(line.bytes().map(|b| b.to_ascii_uppercase()));
        }
    }
    for w in &out {
        assert_eq!(
            w.seq.len() as u64,
            w.end1 - w.start1 + 1,
            "window {}:{}-{} has the wrong length",
            w.chrom,
            w.start1,
            w.end1
        );
    }
    out
}

/// A `len`-byte contig of `N` with every window on `chrom` pasted in.
fn contig_with_windows(chrom: &str, len: usize, windows: &[Window]) -> Vec<u8> {
    let mut seq = vec![b'N'; len];
    for w in windows.iter().filter(|w| w.chrom == chrom) {
        let start = usize::try_from(w.start1 - 1).expect("window offset fits in usize");
        seq[start..start + w.seq.len()].copy_from_slice(&w.seq);
    }
    seq
}

/// Write the tiny genome (`chr21` and `chrM`, build `GRCh38`) as
/// `genome.bin` and `genome.bin.idx` under `dir`, returning the `.bin` path.
fn write_tiny_genome(dir: &Path) -> std::path::PathBuf {
    let windows = windows();
    let chr21 = contig_with_windows("chr21", CHR21_LEN, &windows);
    let chrm = contig_with_windows("chrM", CHRM_LEN, &windows);

    let bin = dir.join("genome.bin");
    let idx = dir.join("genome.bin.idx");
    write_genome_binary(&[("chr21", &chr21), ("chrM", &chrm)], "GRCh38", &bin, &idx)
        .expect("write tiny genome");
    bin
}

/// Build the tiny genome in a fresh temporary directory and open it.
///
/// `chr21` is [`CHR21_LEN`] bytes of `N` and `chrM` is [`CHRM_LEN`] bytes of
/// `N`, each with the [`windows`] on that contig pasted in at their 1-based
/// offsets. Keep the returned [`TempDir`] alive for as long as the reader is
/// in use, and use its path (`dir.path().join("genome.bin")`) to reopen the
/// genome elsewhere.
///
/// # Panics
///
/// On any I/O failure, since this is test-only code.
pub fn tiny_genome() -> (TempDir, FastaReader) {
    let dir = TempDir::new().expect("create temp dir");
    let bin = write_tiny_genome(dir.path());
    let reader = FastaReader::open(&bin).expect("open tiny genome");
    (dir, reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_the_three_committed_ones() {
        let ws = windows();
        let summary: Vec<_> = ws
            .iter()
            .map(|w| (w.chrom.as_str(), w.start1, w.end1, w.seq.len()))
            .collect();
        assert_eq!(
            summary,
            [
                ("chr21", 33_000_001, 33_000_300, 300),
                ("chr21", 5_030_500, 5_030_650, 151),
                ("chrM", 3_200, 3_300, 101),
            ]
        );
        // Facts confirmed against the UCSC hg38 sequence.
        assert_eq!(ws[0].base_at(33_000_166), Some(b'T'));
        assert_eq!(&ws[0].seq[166..172], b"AAAAAA");
        assert_eq!(ws[1].base_at(5_030_578), Some(b'C'));
        assert_eq!(ws[2].base_at(3_243), Some(b'A'));
        assert_eq!(ws[2].base_at(3_199), None);
        assert_eq!(ws[2].base_at(3_301), None);
    }

    #[test]
    fn tiny_genome_has_windows_in_n_background() {
        let (_dir, genome) = tiny_genome();
        assert_eq!(genome.chrom_length("chr21"), Some(CHR21_LEN as u64));
        assert_eq!(genome.chrom_length("chrM"), Some(CHRM_LEN as u64));
        assert_eq!(genome.chrom_length("chr1"), None);

        // 0-based reads: position 33000166 (1-based) is T, then six As.
        assert_eq!(genome.fetch_base("chr21", 33_000_165).unwrap(), b'T');
        assert_eq!(
            genome
                .fetch_sequence("chr21", 33_000_166, 33_000_172)
                .unwrap(),
            b"AAAAAA"
        );
        assert_eq!(genome.fetch_base("chrM", 3_242).unwrap(), b'A');
        // Outside every window the background is N.
        assert_eq!(genome.fetch_base("chr21", 33_000_300).unwrap(), b'N');
        assert_eq!(genome.fetch_base("chr21", 5_030_498).unwrap(), b'N');
        assert_eq!(genome.fetch_base("chrM", 3_300).unwrap(), b'N');
        assert_eq!(genome.fetch_base("chrM", 3_198).unwrap(), b'N');
    }
}
