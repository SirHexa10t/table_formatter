use clap::Parser;
use ordered_float::OrderedFloat;
use rayon::prelude::*;
use regex::Regex;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write as _};
use std::iter::{repeat, repeat_n};
use std::path::Path;
use std::sync::LazyLock;

mod fold;


// ——— Configuration ——————————————————————————————
/// Column delimiters default to two spaces: the input splits on runs of 2+ whitespace,
/// and the output puts two spaces between columns. Both must carry leading and trailing
/// whitespace (see [`validate_delimiter`]), so the default is the tightest legal value.
const DEFAULT_DIVIDE_BY: &str = "  ";
const DEFAULT_JOIN_WITH: &str = "  ";

// ——— Special characters ——————————————————————————————
// The tool's marker vocabulary, kept together so no two roles collide by accident.

/// Written into cells that hold no data (empty or whitespace-only after splitting), so the
/// cell keeps its column through re-splitting and sorting. `-` is in the neutral set, so a
/// filled cell never flips a numeric column to text alignment.
pub(crate) const EMPTY_CELL_FILLER: &str = "-";
/// Fallback filler when `-` appears in a delimiter (a `-` cell would then read as part of
/// the delimiter). `×` (U+00D7) is also in the neutral set.
pub(crate) const EMPTY_CELL_FILLER_FALLBACK: &str = "×";
/// Fold gutter + empty-slot placeholder marker (default; `--sentinel` overrides): a middle
/// dot — low ink, rarely typed in data.
pub(crate) const DEFAULT_SENTINEL: char = '\u{b7}'; // ·
/// Fold mid-word break marker: a typographic hyphen — looks like `-` but is virtually
/// never typed (people type U+002D), so unfold can trust it marks a break, not data.
pub(crate) const BREAK_HYPHEN: char = '\u{2010}'; // ‐

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
#[must_use]
pub fn visible_len(text: &str) -> usize {
    // Printable ASCII is one cell per byte — no escapes, no wide glyphs. Worth a fast
    // path because width measurement runs twice per cell (column sizing + padding).
    if text.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return text.len();
    }
    console::measure_text_width(text)
}

/// ANSI-stripped view of `text`, skipping the escape parser entirely when no ESC byte
/// is present — which is every cell of a plain table.
fn ansi_stripped(text: &str) -> Cow<'_, str> {
    if text.contains('\u{1b}') {
        console::strip_ansi_codes(text)
    } else {
        Cow::Borrowed(text)
    }
}

#[must_use]
pub fn is_numeric_or_neutral(text: &str) -> bool {
    let clean = ansi_stripped(text);
    let clean = clean.trim();
    matches!(clean, "" | "-" | "--" | "---" | "*" | "−" | "=" | "y" | "n" | "?" | "×")
        || NUMERIC_PATTERN.is_match(clean)
}

