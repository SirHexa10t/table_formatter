use clap::Parser;
use ordered_float::OrderedFloat;
use rayon::prelude::*;
use regex::Regex;
use std::cmp::Reverse;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write as _};
use std::iter::{repeat, repeat_n};
use std::path::Path;
use std::sync::LazyLock;


// ——— Configuration ——————————————————————————————
const DEFAULT_SEPARATOR: usize = 2;
const DEFAULT_THRESHOLD: usize = 2;

// Regular expression patterns
/// The numeric-cell grammar, shared by classification (`is_numeric_or_neutral`) and
/// evaluation (`parse_numeric`) so the two can never disagree:
///   number, optional space, optional scale letter, optional unit.
/// The scale letter multiplies the number (k/K = 10³, m/M = 10⁶, g/G = 10⁹, t/T = 10¹²;
/// a following `i` marks the 1024-based binary variant, as in KiB). `p` is a unit
/// (pixels, as in 1080p), never a multiplier.
static NUMERIC_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?P<num>[+-]?[0-9]+(?:\.[0-9]+)?)", // integer or decimal number
        r"\s?",                                // optional space before the suffix
        r"(?P<scale>[pKkMmGgTt])?",            // scale letter (or p = pixels)
        r"(?P<unit>i?[bB]?(?:/s)?|%|Hz|@[0-9]+Hz)?$", // units: MiB, %, Hz, @60Hz
    ))
    .unwrap()
});

// ——— Utilities ——————————————————————————————————————
/// Display width of `text` in terminal cells. ANSI escape sequences contribute nothing:
/// `measure_text_width` parses them itself (the equivalence test in tests.rs pins this,
/// including for malformed escapes — no pre-stripping needed).
pub fn visible_len(text: &str) -> usize {
    // Printable ASCII is one cell per byte — no escapes, no wide glyphs. Worth a fast
    // path because width measurement runs twice per cell (column sizing + padding).
    if text.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return text.len();
    }
    console::measure_text_width(text)
}

pub fn is_numeric_or_neutral(text: &str) -> bool {
    let clean = console::strip_ansi_codes(text);
    let clean = clean.trim();
    matches!(clean, "" | "-" | "--" | "---" | "*" | "−" | "=" | "y" | "n" | "?")
        || NUMERIC_PATTERN.is_match(clean)
}

/// Numeric interpretation of a cell: `Some(magnitude)` for values `NUMERIC_PATTERN`
/// accepts — plain numbers, scaled ones (`3.5K`, `-1.2GiB/s`), percentages, frequencies,
/// resolutions (`1080p`) — and `None` for anything else, neutral markers included.
pub(crate) fn parse_numeric(text: &str) -> Option<f64> {
    let clean = console::strip_ansi_codes(text);
    let caps = NUMERIC_PATTERN.captures(clean.trim())?;
    let number: f64 = caps["num"].parse().ok()?;

    let binary = caps.name("unit").is_some_and(|unit| unit.as_str().starts_with('i'));
    let scale_letter = caps
        .name("scale")
        .and_then(|scale| scale.as_str().chars().next())
        .map(|letter| letter.to_ascii_lowercase());

    let scale = match (scale_letter, binary) {
        (None, _) | (Some('p'), _) => 1.0, // no scale, or pixels (1080p) — not peta
        (Some('k'), false) => 1e3,
        (Some('k'), true) => 1024.0,
        (Some('m'), false) => 1e6,
        (Some('m'), true) => 1024f64.powi(2),
        (Some('g'), false) => 1e9,
        (Some('g'), true) => 1024f64.powi(3),
        (Some('t'), false) => 1e12,
        (Some('t'), true) => 1024f64.powi(4),
        _ => 1.0, // unreachable: the pattern admits no other letters
    };
    Some(number * scale)
}

/// Column-splitting regex for a given threshold: a run of `threshold`+ spaces (never fewer
/// than 2, so multi-word cells stay intact) or any run of tabs.
fn split_pattern(threshold: usize) -> Regex {
    Regex::new(&format!(r"\s{{{},}}|\t+", threshold.max(2))).unwrap()
}

fn split_row(line: &str, pattern: &Regex) -> Vec<String> {
    pattern.split(line.trim()).map(String::from).collect()
}

