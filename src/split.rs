//! Aligned line wrapping (`--split-until-width` / `--unsplit`).
//!
//! When a table is wider than a `--split-until-width` budget, [`render_wrapped`] chooses a
//! width cap per column (a deterministic breakpoint-walking greedy — see [`allocate_caps`] —
//! that keeps words whole for as long as words can fit), word-wraps each cell into
//! fragments within its cap, and stacks a record's fragments into aligned visual lines. Every line carries a one-column left
//! **gutter**: a space on a record's first line, the sentinel (default [`DEFAULT_SENTINEL`])
//! on continuations. Empty slots get a lone sentinel so the column survives re-splitting,
//! and a word forced to split mid-way gets a [`BREAK_HYPHEN`] so the break is visible and lossless.
//!
//! [`unsplit`] reverses it, recovering columns and rejoining fragments. Padding is spaces;
//! the sentinel appears only in the gutter and empty slots. The same sentinel must be given
//! to both directions.

// The marker characters (sentinel, break hyphen) live with the rest of the tool's special
// characters in lib.rs — this module only consumes them. The sentinel is passed explicitly
// through both directions so split and unsplit can't drift apart.
use crate::{visible_len, Divider, BREAK_HYPHEN};
use std::iter::repeat_n;

/// Gutter on a record's first visual line.
const HEAD: char = ' ';

// ——— ANSI-aware visible-width slicing ————————————————————————————————————

/// Visible width of a single char. ANSI escapes are handled by the scanners, so this only
/// sees printable text; ASCII is one column, wider glyphs defer to `console`.
fn char_width(ch: char) -> usize {
    if ch.is_ascii() {
        return 1;
    }
    let mut buf = [0u8; 4];
    console::measure_text_width(ch.encode_utf8(&mut buf))
}

/// Scanner state: plain text, just-saw-ESC, or inside a CSI sequence.
enum Scan {
    Text,
    AfterEsc,
    InCsi,
}

/// Break `s` into pieces of at most `budget` visible columns, cutting only at char
/// boundaries and never inside an ANSI escape. Used to hard-break a single word that is
/// itself wider than its column cap. Concatenating the pieces reproduces `s` exactly.
fn slice_visible(s: &str, budget: usize) -> Vec<&str> {
    let budget = budget.max(1);
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut width = 0;
    let mut scan = Scan::Text;
    for (i, ch) in s.char_indices() {
        match scan {
            Scan::AfterEsc => scan = if ch == '[' { Scan::InCsi } else { Scan::Text },
            Scan::InCsi => {
                if ('@'..='~').contains(&ch) {
                    scan = Scan::Text;
                }
            }
            Scan::Text => {
                if ch == '\u{1b}' {
                    scan = Scan::AfterEsc;
                    continue;
                }
                let w = char_width(ch);
                if width + w > budget && i > start {
                    pieces.push(&s[start..i]);
                    start = i;
                    width = 0;
                }
                width += w;
            }
        }
    }
    pieces.push(&s[start..]);
    pieces
}

// ——— Word wrapping ————————————————————————————————————————————————————————

