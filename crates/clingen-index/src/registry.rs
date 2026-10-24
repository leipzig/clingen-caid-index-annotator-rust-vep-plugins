//! `clingen-index registry`: ask the ClinGen Allele Registry for CA IDs.
//!
//! Two questions, one subcommand each:
//!
//! - `enumerate` lists every allele the Registry holds inside each query
//!   window that `regions` wrote (`GET /alleles?refseq=…&begin=…&end=…`),
//!   paging with `limit` and `skip` while a page comes back full, and writes
//!   `HGVS<TAB>CAID` for each allele's HGVS on the window's own GRCh38
//!   accession. Reference alleles (`…=`) are left out: they name no change,
//!   and no VCF record looks one up. The Registry returns the alleles whose
//!   start is greater than `begin` and whose end is at most `end`, which is
//!   the span `regions` already wrote, so the columns go out unchanged.
//! - `lookup` asks about given sites (`POST /alleles?file=vcf`). It is the
//!   port of `harvest/query_registry.py` and writes the same seven columns,
//!   `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`, so its output is a
//!   harvester chunk that `build --chunks` and `validate` read.
//!
//! Both only read. Neither ever sends `PUT`, which would register alleles.
//!
//! One agent makes every request of a run, one at a time, under the
//! `User-Agent` `clingen-index/<version>`. A request that fails in a way that
//! may pass (a timeout, a dropped connection, 429, a 5xx, or a body that is
//! not the JSON asked for, as when a reply is cut short) is tried again after
//! 30 s, then 60 s, and so on, six attempts in all. Any other failure, such as
//! a 400 or a certificate error, ends the run at once: asking again would get
//! the same answer. Otherwise ureq's defaults hold: the Registry's gzip is
//! undone, and a proxy named in the environment (`HTTPS_PROXY` and the like)
//! carries the requests.
//!
//! Output goes to `<out>.partial` and is renamed to `<out>` only once every
//! request has been answered, so a run that fails, or a workflow task that is
//! retried, never leaves a file that looks complete but is not.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{Args, Subcommand};
use clingen_allele::resolve_contig;
use serde::Deserialize;
use ureq::http::Response;
use ureq::{Agent, Body};

use crate::input::{input_label, open_input};
use crate::regions::bare_name;

/// Attempts at one request before the run gives up.
const ATTEMPTS: u32 = 6;

/// Largest body read, in bytes on the wire (the gzip the Registry sends is
/// undone after this limit). ureq's own default of 10 MiB is below a full
/// 50,000-allele page or a 250,000-site lookup.
const BODY_LIMIT: u64 = 1 << 30;

/// Whole-request budget, from DNS to the last body byte. A 250,000-site
/// lookup took about two minutes in the harvest (2,100 sites/s).
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(600);

/// Budget for connecting, TLS handshake included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Bytes of a failing reply's body quoted in the error.
const QUOTED_BODY: usize = 200;

/// Who is asking, as the Registry's operators see it.
const USER_AGENT: &str = concat!("clingen-index/", env!("CARGO_PKG_VERSION"));

/// The fields `enumerate` asks for. Sent raw: ureq would percent-encode `+`
/// and `@`, and this literal form is the one known to work.
const ENUMERATE_FIELDS: &str = "none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome";

/// The fields `lookup` asks for, as `harvest/query_registry.py` sends them.
const LOOKUP_FIELDS: &str = "none+@id+genomicAlleles.referenceGenome+genomicAlleles.hgvs";

/// What follows the host in a registered allele's `@id`, before its CA ID.
const ALLELE_PATH: &str = "/allele/";