/// Numeric interpretation of a cell: `Some(magnitude)` for values `NUMERIC_PATTERN`
/// accepts — plain numbers, scaled ones (`3.5K`, `-1.2GiB/s`), percentages, frequencies,
/// resolutions (`1080p`) — and `None` for anything else, neutral markers included.
pub(crate) fn parse_numeric(text: &str) -> Option<f64> {
    let clean = ansi_stripped(text);
    let caps = NUMERIC_PATTERN.captures(clean.trim())?;
    let number: f64 = caps["num"].parse().ok()?;

    let binary = caps.name("unit").is_some_and(|unit| unit.as_str().starts_with('i'));
    let scale_letter = caps
        .name("scale")
        .and_then(|scale| scale.as_str().chars().next())
        .map(|letter| letter.to_ascii_lowercase());

    let scale = match (scale_letter, binary) {
        (None | Some('p'), _) => 1.0, // no scale, or pixels (1080p) — not peta
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

/// Build the column-splitting regex for a `--divide-by` string. A whitespace-only
/// delimiter (like the default `"  "`) splits on a run of that many-or-more whitespace
/// characters, with single tabs always breaking too — the historical behavior, and the
/// clean generalization of the old numeric threshold (`"   "` == old `-t 3`). A delimiter
/// with a visible core (like `" | "`) splits on that core wherever it's flanked by at
/// least one whitespace on each side, so `" | "`, `"  |  "`, and `"\t|\t"` all divide alike.
///
/// The cored form consumes only ONE trailing whitespace: adjacent delimiters around an
/// empty cell (`x |  | y`) must each find a leading whitespace of their own, which a
/// greedy `\s+` tail would have swallowed. Leftover whitespace lands in the next cell and
/// is removed by the per-cell trim in [`split_row`].
///
/// Assumes `divide_by` already passed [`validate_delimiter`] (≥1 leading + trailing ws).
fn split_pattern(divide_by: &str) -> Regex {
    let core = divide_by.trim();
    let pattern = if core.is_empty() {
        let run = divide_by.chars().count().max(2);
        format!(r"\s{{{run},}}|\t+")
    } else {
        format!(r"\s+{}\s", regex::escape(core))
    };
    Regex::new(&pattern).unwrap()
}

/// The default splitter, shared: repeated `format_table` calls at the default delimiter
/// skip the regex compilation, which dwarfs the actual work on small tables.
static DEFAULT_SPLIT_PATTERN: LazyLock<Regex> = LazyLock::new(|| split_pattern(DEFAULT_DIVIDE_BY));

/// Both delimiters must carry at least one leading and one trailing whitespace character,
/// at distinct positions — so `" | "` and the default `"  "` are legal, but `"|"`, `"x"`,
/// a lone `" "`, and `""` are not. On input this stops a single interior space from being
/// read as a column break; on output it keeps the result re-parseable as a table.
fn validate_delimiter(flag: &'static str, value: &str) -> Result<(), FormatError> {
    let leading = value.chars().next().is_some_and(char::is_whitespace);
    let trailing = value.chars().next_back().is_some_and(char::is_whitespace);
    if value.chars().count() >= 2 && leading && trailing {
        Ok(())
    } else {
        Err(FormatError::InvalidDelimiter { flag, value: value.to_string() })
    }
}

/// The filler for no-data cells: [`EMPTY_CELL_FILLER`] (`-`), unless that character
/// appears in either delimiter — then a `-` cell would be ambiguous against the delimiter
/// itself, so [`EMPTY_CELL_FILLER_FALLBACK`] (`×`) steps in.
fn empty_cell_filler(divide_by: &str, join_with: &str) -> &'static str {
    if divide_by.contains(EMPTY_CELL_FILLER) || join_with.contains(EMPTY_CELL_FILLER) {
        EMPTY_CELL_FILLER_FALLBACK
    } else {
        EMPTY_CELL_FILLER
    }
}

/// Split a line into cells, borrowed straight from the input — cells are read-only
/// views until output assembly, so no per-cell copies are made anywhere.
///
/// `border`, when set, is the delimiter's visible core (e.g. `"|"` for `" | "`), used to
/// peel the outer frame of a Markdown-style row `| a | b |` so its pipes don't fuse onto
/// the edge cells. One leading and one trailing core are stripped; the surrounding spaces
/// are left to the split. A line without the frame is untouched.
fn split_row<'a>(line: &'a str, pattern: &Regex, border: Option<(&str, &str)>) -> Vec<&'a str> {
    let mut inner = line.trim();
    if let Some((lead, trail)) = border {
        inner = inner.strip_prefix(lead).unwrap_or(inner);
        inner = inner.strip_suffix(trail).unwrap_or(inner);
    }
    // Trim each cell, not the whole span: trimming the span would swallow an empty edge
    // cell (`|  | a |`) and lose its column. Non-frame cells are already trim-stable.
    pattern.split(inner).map(str::trim).collect()
}

fn detect_column_properties(rows: &[Vec<&str>]) -> (Vec<usize>, Vec<bool>) {
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
        .reduce(fresh, |(mut widths_a, mut numeric_a), (widths_b, numeric_b)| {
            // Merge into the left operand — no fresh allocations per merge.
            for (a, b) in widths_a.iter_mut().zip(&widths_b) {
                *a = (*a).max(*b);
            }
            for (a, b) in numeric_a.iter_mut().zip(&numeric_b) {
                *a = *a && *b;
            }
            (widths_a, numeric_a)
        })
}