fn detect_column_properties(rows: &[Vec<String>]) -> (Vec<usize>, Vec<bool>) {
    let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let fresh = || (vec![0usize; num_cols], vec![true; num_cols]);

    // One pass per row, row-parallel: track each column's max display width, and whether
    // every data cell is numeric/neutral. Row 0 is exempt from the numeric vote — headers
    // are text — which also keeps ragged tables honest (the old per-column skip(1) skipped
    // a *data* cell wherever the header row was short).
    rows.par_iter()
        .enumerate()
        .fold(fresh, |(mut widths, mut numeric), (row_idx, row)| {
            for (col, cell) in row.iter().enumerate() {
                widths[col] = widths[col].max(visible_len(cell));
                if row_idx > 0 {
                    numeric[col] = numeric[col] && is_numeric_or_neutral(cell);
                }
            }
            (widths, numeric)
        })
        .reduce(fresh, |(widths_a, numeric_a), (widths_b, numeric_b)| {
            (
                widths_a.iter().zip(&widths_b).map(|(a, b)| *a.max(b)).collect(),
                numeric_a.iter().zip(&numeric_b).map(|(a, b)| *a && *b).collect(),
            )
        })
}

fn format_row(
    cells: &[String],
    widths: &[usize],
    is_numeric: &[bool],
    spacer: &str,
    trim_trailing: bool,
) -> String {
    // Pre-compute total capacity
    let total = widths.iter().sum::<usize>() + spacer.len() * widths.len().saturating_sub(1);
    let mut out = String::with_capacity(total);

    // Bind a single empty String for all "missing" cells
    let empty = String::new();

    // Zip widths, flags, and cells (falling back to &empty). Padding goes by *visible*
    // width: `{:<width$}` would count chars, letting ANSI codes soak up the padding and
    // making multi-cell glyphs (emoji, CJK) donate spaces they don't have.
    for ((&width, &numeric), cell) in widths
        .iter()
        .zip(is_numeric)
        .zip(cells.iter().chain(repeat(&empty)))
    {
        let pad = width.saturating_sub(visible_len(cell));
        if numeric {
            out.extend(repeat_n(' ', pad)); // right-align
            out.push_str(cell);
        } else {
            out.push_str(cell);
            out.extend(repeat_n(' ', pad)); // left-align
        }
        out.push_str(spacer);
    }

    // Trim off the trailing separator
    out.truncate(out.len().saturating_sub(spacer.len()));
    if trim_trailing {
        out.truncate(out.trim_end().len());
    }
    out
}

/// Sort data rows in place by column `idx` — descending for numeric columns (biggest
/// first), ascending for text. `header` decides whether row 0 is pinned on top:
/// `Some` overrides, `None` auto-detects — the first row is treated as a header unless
/// its sort cell parses as a number.
fn sort_rows(rows: &mut [Vec<String>], idx: usize, numeric: bool, header: Option<bool>) {
    let first_is_header = header.unwrap_or_else(|| {
        rows.first()
            .and_then(|row| row.get(idx))
            .is_none_or(|cell| parse_numeric(cell).is_none())
    });
    let skip = usize::from(first_is_header).min(rows.len());
    let data = &mut rows[skip..];

    if numeric {
        // Cached keys: the key is computed once per row, not once per comparison.
        // Missing and neutral cells (`-`, empty, …) sort to the bottom.
        data.sort_by_cached_key(|row| {
            let value = row.get(idx).and_then(|cell| parse_numeric(cell));
            Reverse(OrderedFloat(value.unwrap_or(f64::NEG_INFINITY)))
        });
    } else {
        // Compare what the user sees: an ANSI-styled cell must sort by its text, not by
        // its escape bytes. Cached so the strip runs once per row, not per comparison.
        data.sort_by_cached_key(|row| {
            row.get(idx)
                .map_or_else(String::new, |cell| console::strip_ansi_codes(cell).into_owned())
        });
    }
}

// ——— Errors ——————————————————————————————————————————
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// `sort` asked for a column the table doesn't have.
    SortColumnOutOfRange { requested: usize, num_cols: usize },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SortColumnOutOfRange { requested, num_cols } => write!(
                f,
                "sort column {requested} is out of range: the table has {num_cols} column(s)"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

// ——— Formatting options ——————————————————————————————————————
/// How a table gets formatted; `..Default::default()` gives the CLI's defaults.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// Number of spaces between columns.
    pub separator: usize,
    /// Minimum run of spaces treated as a column break (floored at 2).
    pub threshold: usize,
    /// Sort by this 0-based column: descending for numeric columns, ascending for text.
    pub sort: Option<usize>,
    /// `Some(true)`: the first row is a header and stays on top when sorting.
    /// `Some(false)`: the first row is data like any other.
    /// `None`: auto-detect — pinned unless its sort cell parses as a number.
    pub header: Option<bool>,
    /// Strip the trailing padding spaces from each output line.
    pub trim_trailing: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            separator: DEFAULT_SEPARATOR,
            threshold: DEFAULT_THRESHOLD,
            sort: None,
            header: None,
            trim_trailing: false,
        }
    }
}