/// Arguments of `registry`.
#[derive(Args)]
pub(crate) struct RegistryArgs {
    #[command(subcommand)]
    command: RegistryCommand,
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// List every allele the Registry holds inside each query window.
    ///
    /// Asks for each window of the --windows files in turn, page by page, and
    /// writes `HGVS<TAB>CAID` for every allele with a non-reference HGVS on
    /// the window's accession, in the order the Registry gave them. Prints
    /// `windows requests alleles reference no_grch38_hgvs written` on stderr.
    /// Exits 2 if a window line is malformed, a request fails, or an allele
    /// to be written has no CA ID; nothing is written then.
    Enumerate(EnumerateArgs),
    /// Look up the CA ID and HGVS of each site, as the Python harvester did.
    ///
    /// Sends the sites as a VCF, --batch at a time, and writes one row per
    /// site: `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`, `CAID` empty
    /// for an allele nobody registered and `ERROR:<type>` for a site the
    /// Registry rejected. Prints
    /// `sites requests registered unregistered error written` on stderr.
    /// Exits 2 if a site line is malformed, a request fails, or an answer
    /// does not hold one result per site; nothing is written then.
    Lookup(LookupArgs),
}

/// Where the Registry is and how long to wait between attempts.
#[derive(Args)]
struct ClientArgs {
    /// Registry to ask; `/alleles` is appended.
    #[arg(long, value_name = "URL", default_value = "https://reg.genome.network")]
    base_url: String,
    /// Wait before the second attempt, in milliseconds, doubling after each
    /// further failure. Tests shorten it.
    #[arg(long, value_name = "N", default_value_t = 30_000, hide = true)]
    retry_base_ms: u64,
}

/// Arguments of `registry enumerate`.
#[derive(Args)]
struct EnumerateArgs {
    /// Windows files as `regions` writes them (no header; columns `chrom
    /// accession begin end core_begin core_end`), read in the order given.
    #[arg(long, required = true, num_args = 1.., value_name = "TSV")]
    windows: Vec<PathBuf>,
    /// Output `HGVS<TAB>CAID` file, written as `<TSV>.partial` first.
    #[arg(long, value_name = "TSV")]
    out: PathBuf,
    /// Alleles asked for per request; a page this full is followed by the
    /// next.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 50_000,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    page_size: u64,
    #[command(flatten)]
    client: ClientArgs,
}

/// Arguments of `registry lookup`.
#[derive(Args)]
struct LookupArgs {
    /// Sites to look up: `CHROM POS REF ALT`, tab-separated, no header,
    /// with bare GRCh38 names (`21`, `X`, `MT`); plain or gzip, or `-`.
    #[arg(long, value_name = "TSV")]
    sites: PathBuf,
    /// Output TSV of the seven harvester columns, written as `<TSV>.partial`
    /// first.
    #[arg(long, value_name = "TSV")]
    out: PathBuf,
    /// Sites sent per request.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 250_000,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    batch: u64,
    #[command(flatten)]
    client: ClientArgs,
}

/// One element of a Registry answer: an allele, or for a lookup a VCF line
/// the Registry rejected. Fields not asked for are ignored.
#[derive(Deserialize)]
struct Allele {
    /// `http://reg.genome.network/allele/CA…` for a registered allele; `_:CA`
    /// for a looked-up allele nobody registered.
    #[serde(rename = "@id")]
    id: Option<String>,
    /// The allele on each reference genome the Registry knows it on.
    #[serde(rename = "genomicAlleles", default)]
    genomic_alleles: Vec<GenomicAllele>,
    /// Why a lookup line was rejected, e.g. `IncorrectReferenceAllele`.
    #[serde(rename = "errorType")]
    error_type: Option<String>,
}

/// An allele on one reference genome.
#[derive(Deserialize)]
struct GenomicAllele {
    /// Its HGVS on each accession of that genome, e.g. `NC_000021.9:…` and
    /// `CM000683.2:…` for GRCh38.
    #[serde(default)]
    hgvs: Vec<String>,
    /// `GRCh38`, `GRCh37` or `NCBI36`.
    #[serde(rename = "referenceGenome")]
    reference_genome: Option<String>,
}