fn format_row(
    cells: &[&str],
    widths: &[usize],
    is_numeric: &[bool],
    spacer: &str,
    frame: Option<(&str, &str)>,
) -> String {
    let (lead, trail) = frame.unwrap_or(("", ""));

    // Pre-compute total capacity
    let total = lead.len() + trail.len()
        + widths.iter().sum::<usize>()
        + spacer.len() * widths.len().saturating_sub(1);
    let mut out = String::with_capacity(total);
    out.push_str(lead); // opening frame, if any

    // Bind a single empty cell for all "missing" cells
    let empty = "";

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

    // Trim off the trailing separator. (Trailing-space stripping is NOT done here — it's a
    // shared post-pass in `format_table`, so the folded path gets the identical treatment.)
    out.truncate(out.len().saturating_sub(spacer.len()));
    out.push_str(trail); // closing frame
    out
}

/// Sort data rows in place by column `idx` — descending for numeric columns (biggest
/// first), ascending for text. `header` decides whether row 0 is pinned on top:
/// `Some` overrides, `None` auto-detects — the first row is treated as a header unless
/// its sort cell parses as a number.
fn sort_rows(rows: &mut [Vec<&str>], idx: usize, numeric: bool, header: Option<bool>) {
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
                .map_or_else(String::new, |cell| ansi_stripped(cell).into_owned())
        });
    }
}

// ——— Errors ——————————————————————————————————————————
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// `sort` asked for a column the table doesn't have.
    SortColumnOutOfRange { requested: usize, num_cols: usize },
    /// A `--divide-by` / `--join-with` value lacked leading and trailing whitespace.
    /// `flag` is the offending option name, for a message that points the user at the fix.
    InvalidDelimiter { flag: &'static str, value: String },
    /// Two options were set that can't work together — e.g. `--emit-frame` needs the
    /// trailing padding that `--remove-trailing-spaces` strips, so the frame would go ragged.
    ConflictingOptions { first: &'static str, second: &'static str },
    /// `--fold-row-width` was below the minimum that can hold a gutter and a column
    /// separator (< 2, or narrower than the `--join-with` delimiter).
    FoldWidthTooSmall { width: usize, minimum: usize },
    /// `--sentinel` was not a single, non-whitespace character.
    InvalidSentinel { value: String },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SortColumnOutOfRange { requested, num_cols } => write!(
                f,
                "sort column {requested} is out of range: the table has {num_cols} column(s)"
            ),
            Self::InvalidDelimiter { flag, value } => write!(
                f,
                "{flag} {value:?} must have leading and trailing whitespace (e.g. \" | \")"
            ),
            Self::ConflictingOptions { first, second } => {
                write!(f, "{first} cannot be combined with {second}")
            }
            Self::FoldWidthTooSmall { width, minimum } => write!(
                f,
                "--fold-row-width {width} is too small: it must be at least {minimum}"
            ),
            Self::InvalidSentinel { value } => {
                write!(f, "--sentinel {value:?} must be a single non-whitespace character")
            }
        }
    }
}

impl std::error::Error for FormatError {}

// ——— Formatting options ——————————————————————————————————————
/// How a table gets formatted; `..Default::default()` gives the CLI's defaults.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// String that divides columns in the INPUT. Whitespace runs on each side are
    /// flexible; a visible core (like `|`) divides only when whitespace-flanked. Must
    /// contain leading and trailing whitespace (validated by [`format_table`]).
    pub divide_by: String,
    /// String placed between columns in the OUTPUT. Must contain leading and trailing
    /// whitespace too, so the rendered table can be re-parsed as input.
    pub join_with: String,
    /// Sort by this 0-based column: descending for numeric columns, ascending for text.
    pub sort: Option<usize>,
    /// `Some(true)`: the first row is a header and stays on top when sorting.
    /// `Some(false)`: the first row is data like any other.
    /// `None`: auto-detect — pinned unless its sort cell parses as a number.
    pub header: Option<bool>,
    /// Strip the trailing padding spaces from each output line.
    pub trim_trailing: bool,
    /// Wrap each output line in the `join_with` delimiter's edge characters, turning the
    /// output into a framed (Markdown-style) table — `| … |` for `join_with = " | "`. A
    /// whitespace-only `join_with` has no edge characters, so this is then a no-op.
    pub emit_frame: bool,
    /// Wrap the table to at most this many visible columns per line: cells word-wrap within
    /// per-column caps and stack, continuation lines marked in a one-column gutter. `None`
    /// leaves lines full-width.
    pub fold_row_width: Option<usize>,
    /// Reverse a `fold_row_width` wrap: collapse the table back to one line per record
    /// (recover columns, drop placeholders, rejoin fragments) before formatting.
    pub unfold: bool,
    /// Marker character in the fold gutter and empty-cell placeholders (default `·`). Must
    /// be non-whitespace. The **same** value has to be given to fold and to `unfold`.
    pub sentinel: char,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            divide_by: DEFAULT_DIVIDE_BY.to_string(),
            join_with: DEFAULT_JOIN_WITH.to_string(),
            sort: None,
            header: None,
            trim_trailing: false,
            emit_frame: false,
            fold_row_width: None,
            unfold: false,
            sentinel: DEFAULT_SENTINEL,
        }
    }
}

