use clap::Parser;
use rayon::prelude::*;
use regex::Regex;
use std::io::{self, BufRead, BufReader, Read};
use std::fs::File;
use std::sync::LazyLock;
use itertools::izip;
use std::fmt::Write;
use std::iter::repeat;
use ordered_float::OrderedFloat;


// ——— Configuration ——————————————————————————————
const DEFAULT_SEPARATOR: usize = 2;

// Regular expression patterns
static SPLIT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}|\t+").unwrap());
static NUMERIC_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[+-]?[0-9]+(?:\.[0-9]+)?\s?[pKkMmGgTt]?(?:i?[bB]?(/s)?|%|Hz|@[0-9]+Hz)?$").unwrap()
});

// ——— Utilities ——————————————————————————————————————
pub fn strip_ansi(text: &str) -> String {
    console::strip_ansi_codes(text).to_string()
}

pub fn visible_len(text: &str) -> usize {
    console::measure_text_width(&console::strip_ansi_codes(text))
}

pub fn is_numeric_or_neutral(text: &str) -> bool {
    let clean = strip_ansi(text);
    let clean = clean.trim();
    matches!(clean, "" | "-" | "--" | "---" | "*" | "−" | "=" | "y" | "n" | "?")
        || NUMERIC_PATTERN.is_match(clean)
}


fn evaluate_numeric_item(s: &str) -> f64 {
    let s = s.trim();

    // first, try plain float
    if let Ok(val) = s.parse::<f64>() { return val; }

    // Regex: optional sign, digits, optional fractional
    let re = Regex::new(r"^[-+]?\d+(\.\d+)?").unwrap();
    if let Some(mat) = re.find(s) {
        let num_str = mat.as_str();
        let mut value = num_str.parse::<f64>().unwrap_or(0.0);

        let rest = s[mat.end()..].trim().to_ascii_lowercase();

        // Multipliers: binary first, then SI
        let multipliers: &[(&str, f64)] = &[
            ("ki", 1024.0), ("mi", 1024.0_f64.powi(2)), ("gi", 1024.0_f64.powi(3)),
            ("ti", 1024.0_f64.powi(4)), ("pi", 1024.0_f64.powi(5)), ("ei", 1024.0_f64.powi(6)),
            ("zi", 1024.0_f64.powi(7)), ("yi", 1024.0_f64.powi(8)),

            ("k", 1e3), ("m", 1e6), ("g", 1e9),
            ("t", 1e12), ("p", 1e15), ("e", 1e18),
            ("z", 1e21), ("y", 1e24),
        ];

        for (prefix, mult) in multipliers {
            if rest.starts_with(prefix) {
                value *= mult;
                break;
            }
        }

        return value;
    }

    0.0
}

fn split_row(line: &str) -> Vec<String> {
    SPLIT_PATTERN.split(line.trim()).map(String::from).collect()
}

fn detect_column_properties(rows: &[Vec<String>]) -> (Vec<usize>, Vec<bool>) {
    let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);

    // Transpose table: convert rows to columns
    let mut columns = vec![vec![]; num_cols];
    for (col_idx, cell) in rows.iter().flat_map(|row| row.iter().enumerate()) {
        columns[col_idx].push(cell);
    }

    // Return calculated widths and numeric-flags
    (0..num_cols).into_par_iter()
        .map(|col_idx| {
            let col = &columns[col_idx];
            let width = col.par_iter().map(|cell| visible_len(cell)).max().unwrap_or(0);
            let is_numeric = col.par_iter().skip(1).all(|cell| is_numeric_or_neutral(cell));
            (width, is_numeric)
        })
        .unzip()
}

fn format_row(cells: &[String], widths: &[usize], is_numeric: &[bool], sep_width: usize, ) -> String {
    // Pre-compute total capacity
    let total = widths.iter().sum::<usize>()
        + sep_width * widths.len().saturating_sub(1);
    let mut out = String::with_capacity(total);
    let spacer = " ".repeat(sep_width);

    // Bind a single empty String for all "missing" cells
    let empty = String::new();

    // Zip widths, flags, and cells (falling back to &empty)
    for (&width, &numeric, cell) in izip!(
        widths.iter(),
        is_numeric.iter(),
        cells.iter().chain(repeat(&empty))
    ) {
        if numeric { write!(out, "{:>width$}", cell, width = width).unwrap(); }
        else { write!(out, "{:<width$}", cell, width = width).unwrap(); }
        out.push_str(&spacer);
    }

    // Trim off the trailing separator
    out.truncate(out.len().saturating_sub(sep_width));
    out
}

// ——— Core formatting functions ——————————————————————————————————
pub fn format_table(lines: &[String], separator: usize, col_idx: Option<usize>) -> Vec<String> {
    // Split rows - always use par_iter, rayon will handle the parallelization decision
    let mut rows: Vec<Vec<String>> = lines.par_iter().map(|line| split_row(line)).collect();
    let (widths, is_numeric) = detect_column_properties(&rows);

    // sort, if asked to
    if let Some(idx) = col_idx {
        // if the first row has an actual number in that index, include it in the sort
        let sorting_first_row_too = !rows.is_empty() && evaluate_numeric_item(&rows[0][idx]) != 0.0;
        let header = if !sorting_first_row_too { rows.remove(0) } else { vec![] };

        if is_numeric[idx] {
            rows.sort_by_key(|row| {
                OrderedFloat(row.get(idx).map(|s| evaluate_numeric_item(s)).unwrap_or(0.0))
            });
            rows.reverse();  // make biggest numbers appear at the top
        } else {rows.sort_by_key(|row| { row.get(idx).cloned().unwrap_or_default() }); }
        if !sorting_first_row_too { rows.insert(0, header); }  // restore header post-sort
    }

    // Format rows (the main feature; handle the spacing)
    rows.par_iter()
        .map(|row| format_row(row, &widths, &is_numeric, separator))
        .collect()
}

fn print_table(lines: &[String], separator: usize, col_idx: Option<usize>) {
    format_table(lines, separator, col_idx)
        .iter()
        .for_each(|line| println!("{line}"));
}

// ——— CLI Options ——————————————————————————————————————
#[derive(Parser)]
#[command(author, version, about = "Align whitespace-delimited columns into a neat table")]
struct Args {
    /// Input file path / data (or use stdin if not provided)
    #[arg(default_value = "-")]
    input: String,

    /// Number of spaces to separate columns
    #[arg(short, long, default_value_t = DEFAULT_SEPARATOR)]
    separator: usize,

    /// Sort by column index (0-based), Header row is kept on top.
    #[arg(long)]
    sort: Option<usize>,
}

// ——— Main Function ——————————————————————————————————————
fn main() -> io::Result<()> {
    let args = Args::parse();

    // get the data from input (file / arg-str / stdin)
    let lines: Vec<String> = if args.input == "-" {
        io::stdin().lock().lines().collect::<Result<_, _>>()?
    } else if args.input.contains('\n') {
        // multiline string provided directly → treat as raw data rather than filepath
        args.input.lines().map(|s| s.to_string()).collect()
    } else {
        // BufReader::new(File::open(&args.input)?).lines().collect::<Result<_, _>>()?  // assumes utf8 input
        let mut buf = Vec::new();
        BufReader::new(File::open(args.input).unwrap()).read_to_end(&mut buf)?;
        String::from_utf8_lossy(&buf)  // replaces invalid utf8 with '�'
            .lines()
            .map(|s| s.to_string())
            .collect()
    };

    print_table(&lines, args.separator, args.sort);
    Ok(())
}

// Include tests
#[cfg(test)]
mod tests;