/// Why one attempt at a request gave no answer.
enum Failure {
    /// May pass on another attempt.
    Transient(String),
    /// Would fail the same way again.
    Permanent(String),
}

/// The connection to the Registry: one agent for every request of a run.
struct Client {
    agent: Agent,
    alleles_url: String,
    retry_base: Duration,
}

impl Client {
    fn new(args: &ClientArgs) -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(GLOBAL_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            // A 4xx or 5xx comes back as a response, so its body can be
            // quoted and its status decide between retrying and failing.
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build()
            .into();
        Client {
            agent,
            alleles_url: format!("{}/alleles", args.base_url.trim_end_matches('/')),
            retry_base: Duration::from_millis(args.retry_base_ms),
        }
    }

    /// The page of at most `limit` alleles, after the first `skip`, that lie
    /// inside `(begin, end]` on `refseq`.
    fn enumerate_page(
        &self,
        refseq: &str,
        begin: u64,
        end: u64,
        limit: u64,
        skip: u64,
    ) -> Result<Vec<Allele>> {
        let what = format!("GET {refseq} {begin}-{end} skip={skip}");
        self.with_retries(&what, || {
            self.agent
                .get(&self.alleles_url)
                .query("refseq", refseq)
                .query("begin", begin.to_string())
                .query("end", end.to_string())
                .query_raw("fields", ENUMERATE_FIELDS)
                .query("limit", limit.to_string())
                .query("skip", skip.to_string())
                .call()
        })
    }

    /// The Registry's answer for each data line of `vcf`, in order.
    fn lookup(&self, vcf: &str) -> Result<Vec<Allele>> {
        self.with_retries("POST /alleles?file=vcf", || {
            self.agent
                .post(&self.alleles_url)
                .query("file", "vcf")
                .query_raw("fields", LOOKUP_FIELDS)
                // What Python's urllib sent for the harvester, the form the
                // Registry is known to accept.
                .content_type("application/x-www-form-urlencoded")
                .send(vcf)
        })
    }

    /// Make the request `send` builds until it is answered, waiting
    /// `retry_base`, then twice that, and so on, after each transient
    /// failure, and reporting each on stderr as `what`.
    fn with_retries(
        &self,
        what: &str,
        send: impl Fn() -> Result<Response<Body>, ureq::Error>,
    ) -> Result<Vec<Allele>> {
        let mut wait = self.retry_base;
        let mut attempt = 1;
        loop {
            match answer(send()) {
                Ok(alleles) => return Ok(alleles),
                Err(Failure::Permanent(why)) => bail!("{what}: {why}"),
                Err(Failure::Transient(why)) if attempt == ATTEMPTS => {
                    bail!("{what}: {why}; gave up after {ATTEMPTS} attempts")
                }
                Err(Failure::Transient(why)) => {
                    eprintln!(
                        "{what}: attempt {attempt} of {ATTEMPTS} failed ({why}); \
                         retrying in {wait:?}"
                    );
                    thread::sleep(wait);
                    wait = wait.saturating_mul(2);
                    attempt += 1;
                }
            }
        }
    }
}

/// The alleles in a response, or why there are none.
///
/// A 2xx body that does not parse is transient: a reply cut short arrives as
/// complete HTTP holding incomplete JSON. A 429 or 5xx is transient; any
/// other status is not, and fails with the start of the body, which is where
/// the Registry explains a refusal.
fn answer(response: Result<Response<Body>, ureq::Error>) -> Result<Vec<Allele>, Failure> {
    let mut response = response.map_err(classify)?;
    let status = response.status().as_u16();
    let ok = (200..300).contains(&status);
    let body = match response
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_string()
    {
        Ok(body) => body,
        Err(e) if ok => return Err(classify(e)),
        // The status alone decides what a failed request means.
        Err(e) => format!("(body unreadable: {e})"),
    };
    match status {
        _ if ok => serde_json::from_str(&body).map_err(|e| {
            Failure::Transient(format!(
                "the answer is not the JSON asked for ({e}): {}",
                quote(&body)
            ))
        }),
        429 | 500..=599 => Err(Failure::Transient(format!(
            "HTTP {status}: {}",
            quote(&body)
        ))),
        _ => Err(Failure::Permanent(format!(
            "HTTP {status}: {}",
            quote(&body)
        ))),
    }
}