// ——— Core formatting functions ——————————————————————————————————
pub fn format_table(lines: &[String], opts: &FormatOptions) -> Result<Vec<String>, FormatError> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }

    // Split rows - always use par_iter, rayon will handle the parallelization decision
    let pattern = split_pattern(opts.threshold);
    let mut rows: Vec<Vec<String>> =
        lines.par_iter().map(|line| split_row(line, &pattern)).collect();
    let (widths, is_numeric) = detect_column_properties(&rows);

    // sort, if asked to
    if let Some(idx) = opts.sort {
        if idx >= widths.len() {
            return Err(FormatError::SortColumnOutOfRange { requested: idx, num_cols: widths.len() });
        }
        sort_rows(&mut rows, idx, is_numeric[idx], opts.header);
    }

    // Format rows (the main feature; handle the spacing)
    let spacer = " ".repeat(opts.separator);
    Ok(rows
        .par_iter()
        .map(|row| format_row(row, &widths, &is_numeric, &spacer, opts.trim_trailing))
        .collect())
}

fn print_table(lines: &[String], opts: &FormatOptions) -> io::Result<()> {
    let table = format_table(lines, opts)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    // One locked, buffered writer for the whole table: per-line println! would take the
    // stdout lock and issue a write syscall for every line.
    let mut out = BufWriter::new(io::stdout().lock());
    for line in &table {
        writeln!(out, "{line}")?;
    }
    out.flush()
}

// ——— CLI Options ——————————————————————————————————————
#[derive(Parser)]
#[command(author, version, about = "Align whitespace-delimited columns into a neat table")]
pub struct Args {
    /// Input file path / data (or use stdin if not provided)
    #[arg(default_value = "-")]
    input: String,

    /// Number of spaces to separate columns
    #[arg(short, long, default_value_t = DEFAULT_SEPARATOR)]
    separator: usize,

    /// Minimum run of spaces treated as a column break (tabs always break); floored at 2, so
    /// a value with a couple of interior spaces stays in one cell.
    #[arg(short, long, default_value_t = DEFAULT_THRESHOLD)]
    threshold: usize,

    /// Sort by column index (0-based): numeric columns descending, text ascending.
    #[arg(long)]
    sort: Option<usize>,

    /// Treat the first row as a header that stays on top when sorting [default: auto-detect]
    #[arg(long, overrides_with = "no_header")]
    header: bool,

    /// Treat the first row as data: it participates in sorting [default: auto-detect]
    #[arg(long, overrides_with = "header")]
    no_header: bool,

    /// Strip trailing padding spaces from output lines
    #[arg(long)]
    remove_trailing_spaces: bool,
}

impl Args {
    fn format_options(&self) -> FormatOptions {
        FormatOptions {
            separator: self.separator,
            threshold: self.threshold,
            sort: self.sort,
            header: match (self.header, self.no_header) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
            trim_trailing: self.remove_trailing_spaces,
        }
    }
}

// ——— Library entry points ——————————————————————————————————————
/// Run exactly as the `table_formatter` binary does, reading arguments from the
/// process environment.
pub fn run() -> io::Result<()> {
    run_from(std::env::args_os())
}

/// Run with an explicit argument list (argv[0] should be the program name).
/// This lets another program invoke `table_formatter` in-process, as if it had
/// executed the binary with those arguments.
pub fn run_from<I, T>(args: I) -> io::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    run_with(Args::parse_from(args))
}

/// Read a command's input as lines from whichever source fits `input` — the "a file, a
/// pipe, or inline text" convenience, with no external dependency:
/// - `"-"` (or an empty string) reads stdin,
/// - a path to an existing file reads that file,
/// - anything else is treated as inline data and split into lines.
///
/// The file-existence check means a one-line inline string is handled as data rather than
/// mistaken for a path (which previously panicked in `File::open`).
pub fn read_lines(input: &str) -> io::Result<Vec<String>> {
    if input == "-" || input.is_empty() {
        return read_from(io::stdin().lock());
    }
    if Path::new(input).is_file() {
        return read_from(File::open(input)?);
    }
    Ok(input.lines().map(String::from).collect())
}

/// Collect a reader's contents as lines, decoding UTF-8 lossily so stray bytes don't abort.
pub(crate) fn read_from<R: Read>(mut reader: R) -> io::Result<Vec<String>> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).lines().map(String::from).collect())
}

/// Run with an already-parsed [`Args`]. This lets a dependent crate embed [`Args`]
/// directly in its own clap CLI (e.g. as a `Subcommand` variant) and hand it
/// straight here — so the argument definitions live only in this crate.
pub fn run_with(args: Args) -> io::Result<()> {
    let lines = read_lines(&args.input)?;
    print_table(&lines, &args.format_options())
}

// Include tests
#[cfg(test)]
mod tests;