// ——— Core formatting functions ——————————————————————————————————
/// Format `lines` into an aligned table (one output line per input line).
/// Accepts any string-ish slice — `&[String]`, `&[&str]`, `&[Box<str>]`, …
///
/// # Errors
/// [`FormatError::SortColumnOutOfRange`] when `opts.sort` names a missing column,
/// [`FormatError::InvalidDelimiter`] when `divide_by`/`join_with` lack leading and
/// trailing whitespace, or [`FormatError::ConflictingOptions`] when incompatible options
/// are combined (`emit_frame` with `trim_trailing`, or `emit_frame` with `fold_row_width`).
#[must_use = "formatting allocates the whole table; ignoring it wastes the work"]
pub fn format_table<S: AsRef<str> + Sync>(
    lines: &[S],
    opts: &FormatOptions,
) -> Result<Vec<String>, FormatError> {
    // Reject bad option combinations up front — one guarantee for the library and the CLI.
    validate_delimiter("--divide-by", &opts.divide_by)?;
    validate_delimiter("--join-with", &opts.join_with)?;
    // A frame needs the trailing padding to keep its right border aligned; stripping that
    // padding would leave the frame ragged, so the two can't be combined.
    if opts.emit_frame && opts.trim_trailing {
        return Err(FormatError::ConflictingOptions {
            first: "--emit-frame",
            second: "--remove-trailing-spaces",
        });
    }
    // Folding breaks a line into pieces, so a per-record frame can't stay intact across them.
    if opts.emit_frame && opts.fold_row_width.is_some() {
        return Err(FormatError::ConflictingOptions {
            first: "--emit-frame",
            second: "--fold-row-width",
        });
    }
    // The sentinel marks the fold gutter and placeholders, so it must be visible.
    if opts.sentinel.is_whitespace() {
        return Err(FormatError::InvalidSentinel { value: opts.sentinel.to_string() });
    }
    // A fold width narrower than a gutter + the output column separator can't lay out a
    // row — and the output columns are joined with `join_with`, so that's the floor.
    if let Some(width) = opts.fold_row_width {
        let minimum = visible_len(&opts.join_with).max(2);
        if width < minimum {
            return Err(FormatError::FoldWidthTooSmall { width, minimum });
        }
    }

    // --unfold: collapse a table wrapped by --fold-row-width back to one line per record
    // (recover columns, drop `·` placeholders, rejoin fragments), then format it normally.
    // The output columns are separated by `join_with`; re-emit cells joined by `divide_by`
    // so the recursive format re-parses them. The inverse of --fold-row-width.
    if opts.unfold {
        let separator = split_pattern(&opts.join_with);
        let collapsed = fold::unfold(lines, &separator, &opts.divide_by, opts.sentinel);
        let reflow = FormatOptions { unfold: false, ..opts.clone() };
        return format_table(&collapsed, &reflow);
    }

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    // Split every line into its cells, in parallel — lines are independent. The default
    // delimiter reuses the cached splitter; a custom one compiles its own.
    let custom_pattern; // keeps a non-default splitter alive for the borrow below
    let pattern = if opts.divide_by == DEFAULT_DIVIDE_BY {
        &*DEFAULT_SPLIT_PATTERN
    } else {
        custom_pattern = split_pattern(&opts.divide_by);
        &custom_pattern
    };
    // A delimiter with a visible core (`" | "`) frames Markdown-style rows as `| … |`;
    // peel that outer frame (one leading + one trailing core pipe) so it doesn't fuse onto
    // the edge cells. A whitespace-only delimiter has no frame, so those rows are untouched.
    let border = (!opts.divide_by.trim().is_empty()).then(|| {
        let core = opts.divide_by.trim();
        (core, core)
    });
    // No-data cells (empty or whitespace-only) get a filler mark right at the split, so
    // every later stage — width detection, sorting, folding, output — sees a real cell
    // that survives re-splitting. Filled here once; `fold` relies on it. Emptiness is
    // judged on the *visible* text: a cell of pure ANSI codes is just as data-free, and
    // styling must never change layout.
    let filler = empty_cell_filler(&opts.divide_by, &opts.join_with);
    let is_no_data = |cell: &str| {
        cell.is_empty() || (cell.contains('\u{1b}') && ansi_stripped(cell).trim().is_empty())
    };
    let mut rows: Vec<Vec<&str>> = lines
        .par_iter()
        .map(|line| {
            let mut cells = split_row(line.as_ref(), pattern, border);
            for cell in &mut cells {
                if is_no_data(cell) {
                    *cell = filler;
                }
            }
            cells
        })
        .collect();
    let (widths, is_numeric) = detect_column_properties(&rows);

    // sort, if asked to
    if let Some(idx) = opts.sort {
        if idx >= widths.len() {
            return Err(FormatError::SortColumnOutOfRange { requested: idx, num_cols: widths.len() });
        }
        sort_rows(&mut rows, idx, is_numeric[idx], opts.header);
    }

    // Aligned wrapping: with a width budget, word-wrap cells within per-column caps and
    // stack them, instead of the one-line-per-record path below. (Framing is disallowed
    // alongside it, so there's no frame to reconcile here.)
    let mut table = if let Some(budget) = opts.fold_row_width {
        fold::render_wrapped(&rows, &widths, &is_numeric, &opts.join_with, budget, opts.sentinel)
    } else {
        // Optional output frame: re-add the join delimiter's edge halves around each line,
        // so a table joined with `" | "` reads back as `| … |`. Mirror image of `border`
        // above — and, with a matching `divide_by`, its exact inverse, so framed tables
        // round-trip.
        let frame = (opts.emit_frame && !opts.join_with.trim().is_empty())
            .then(|| (opts.join_with.trim_start(), opts.join_with.trim_end()));

        // Format rows (the main feature; handle the spacing)
        rows.par_iter()
            .map(|row| format_row(row, &widths, &is_numeric, &opts.join_with, frame))
            .collect()
    };

    // One shared trailing-space pass for both paths (framing is trim-incompatible and was
    // rejected above, so nothing here ever trims inside a frame).
    if opts.trim_trailing {
        for line in &mut table {
            line.truncate(line.trim_end().len());
        }
    }
    Ok(table)
}