/// Sort a ureq error into one worth another attempt or not.
///
/// TLS failures, certificate errors among them, arrive as `Io` of kind
/// `InvalidData` (rustls wraps them so), as does a body that is not UTF-8;
/// both would recur, so they are not retried. So are a body over the limit, a
/// bad URL and the rest of ureq's configuration errors.
fn classify(e: ureq::Error) -> Failure {
    use ureq::Error as E;
    let transient = match &e {
        E::Io(io) => io.kind() != ErrorKind::InvalidData,
        E::Timeout(_)
        | E::HostNotFound
        | E::ConnectionFailed
        | E::Protocol(_)
        | E::Decompress(..) => true,
        _ => false,
    };
    if transient {
        Failure::Transient(e.to_string())
    } else {
        Failure::Permanent(e.to_string())
    }
}

/// The first [`QUOTED_BODY`] bytes of `body`, cut at a character boundary.
fn quote(body: &str) -> &str {
    body[..body.floor_char_boundary(QUOTED_BODY)].trim_end()
}

/// The CA ID an `@id` ends in (`http://reg.genome.network/allele/CA068622`
/// gives `CA068622`, leading zeros and all), or `None` for one that names no
/// registered allele (`_:CA`). An `@id` with the allele path but no CA ID
/// after it is an error: the answer's form has changed.
fn ca_id(id: &str) -> Result<Option<&str>> {
    let Some((_, ca)) = id.split_once(ALLELE_PATH) else {
        return Ok(None);
    };
    let digits = ca.strip_prefix("CA").unwrap_or_default();
    ensure!(
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        "@id {id:?} does not end in a CA ID"
    );
    Ok(Some(ca))
}

/// A TSV input read line by line, keeping the line number for errors.
struct Lines {
    label: String,
    input: Box<dyn BufRead>,
    line_no: u64,
    buf: String,
}

impl Lines {
    fn open(path: &Path) -> Result<Self> {
        Ok(Lines {
            label: input_label(path),
            input: open_input(path)?,
            line_no: 0,
            buf: String::new(),
        })
    }

    /// `parse` of the next line that is not empty, or `None` at the end; an
    /// error names `<file>:<line>`.
    fn next<T>(&mut self, parse: impl FnOnce(&str) -> Result<T>) -> Result<Option<T>> {
        loop {
            self.buf.clear();
            self.line_no += 1;
            let n = self
                .input
                .read_line(&mut self.buf)
                .with_context(|| format!("{}:{}", self.label, self.line_no))?;
            if n == 0 {
                return Ok(None);
            }
            let line = self.buf.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                continue;
            }
            return parse(line)
                .map(Some)
                .with_context(|| format!("{}:{}", self.label, self.line_no));
        }
    }

    /// `<file>:<line>` of the line last read.
    fn position(&self) -> String {
        format!("{}:{}", self.label, self.line_no)
    }
}

/// `field` as a non-negative integer, naming the column `what` on failure.
fn number(field: &str, what: &str) -> Result<u64> {
    field
        .parse()
        .map_err(|_| anyhow!("{what} {field:?} is not a non-negative integer"))
}

/// An output written as `<path>.partial` and renamed to `<path>` by
/// [`AtomicFile::commit`]. Dropped uncommitted, as when a request fails, it
/// removes the partial file.
struct AtomicFile {
    path: PathBuf,
    partial: PathBuf,
    out: BufWriter<File>,
    committed: bool,
}