/// Byte ranges of maximal non-space runs ("words") in `s`. ANSI escapes contain no spaces,
/// so they ride along with the adjacent word and contribute no visible width.
fn word_ranges(s: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, ch) in s.char_indices() {
        if ch == ' ' {
            if let Some(begin) = start.take() {
                runs.push((begin, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(begin) = start {
        runs.push((begin, s.len()));
    }
    runs
}

/// A wrapped piece of a cell: the borrowed text, plus whether a hyphen must follow it
/// because a word was split mid-way here (so unsplit rejoins the pieces with no space).
struct Fragment<'a> {
    text: &'a str,
    hyphen: bool,
}

/// Word-wrap `cell` into fragments, each rendering to at most `cap` visible columns. Breaks
/// at spaces; a single word wider than `cap` is split mid-word, with a hyphen on every piece
/// but the last (so the split is visible and losslessly reversible). Break spaces are
/// dropped. An empty cell yields one empty fragment, so it still occupies its column slot.
fn wrap_cell(cell: &str, cap: usize) -> Vec<Fragment<'_>> {
    let cap = cap.max(1);
    let words = word_ranges(cell);
    if words.is_empty() {
        return vec![Fragment { text: "", hyphen: false }];
    }

    let mut frags = Vec::new();
    let (mut frag_start, mut frag_end) = words[0];
    for &(word_start, word_end) in &words[1..] {
        if visible_len(&cell[frag_start..word_end]) <= cap {
            frag_end = word_end; // this word still fits on the current fragment
        } else {
            push_fragment(&mut frags, &cell[frag_start..frag_end], cap);
            frag_start = word_start;
            frag_end = word_end;
        }
    }
    push_fragment(&mut frags, &cell[frag_start..frag_end], cap);
    frags
}

/// Emit `word` as one fragment, or — if it's a single word wider than `cap` — split it,
/// reserving a column for the hyphen so each piece still renders within `cap`.
fn push_fragment<'a>(frags: &mut Vec<Fragment<'a>>, word: &'a str, cap: usize) {
    if visible_len(word) <= cap {
        frags.push(Fragment { text: word, hyphen: false });
        return;
    }
    let piece_cap = if cap >= 2 { cap - 1 } else { cap }; // leave a column for the hyphen
    let pieces = slice_visible(word, piece_cap);
    let last = pieces.len() - 1;
    for (i, piece) in pieces.into_iter().enumerate() {
        frags.push(Fragment { text: piece, hyphen: i != last && cap >= 2 });
    }
}

// Column-width allocation (the breakpoint-walking greedy + hyphenation fallback +
// slack relaxation) lives in its own submodule, with its own tests.
mod alloc;
use alloc::allocate_caps;

// ——— Rendering ————————————————————————————————————————————————————————————

fn pad(out: &mut String, text: &str, width: usize, right_align: bool) {
    let gap = width.saturating_sub(visible_len(text));
    if right_align {
        out.extend(repeat_n(' ', gap));
        out.push_str(text);
    } else {
        out.push_str(text);
        out.extend(repeat_n(' ', gap));
    }
}

/// An empty continuation slot: a lone `sentinel` (so the column survives re-splitting)
/// padded to the column's cap.
fn placeholder(out: &mut String, width: usize, right_align: bool, sentinel: char) {
    let gap = width.saturating_sub(1);
    if right_align {
        out.extend(repeat_n(' ', gap));
        out.push(sentinel);
    } else {
        out.push(sentinel);
        out.extend(repeat_n(' ', gap));
    }
}

/// Render `rows` as an aligned, wrapped table fitting `budget` visible columns per line.
/// `natural` and `is_numeric` come from the caller's column detection; `join` is the
/// between-column string; `sentinel` marks continuation gutters and empty slots. Returns one
/// string per visual line (a record spans as many lines as its tallest wrapped cell).
pub(crate) fn render_wrapped(
    rows: &[Vec<&str>],
    natural: &[usize],
    is_numeric: &[bool],
    join: &str,
    budget: usize,
    sentinel: char,
) -> Vec<String> {
    let cols = natural.len();
    if cols == 0 {
        return rows.iter().map(|_| String::new()).collect();
    }
    // Budget for content = line budget minus the gutter and the inter-column joins.
    let overhead = 1 + visible_len(join) * cols.saturating_sub(1);
    let content_budget = budget.saturating_sub(overhead).max(cols);
    let caps = allocate_caps(rows, natural, content_budget);

    let mut out = Vec::new();
    for row in rows {
        let frags: Vec<Vec<Fragment>> =
            (0..cols).map(|c| wrap_cell(row.get(c).copied().unwrap_or(""), caps[c])).collect();
        let height = frags.iter().map(Vec::len).max().unwrap_or(1);
        for k in 0..height {
            let mut line = String::new();
            line.push(if k == 0 { HEAD } else { sentinel });
            for c in 0..cols {
                if c > 0 {
                    line.push_str(join);
                }
                match frags[c].get(k) {
                    Some(f) if f.hyphen => {
                        pad(&mut line, &format!("{}{BREAK_HYPHEN}", f.text), caps[c], is_numeric[c]);
                    }
                    Some(f) if !f.text.is_empty() => pad(&mut line, f.text, caps[c], is_numeric[c]),
                    // an empty or missing slot → a lone sentinel, so every column carries
                    // something and its position survives unsplit's re-split
                    _ => placeholder(&mut line, caps[c], is_numeric[c], sentinel),
                }
            }
            out.push(line);
        }
    }
    out
}

// ——— Reverse (unsplit) —————————————————————————————————————————————————————

/// Reverse of [`render_wrapped`]: collapse a wrapped table back to one line per record.
///
/// Groups visual lines by the gutter (space = new record, `sentinel` = continuation),
/// splits each de-guttered line into column fragments with `separator` (the pattern the
/// output columns are joined by), drops the `sentinel` placeholders, rejoins each column's
/// fragments (see [`rejoin_fragments`]), and re-emits the record's cells joined by `rejoin`
/// (the input delimiter, so the caller can re-parse). `sentinel` **must** match the one used
/// to split — otherwise the gutters and placeholders aren't recognized and the result is
/// garbled. Reversible up to whitespace normalization at wrap points.
pub(crate) fn unsplit<S: AsRef<str>>(
    lines: &[S],
    separator: &Divider,
    rejoin: &str,
    sentinel: char,
) -> Vec<String> {
    let mut sentinel_buf = [0u8; 4];
    let sentinel_str = &*sentinel.encode_utf8(&mut sentinel_buf);
    let mut out = Vec::new();
    let mut cols: Vec<Vec<&str>> = Vec::new();
    let mut open = false;

    for line in lines {
        let line = line.as_ref();
        let mut chars = line.chars();
        let gutter = chars.next();
        let frags: Vec<&str> = separator.split(chars.as_str().trim());

        if gutter == Some(sentinel) && open {
            for (c, &frag) in frags.iter().enumerate() {
                if c == cols.len() {
                    cols.push(Vec::new());
                }
                if frag != sentinel_str && !frag.is_empty() {
                    cols[c].push(frag);
                }
            }
        } else {
            if open {
                out.push(collapse_record(&cols, rejoin));
            }
            cols = frags
                .iter()
                .map(|&frag| if frag == sentinel_str || frag.is_empty() { Vec::new() } else { vec![frag] })
                .collect();
            open = true;
        }
    }
    if open {
        out.push(collapse_record(&cols, rejoin));
    }
    out
}

/// Rejoin one record's per-column fragments into a single line: each cell is its fragments
/// rejoined (see [`rejoin_fragments`]), then the cells are joined by `rejoin`. When `rejoin`
/// has a visible core (`" | "`), the line is re-wrapped in the frame so an empty *edge* cell
/// survives the re-parse — without the outer pipe, a leading `" | "` would read as the frame.
fn collapse_record(cols: &[Vec<&str>], rejoin: &str) -> String {
    let body = cols.iter().map(|frags| rejoin_fragments(frags)).collect::<Vec<_>>().join(rejoin);
    if rejoin.trim().is_empty() {
        body
    } else {
        format!("{}{body}{}", rejoin.trim_start(), rejoin.trim_end())
    }
}

/// Rejoin one column's fragments back into a cell. A fragment ending in the hyphen marker
/// was split mid-word, so the next piece follows with no space (and the marker is dropped);
/// any other fragment is a whole word, so a single space precedes the next one. The exact
/// inverse of the wrapping in [`wrap_cell`]/[`push_fragment`].
fn rejoin_fragments(frags: &[&str]) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for frag in frags {
        if pending_space {
            out.push(' ');
        }
        match frag.strip_suffix(BREAK_HYPHEN) {
            Some(head) => {
                out.push_str(head);
                pending_space = false; // the word continues on the next fragment
            }
            None => {
                out.push_str(frag);
                pending_space = true;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_SENTINEL;

    fn vis(lines: &[String]) -> Vec<usize> {
        lines.iter().map(|l| visible_len(l)).collect()
    }

    /// (text, hyphen) shape of wrapped fragments, for terse assertions.
    fn shape<'a>(frags: Vec<Fragment<'a>>) -> Vec<(&'a str, bool)> {
        frags.iter().map(|f| (f.text, f.hyphen)).collect()
    }

    #[test]
    fn wrap_cell_breaks_at_spaces() {
        assert_eq!(shape(wrap_cell("the quick brown fox", 9)), vec![("the quick", false), ("brown fox", false)]);
        assert_eq!(shape(wrap_cell("one two three", 100)), vec![("one two three", false)]);
        assert_eq!(shape(wrap_cell("", 5)), vec![("", false)]);
    }

    #[test]
    fn wrap_cell_hyphenates_a_hard_broken_word() {
        // cap 6 leaves 5 columns for text + 1 for the hyphen; every piece but the last is hyphenated
        assert_eq!(
            shape(wrap_cell("supercalifragilistic", 6)),
            vec![("super", true), ("calif", true), ("ragil", true), ("istic", false)]
        );
    }

    #[test]
    fn wrapped_fragments_are_compact() {
        // "recollect" correctness: words never flow to the next fragment while there is
        // still room — no fragment's first word could have fit on the previous line
        for (cell, cap) in [
            ("a fairly long detail that must wrap", 10),
            ("one two three four five", 7),
            ("cccc dd", 4),
            ("supercalifragilistic", 6), // hyphen-broken pieces are maximal too
        ] {
            let frags = wrap_cell(cell, cap);
            for pair in frags.windows(2) {
                let first_word = pair[1].text.split(' ').next().unwrap();
                assert!(
                    visible_len(pair[0].text) + 1 + visible_len(first_word) > cap,
                    "{cell:?} at cap {cap}: {:?} still had room for {:?}",
                    pair[0].text,
                    first_word
                );
            }
        }
    }

    #[test]
    fn wrap_cell_keeps_ansi_zero_width() {
        let cell = "\u{1b}[31mred\u{1b}[0m word"; // visible "red word" = 8 cols
        assert_eq!(shape(wrap_cell(cell, 4)), vec![("\u{1b}[31mred\u{1b}[0m", false), ("word", false)]);
    }

    #[test]
    fn render_fits_the_budget_and_aligns_columns() {
        let rows = vec![
            vec!["name", "detail"],
            vec!["foo", "a fairly long detail that must wrap"],
            vec!["bar", "short"],
        ];
        let widths = vec![4, 35];
        let numeric = vec![false, false];
        let out = render_wrapped(&rows, &widths, &numeric, "  ", 20, DEFAULT_SENTINEL);
        for (line, w) in out.iter().zip(vis(&out)) {
            assert!(w <= 20, "{line:?} is {w} cols");
        }
        // more visual lines than records (something wrapped)
        assert!(out.len() > rows.len());
        // continuation lines start with the marker, heads with a space
        assert!(out.iter().any(|l| l.starts_with(DEFAULT_SENTINEL)));
        assert!(out[0].starts_with(HEAD));
    }

    #[test]
    fn empty_continuation_slots_get_a_placeholder() {
        // col 1 wraps to 2 lines; col 0 is empty on the continuation → a lone marker
        let rows = vec![vec!["a", "one two"]];
        let out = render_wrapped(&rows, &[1, 3], &[false, false], "  ", 8, DEFAULT_SENTINEL);
        assert_eq!(out.len(), 2);
        assert!(out[1].contains(DEFAULT_SENTINEL)); // gutter + placeholder both present
    }

    #[test]
    fn render_without_overflow_just_adds_a_gutter() {
        // budget far larger than the table: no wrapping, each line just gains a gutter space
        let rows = vec![vec!["a", "bb"], vec!["cc", "d"]];
        let out = render_wrapped(&rows, &[2, 2], &[false, false], "  ", 100, DEFAULT_SENTINEL);
        assert_eq!(out, vec![" a   bb".to_string(), " cc  d ".to_string()]);
    }

    #[test]
    fn hard_break_shows_a_hyphen_and_unsplits_byte_exact() {
        let sep = Divider::new("  ");
        let rows = vec![vec!["x", "antidisestablishmentarianism"]];
        let wrapped = render_wrapped(&rows, &[1, 28], &[false, false], "  ", 14, DEFAULT_SENTINEL);
        // the split word is hyphenated, and no visual line exceeds the budget
        assert!(wrapped.iter().any(|l| l.contains(BREAK_HYPHEN)), "no hyphen at the break");
        for l in &wrapped {
            assert!(visible_len(l) <= 14, "{l:?} overflows");
        }
        // unsplit rebuilds the word exactly — the hyphen is dropped, no space inserted
        assert_eq!(unsplit(&wrapped, &sep, "  ", DEFAULT_SENTINEL), vec!["x  antidisestablishmentarianism".to_string()]);
    }

    #[test]
    fn unsplit_drops_placeholders_and_rejoins_fragments() {
        let sep = Divider::new("  ");
        let wrapped = vec![
            " a    one  x".to_string(),                   // head: a | one | x
            format!("{DEFAULT_SENTINEL}{DEFAULT_SENTINEL}    two  {DEFAULT_SENTINEL}"), // cont: · | two | ·
        ];
        assert_eq!(unsplit(&wrapped, &sep, "  ", DEFAULT_SENTINEL), vec!["a  one two  x".to_string()]);
    }

    #[test]
    fn render_then_unsplit_round_trips() {
        let sep = Divider::new("  ");
        let rows = vec![
            vec!["name", "detail"],
            vec!["foo", "a fairly long detail that must wrap"],
            vec!["bar", "short"],
        ];
        let wrapped = render_wrapped(&rows, &[4, 35], &[false, false], "  ", 20, DEFAULT_SENTINEL);
        assert_eq!(
            unsplit(&wrapped, &sep, "  ", DEFAULT_SENTINEL),
            vec![
                "name  detail".to_string(),
                "foo  a fairly long detail that must wrap".to_string(),
                "bar  short".to_string(),
            ]
        );
    }
}