fn print_table<S: AsRef<str> + Sync>(lines: &[S], opts: &FormatOptions) -> io::Result<()> {
    let table = format_table(lines, opts)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    // One locked, buffered writer for the whole table: per-line println! would take the
    // stdout lock and issue a write syscall for every line.
    let mut out = BufWriter::new(io::stdout().lock());
    for line in &table {
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()
}

// ——— CLI Options ——————————————————————————————————————
#[derive(Parser)]
#[command(version, about = "Align whitespace-delimited columns into a neat table")]
pub struct Args {
    /// Input file path / data (or use stdin if not provided)
    #[arg(default_value = "-")]
    input: String,

    /// String that divides columns in the input; needs leading + trailing whitespace, so
    /// " | " is a valid pipe delimiter but "|" is not. Whitespace runs are flexible.
    #[arg(short = 'd', long, default_value = DEFAULT_DIVIDE_BY)]
    divide_by: String,

    /// String placed between columns in the output; needs leading + trailing whitespace
    /// too (so the result stays re-parseable), e.g. " | ".
    #[arg(short = 'j', long, default_value = DEFAULT_JOIN_WITH)]
    join_with: String,

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

    /// Wrap each output line in the --join-with edge characters, e.g. "| … |" for
    /// --join-with " | " — emitting a framed (Markdown-style) table
    #[arg(long)]
    emit_frame: bool,

    /// Wrap the table to at most N visible columns per line: cells word-wrap within
    /// per-column widths and stack, continuations marked with · in a one-column left gutter
    #[arg(long, value_name = "N")]
    fold_row_width: Option<usize>,

    /// Reverse --fold-row-width: collapse a wrapped table back to one line per record
    /// (recover columns, drop placeholders, rejoin) — use the same -d/-j you folded with
    #[arg(long)]
    unfold: bool,

    /// Marker character for the fold gutter and empty-cell placeholders [default: ·]. Must
    /// be non-whitespace; pass the SAME one to --fold-row-width and --unfold
    #[arg(long, value_name = "CHAR", default_value_t = DEFAULT_SENTINEL)]
    sentinel: char,
}

impl Args {
    fn format_options(&self) -> FormatOptions {
        FormatOptions {
            divide_by: self.divide_by.clone(),
            join_with: self.join_with.clone(),
            sort: self.sort,
            header: match (self.header, self.no_header) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
            trim_trailing: self.remove_trailing_spaces,
            emit_frame: self.emit_frame,
            fold_row_width: self.fold_row_width,
            unfold: self.unfold,
            sentinel: self.sentinel,
        }
    }
}

// ——— Library entry points ——————————————————————————————————————
/// Run exactly as the `table_formatter` binary does, reading arguments from the
/// process environment.
///
/// # Errors
/// I/O errors from reading the input or writing stdout; an invalid `--sort`
/// column surfaces as [`io::ErrorKind::InvalidInput`].
pub fn run() -> io::Result<()> {
    run_from(std::env::args_os())
}

/// Run with an explicit argument list (argv[0] should be the program name).
/// This lets another program invoke `table_formatter` in-process, as if it had
/// executed the binary with those arguments.
///
/// # Errors
/// Same as [`run`].
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
///
/// # Errors
/// Any I/O error from reading stdin or opening/reading the file.
pub fn read_lines(input: &str) -> io::Result<Vec<String>> {
    Ok(read_input(input)?.lines().map(String::from).collect())
}

/// Read a command's whole input as one string, routed exactly like [`read_lines`]:
/// `"-"` / empty reads stdin, an existing file path reads that file, anything else is
/// inline data. Callers can then borrow line slices instead of owning each line.
fn read_input(input: &str) -> io::Result<String> {
    if input == "-" || input.is_empty() {
        return read_to_string_lossy(io::stdin().lock());
    }
    if Path::new(input).is_file() {
        return read_to_string_lossy(File::open(input)?);
    }
    Ok(input.to_string())
}

/// Collect a reader's contents, decoding UTF-8 lossily so stray bytes don't abort.
/// Valid UTF-8 converts in place — no second copy.
fn read_to_string_lossy<R: Read>(mut reader: R) -> io::Result<String> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(String::from_utf8(buf)
        .unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned()))
}

/// Collect a reader's contents as lines, decoding UTF-8 lossily so stray bytes don't abort.
/// (Kept for the test suite's reader-path coverage; production goes through `read_input`.)
#[cfg(test)]
pub(crate) fn read_from<R: Read>(reader: R) -> io::Result<Vec<String>> {
    Ok(read_to_string_lossy(reader)?.lines().map(String::from).collect())
}

/// Run with an already-parsed [`Args`]. This lets a dependent crate embed [`Args`]
/// directly in its own clap CLI (e.g. as a `Subcommand` variant) and hand it
/// straight here — so the argument definitions live only in this crate.
///
/// # Errors
/// Same as [`run`].
pub fn run_with(args: Args) -> io::Result<()> {
    // One buffer holds the whole input; rows borrow from it — no per-line copies.
    let text = read_input(&args.input)?;
    let lines: Vec<&str> = text.lines().collect();
    print_table(&lines, &args.format_options())
}

// Include tests
#[cfg(test)]
mod tests;
#[cfg(test)]
mod fold_tests;