impl AtomicFile {
    fn create(path: &Path) -> Result<Self> {
        let mut name = path.file_name().unwrap_or_default().to_owned();
        name.push(".partial");
        let partial = path.with_file_name(name);
        let file = File::create(&partial)
            .with_context(|| format!("cannot create {}", partial.display()))?;
        Ok(AtomicFile {
            path: path.to_owned(),
            partial,
            out: BufWriter::new(file),
            committed: false,
        })
    }

    fn write(&mut self, text: &str) -> Result<()> {
        self.out
            .write_all(text.as_bytes())
            .with_context(|| format!("writing {}", self.partial.display()))
    }

    /// Flush the partial file to disk and rename it over `path`.
    fn commit(mut self) -> Result<()> {
        self.out
            .flush()
            .and_then(|()| self.out.get_ref().sync_all())
            .with_context(|| format!("writing {}", self.partial.display()))?;
        fs::rename(&self.partial, &self.path).with_context(|| {
            format!(
                "cannot rename {} to {}",
                self.partial.display(),
                self.path.display()
            )
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if !self.committed {
            // Best effort: there is already an error to report.
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// A query window: the span `(begin, end]` of one GRCh38 accession.
struct Window {
    /// `<file>:<line>` it came from, for errors.
    position: String,
    accession: &'static str,
    begin: u64,
    end: u64,
}

/// Every window of `paths`, checked before the first request is made.
fn read_windows(paths: &[PathBuf]) -> Result<Vec<Window>> {
    let mut windows = Vec::new();
    for path in paths {
        let mut lines = Lines::open(path)?;
        while let Some((accession, begin, end)) = lines.next(parse_window)? {
            windows.push(Window {
                position: lines.position(),
                accession,
                begin,
                end,
            });
        }
    }
    Ok(windows)
}

/// The accession, begin and end of a windows line.
fn parse_window(line: &str) -> Result<(&'static str, u64, u64)> {
    let cols: Vec<&str> = line.split('\t').collect();
    ensure!(
        cols.len() == 6,
        "{} tab-separated columns, but a windows line has 6 \
         (chrom accession begin end core_begin core_end)",
        cols.len()
    );
    let accession = resolve_contig(cols[1])
        .map(|contig| contig.accession)
        .filter(|&accession| accession == cols[1])
        .ok_or_else(|| {
            anyhow!(
                "accession {:?} is not that of a GRCh38 primary contig",
                cols[1]
            )
        })?;
    let begin = number(cols[2], "begin")?;
    let end = number(cols[3], "end")?;
    ensure!(begin < end, "window {begin}-{end} is empty or backwards");
    Ok((accession, begin, end))
}

/// What `enumerate` saw, for its stderr line.
#[derive(Default)]
struct EnumerateTally {
    /// Pages answered; a retried request counts once.
    requests: u64,
    alleles: u64,
    /// Alleles whose HGVS on the window's accession ends in `=`.
    reference: u64,
    /// Alleles with no HGVS on the window's accession.
    no_grch38_hgvs: u64,
    written: u64,
}

/// The first HGVS of `allele` on `accession` (`NC_000021.9:g.…`), whichever
/// genomic allele lists it. The GenBank (`CM000683.2:`) and GRCh37
/// (`NC_000021.8:`) forms of the same allele are other accessions, and no VCF
/// record on GRCh38 looks them up.
fn window_hgvs<'a>(allele: &'a Allele, accession: &str) -> Option<&'a str> {
    allele
        .genomic_alleles
        .iter()
        .flat_map(|g| &g.hgvs)
        .map(String::as_str)
        .find(|h| {
            h.strip_prefix(accession)
                .is_some_and(|rest| rest.starts_with(':'))
        })
}

/// Ask for every page of `window` and write its rows.
fn enumerate_window(
    client: &Client,
    window: &Window,
    page_size: u64,
    out: &mut AtomicFile,
    tally: &mut EnumerateTally,
) -> Result<()> {
    let mut skip = 0;
    loop {
        let page =
            client.enumerate_page(window.accession, window.begin, window.end, page_size, skip)?;
        tally.requests += 1;
        let returned = page.len() as u64;
        ensure!(
            returned <= page_size,
            "asked for at most {page_size} alleles, the Registry returned {returned}"
        );
        let mut rows = String::new();
        for allele in &page {
            tally.alleles += 1;
            match window_hgvs(allele, window.accession) {
                None => tally.no_grch38_hgvs += 1,
                Some(hgvs) if hgvs.ends_with('=') => tally.reference += 1,
                Some(hgvs) => {
                    let id = allele.id.as_deref().unwrap_or_default();
                    let ca = ca_id(id)
                        .with_context(|| format!("allele {hgvs}"))?
                        .ok_or_else(|| anyhow!("allele {hgvs} has no CA ID (@id {id:?})"))?;
                    rows.push_str(&format!("{hgvs}\t{ca}\n"));
                    tally.written += 1;
                }
            }
        }
        out.write(&rows)?;
        if returned < page_size {
            return Ok(());
        }
        skip += page_size;
    }
}

/// Run `registry enumerate`.
fn enumerate(args: EnumerateArgs) -> Result<ExitCode> {
    let windows = read_windows(&args.windows)?;
    let client = Client::new(&args.client);
    let mut out = AtomicFile::create(&args.out)?;
    let mut tally = EnumerateTally::default();
    for window in &windows {
        enumerate_window(&client, window, args.page_size, &mut out, &mut tally)
            .with_context(|| window.position.clone())?;
    }
    out.commit()?;
    eprintln!(
        "windows={} requests={} alleles={} reference={} no_grch38_hgvs={} written={}",
        windows.len(),
        tally.requests,
        tally.alleles,
        tally.reference,
        tally.no_grch38_hgvs,
        tally.written
    );
    Ok(ExitCode::SUCCESS)
}

/// A site to look up, its columns kept as written so the output row repeats
/// them exactly.
struct Site {
    chrom: String,
    pos: String,
    reference: String,
    alt: String,
    /// GRCh38 length of `chrom`, for the VCF's `##contig` line.
    length: u64,
}

/// A sites line: `CHROM POS REF ALT`.
///
/// `CHROM` must be a bare GRCh38 name (`21`, `X`, `MT`), the names the
/// harvester declared to the Registry; `REF` and `ALT` go to the Registry as
/// they are, which answers a bad one with an `errorType` for that line.
fn parse_site(line: &str) -> Result<Site> {
    let cols: Vec<&str> = line.split('\t').collect();
    let [chrom, pos, reference, alt] = cols[..] else {
        bail!(
            "{} tab-separated columns, but a sites line has 4 (CHROM POS REF ALT)",
            cols.len()
        );
    };
    let contig = resolve_contig(chrom)
        .filter(|contig| bare_name(contig) == chrom)
        .ok_or_else(|| anyhow!("CHROM {chrom:?} is not a bare GRCh38 name (1-22, X, Y, MT)"))?;
    ensure!(
        number(pos, "POS")? > 0,
        "POS {pos:?} is not a 1-based position"
    );
    Ok(Site {
        chrom: chrom.to_owned(),
        pos: pos.to_owned(),
        reference: reference.to_owned(),
        alt: alt.to_owned(),
        length: contig.length,
    })
}

/// The VCF `harvest/query_registry.py` sent for `sites`: a `##contig` line for
/// each chromosome present, in string order (the Registry rejects data lines
/// without one), then one data line per site.
fn vcf(sites: &[Site]) -> String {
    let contigs: BTreeMap<&str, u64> = sites
        .iter()
        .map(|site| (site.chrom.as_str(), site.length))
        .collect();
    let mut text = String::from("##fileformat=VCFv4.2\n");
    for (chrom, length) in contigs {
        text.push_str(&format!(
            "##contig=<ID={chrom},length={length},assembly=GRCh38>\n"
        ));
    }
    text.push_str("#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n");
    for site in sites {
        text.push_str(&format!(
            "{}\t{}\t.\t{}\t{}\t.\t.\t.\n",
            site.chrom, site.pos, site.reference, site.alt
        ));
    }
    text
}

/// The first `NC_` HGVS of the first genomic allele on `genome`, or `""`, as
/// the harvester's `hgvs_for` takes it (a later genomic allele on the same
/// genome is not looked at).
fn harvester_hgvs<'a>(allele: &'a Allele, genome: &str) -> &'a str {
    allele
        .genomic_alleles
        .iter()
        .find(|g| g.reference_genome.as_deref() == Some(genome))
        .and_then(|g| g.hgvs.iter().find(|h| h.starts_with("NC_")))
        .map_or("", String::as_str)
}

/// What `lookup` saw, for its stderr line.
#[derive(Default)]
struct LookupTally {
    sites: u64,
    /// Batches answered; a retried request counts once.
    requests: u64,
    registered: u64,
    unregistered: u64,
    error: u64,
    written: u64,
}

/// Look up one batch of sites and write a row for each.
fn lookup_batch(
    client: &Client,
    sites: &[Site],
    out: &mut AtomicFile,
    tally: &mut LookupTally,
) -> Result<()> {
    let results = client.lookup(&vcf(sites))?;
    tally.requests += 1;
    // A complete answer of the wrong length cannot be matched to the sites,
    // and asking again would get the same answer.
    ensure!(
        results.len() == sites.len(),
        "expected {} results, got {}",
        sites.len(),
        results.len()
    );
    let mut rows = String::new();
    for (site, result) in sites.iter().zip(&results) {
        let (caid, grch38, grch37) = if let Some(error) = &result.error_type {
            tally.error += 1;
            (format!("ERROR:{error}"), "", "")
        } else {
            let ca = ca_id(result.id.as_deref().unwrap_or_default()).with_context(|| {
                format!(
                    "the answer for {} {} {} {}",
                    site.chrom, site.pos, site.reference, site.alt
                )
            })?;
            if ca.is_some() {
                tally.registered += 1;
            } else {
                tally.unregistered += 1;
            }
            (
                ca.unwrap_or_default().to_owned(),
                harvester_hgvs(result, "GRCh38"),
                harvester_hgvs(result, "GRCh37"),
            )
        };
        rows.push_str(&format!(
            "{}\t{}\t{}\t{}\t{caid}\t{grch38}\t{grch37}\n",
            site.chrom, site.pos, site.reference, site.alt
        ));
        tally.written += 1;
    }
    out.write(&rows)
}

/// Run `registry lookup`.
fn lookup(args: LookupArgs) -> Result<ExitCode> {
    let client = Client::new(&args.client);
    let label = input_label(&args.sites);
    let mut lines = Lines::open(&args.sites)?;
    let mut out = AtomicFile::create(&args.out)?;
    let mut tally = LookupTally::default();
    let batch_size = usize::try_from(args.batch).unwrap_or(usize::MAX);
    let mut batch: Vec<Site> = Vec::new();
    // The first and last line of the batch, for errors.
    let mut span = (0, 0);
    loop {
        let site = lines.next(parse_site)?;
        let at_end = site.is_none();
        if let Some(site) = site {
            if batch.is_empty() {
                span.0 = lines.line_no;
            }
            span.1 = lines.line_no;
            tally.sites += 1;
            batch.push(site);
        }
        if batch.len() == batch_size || (at_end && !batch.is_empty()) {
            lookup_batch(&client, &batch, &mut out, &mut tally)
                .with_context(|| format!("{label}:{}-{}", span.0, span.1))?;
            batch.clear();
        }
        if at_end {
            break;
        }
    }
    out.commit()?;
    eprintln!(
        "sites={} requests={} registered={} unregistered={} error={} written={}",
        tally.sites,
        tally.requests,
        tally.registered,
        tally.unregistered,
        tally.error,
        tally.written
    );
    Ok(ExitCode::SUCCESS)
}

/// Run `registry`.
pub(crate) fn registry(args: RegistryArgs) -> Result<ExitCode> {
    match args.command {
        RegistryCommand::Enumerate(args) => enumerate(args),
        RegistryCommand::Lookup(args) => lookup(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_ids_keep_their_digits_and_unregistered_is_none() {
        assert_eq!(
            ca_id("http://reg.genome.network/allele/CA068622").unwrap(),
            Some("CA068622")
        );
        assert_eq!(ca_id("_:CA").unwrap(), None);
        assert_eq!(ca_id("").unwrap(), None);
        for bad in [
            "http://reg.genome.network/allele/",
            "http://reg.genome.network/allele/CA",
            "http://reg.genome.network/allele/PA123",
            "http://reg.genome.network/allele/CA12x",
        ] {
            assert!(ca_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn window_hgvs_needs_the_accession_and_its_colon() {
        let allele: Allele = serde_json::from_str(
            r#"{"@id": "x", "genomicAlleles": [
                {"hgvs": ["NC_000021.90:g.1A>G", "CM000683.2:g.5A>G"], "referenceGenome": "GRCh38"},
                {"hgvs": ["NC_000021.9:g.5A>G"], "referenceGenome": "GRCh38"}]}"#,
        )
        .unwrap();
        assert_eq!(
            window_hgvs(&allele, "NC_000021.9"),
            Some("NC_000021.9:g.5A>G")
        );
        assert_eq!(window_hgvs(&allele, "NC_000022.11"), None);
    }

    #[test]
    fn harvester_hgvs_takes_the_first_entry_of_the_genome_only() {
        // As `hgvs_for` does: the first GRCh38 entry has no NC_ HGVS, so the
        // answer is empty even though the second has one.
        let allele: Allele = serde_json::from_str(
            r#"{"genomicAlleles": [
                {"hgvs": ["CM000683.2:g.5A>G"], "referenceGenome": "GRCh38"},
                {"hgvs": ["NC_000021.9:g.5A>G"], "referenceGenome": "GRCh38"},
                {"hgvs": ["CM000683.1:g.7A>G", "NC_000021.8:g.7A>G"], "referenceGenome": "GRCh37"}]}"#,
        )
        .unwrap();
        assert_eq!(harvester_hgvs(&allele, "GRCh38"), "");
        assert_eq!(harvester_hgvs(&allele, "GRCh37"), "NC_000021.8:g.7A>G");
        assert_eq!(harvester_hgvs(&allele, "NCBI36"), "");
    }

    #[test]
    fn vcf_declares_each_contig_once_in_string_order() {
        let sites: Vec<Site> = ["3\t10\tA\tG", "21\t20\tC\tT", "3\t30\tG\tGA"]
            .into_iter()
            .map(|line| parse_site(line).unwrap())
            .collect();
        assert_eq!(
            vcf(&sites),
            "##fileformat=VCFv4.2\n\
             ##contig=<ID=21,length=46709983,assembly=GRCh38>\n\
             ##contig=<ID=3,length=198295559,assembly=GRCh38>\n\
             #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
             3\t10\t.\tA\tG\t.\t.\t.\n\
             21\t20\t.\tC\tT\t.\t.\t.\n\
             3\t30\t.\tG\tGA\t.\t.\t.\n"
        );
    }

    #[test]
    fn sites_need_a_bare_name_and_a_position() {
        assert!(parse_site("MT\t73\tA\tG").is_ok());
        for bad in [
            "chr21\t5\tA\tG",
            "x\t5\tA\tG",
            "M\t5\tA\tG",
            "21\t0\tA\tG",
            "21\t-5\tA\tG",
            "21\t5\tA",
            "21\t5\tA\tG\t.",
        ] {
            assert!(parse_site(bad).is_err(), "{bad:?}");
        }
    }
}
