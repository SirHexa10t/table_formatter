//! Tests for line splitting and unsplitting (`--split-until-width` / `--unsplit` / `--sentinel`),
//! exercised through the public `format_table` API. Wrapping internals (`wrap_cell`,
//! allocation, the `Fragment`/hyphen machinery) are unit-tested inside `split.rs` itself,
//! where its private items are reachable.

use crate::{
    format_table, read_lines, resolve_terminal_width, visible_len, Args, FormatError,
    FormatOptions, Divider, FALLBACK_TERMINAL_WIDTH,
};

fn to_strings(arr: &[&str]) -> Vec<String> {
    arr.iter().map(|s| s.to_string()).collect()
}

// ——— Splitting ——————————————————————————————————————————————————————————————

#[test]
fn split_until_width_wraps_aligned_within_budget() {
    let input = to_strings(&[
        "name  detail",
        "foo   a fairly long detail that has to wrap across several lines",
        "bar   short",
    ]);
    let plain = format_table(&input, &FormatOptions::default()).unwrap();
    let wrapped = format_table(&input, &FormatOptions { split_until_width: Some(24), ..Default::default() }).unwrap();

    // no visual line exceeds the budget (gutter + joins included)
    for line in &wrapped {
        assert!(visible_len(line) <= 24, "{line:?} is {} cols", visible_len(line));
    }
    // the long record wrapped onto extra visual lines, marked in the gutter
    assert!(wrapped.len() > plain.len(), "nothing wrapped");
    assert!(wrapped.iter().any(|l| l.starts_with('\u{b7}')), "no continuation marker");
    assert!(wrapped[0].starts_with(' '), "first line should be a gutter space, not a marker");
}

#[test]
fn wrapping_the_fixture_fits_the_budget_and_keeps_ansi() {
    // real ANSI + wide content: the pipe-framed fixture, divided on " | " then wrapped
    let raw = read_lines("testing/freq_table.txt").unwrap();
    let wrapped = format_table(
        &raw,
        &FormatOptions { divide_by: " | ".to_string(), split_until_width: Some(60), ..Default::default() },
    )
    .unwrap();

    for line in &wrapped {
        assert!(visible_len(line) <= 60, "{line:?} is {} cols", visible_len(line));
    }
    assert!(wrapped.iter().any(|l| l.contains('\u{1b}')), "styling was lost while wrapping");
}

// ——— Unsplitting (round trip) ————————————————————————————————————————————————

#[test]
fn split_until_width_then_unsplit_recovers_the_table() {
    // wrap a table narrow, then unsplit it: we get the un-wrapped table back
    let input = to_strings(&[
        "name  detail",
        "foo   a fairly long detail that has to wrap across several lines",
        "bar   short",
    ]);
    let plain = format_table(&input, &FormatOptions::default()).unwrap();
    let wrapped = format_table(&input, &FormatOptions { split_until_width: Some(24), ..Default::default() }).unwrap();
    assert!(wrapped.len() > plain.len(), "nothing wrapped");

    let restored = format_table(&wrapped, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    assert_eq!(restored, plain);
}

#[test]
fn hard_broken_words_round_trip_byte_exact_via_hyphen() {
    // a narrow budget forces the long word to break mid-way; the hyphen (‐, U+2010) makes
    // the split visible AND lossless — split then unsplit returns the exact un-wrapped table
    let input = to_strings(&["id  note", "7  antidisestablishmentarianism"]);
    let plain = format_table(&input, &FormatOptions::default()).unwrap();

    let split = format_table(&input, &FormatOptions { split_until_width: Some(16), ..Default::default() }).unwrap();
    assert!(split.iter().any(|l| l.contains('\u{2010}')), "expected a hyphen at the forced break");
    for line in &split {
        assert!(visible_len(line) <= 16, "{line:?} overflows");
    }

    let restored = format_table(&split, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    assert_eq!(restored, plain); // byte-exact

    // and when nothing is force-broken, no hyphen appears at all
    let easy = to_strings(&["id  note", "7  short enough words only"]);
    let easy_split = format_table(&easy, &FormatOptions { split_until_width: Some(18), ..Default::default() }).unwrap();
    assert!(!easy_split.iter().any(|l| l.contains('\u{2010}')), "hyphen leaked without a mid-word break");
}

#[test]
fn unsplit_recovers_the_wide_fixture_with_matching_delimiters() {
    // real ANSI + wide content: split and unsplit with the same " | " delimiters round-trips.
    // (Blank/all-empty rows are an edge case that doesn't round-trip exactly, so drop them.)
    let raw: Vec<String> = read_lines("testing/freq_table_overflowing.txt")
        .unwrap()
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let base = FormatOptions { divide_by: " | ".to_string(), join_with: " | ".to_string(), ..Default::default() };

    let wide = format_table(&raw, &base).unwrap();
    let wrapped = format_table(&raw, &FormatOptions { split_until_width: Some(100), ..base.clone() }).unwrap();
    for line in &wrapped {
        assert!(visible_len(line) <= 100, "{line:?} is {} cols", visible_len(line));
    }
    let restored = format_table(&wrapped, &FormatOptions { unsplit: true, ..base.clone() }).unwrap();
    assert_eq!(restored, wide);
}

// ——— Interactions with other options ———————————————————————————————————————

#[test]
fn remove_trailing_spaces_composes_with_splitting() {
    // trim is honored under split (one shared post-pass serves both render paths):
    // lines still fit the budget, carry no trailing whitespace, and still unsplit
    let input = to_strings(&[
        "name  detail",
        "foo   a fairly long detail that has to wrap across several lines",
        "bar   short",
    ]);
    let opts = FormatOptions { split_until_width: Some(24), trim_trailing: true, ..Default::default() };
    let split = format_table(&input, &opts).unwrap();
    for line in &split {
        assert!(visible_len(line) <= 24, "{line:?} is {} cols", visible_len(line));
        assert_eq!(line.trim_end(), line, "trailing whitespace survived the trim: {line:?}");
    }

    // unsplitting the trimmed split gives the trimmed plain table
    let restored = format_table(&split, &FormatOptions { unsplit: true, trim_trailing: true, ..Default::default() }).unwrap();
    let plain_trimmed = format_table(&input, &FormatOptions { trim_trailing: true, ..Default::default() }).unwrap();
    assert_eq!(restored, plain_trimmed);
}

#[test]
fn sorting_composes_with_splitting() {
    // sort happens on records, then the sorted table splits — order preserved across
    // visual lines, budget still respected
    let input = to_strings(&["name  v", "alpha  3", "beta  1", "gamma  2"]);
    let opts = FormatOptions { sort: Some(1), split_until_width: Some(12), ..Default::default() };
    let split = format_table(&input, &opts).unwrap();
    for line in &split {
        assert!(visible_len(line) <= 12, "{line:?} overflows");
    }
    // descending by v: alpha(3), gamma(2), beta(1) — header pinned on top
    let order: Vec<usize> = ["alpha", "gamma", "beta"]
        .iter()
        .map(|name| split.iter().position(|l| l.contains(name)).unwrap())
        .collect();
    assert!(order[0] < order[1] && order[1] < order[2], "sort order lost in split: {split:?}");
}

#[test]
fn split_lines_width_resolution_ladder() {
    // an explicit $COLUMNS wins (the user's override, and the only rung a non-tty sees)
    assert_eq!(resolve_terminal_width(Some("120"), Some(90)), 120);
    // unparseable or zero COLUMNS falls through to the tty, then to the 80 fallback
    assert_eq!(resolve_terminal_width(Some("nonsense"), Some(90)), 90);
    assert_eq!(resolve_terminal_width(Some("0"), None), FALLBACK_TERMINAL_WIDTH);
    assert_eq!(resolve_terminal_width(None, Some(64)), 64);
    assert_eq!(resolve_terminal_width(None, None), FALLBACK_TERMINAL_WIDTH);
}

#[test]
fn split_lines_behavior_is_reachable_from_the_library() {
    // a dependent crate can do exactly what --split-lines does, without clap: detect the
    // width (env-dependent here, so only sanity-checked) and hand it to format_table
    let width = crate::terminal_width();
    assert!(width >= 1, "terminal_width must always produce something usable");

    let input = to_strings(&["a  b"]);
    let opts = FormatOptions { split_until_width: Some(width.max(4)), ..Default::default() };
    assert!(format_table(&input, &opts).is_ok());
}

#[test]
fn split_lines_flag_conflicts_are_enforced_by_the_cli() {
    use clap::Parser;
    // --split-lines IS --split-until-width (auto) — giving both is contradictory, and the
    // frame can't survive splitting either. These are CLI-only flags, so clap enforces it.
    assert!(Args::try_parse_from(["tf", "--split-lines", "--split-until-width", "50"]).is_err());
    assert!(Args::try_parse_from(["tf", "--split-lines", "--emit-frame"]).is_err());
    assert!(Args::try_parse_from(["tf", "--split-lines"]).is_ok());
}

#[test]
fn impossibly_narrow_budget_is_survived_with_overflow() {
    // three columns can never fit in width 4 (gutter + two joins + one char per column
    // already exceeds it): the split accepts the structural overflow deterministically —
    // no panic, no infinite loop, same output every time
    let input = to_strings(&["aa  bb  cc", "d  e  f"]);
    let opts = FormatOptions { split_until_width: Some(4), ..Default::default() };
    let once = format_table(&input, &opts).unwrap();
    let twice = format_table(&input, &opts).unwrap();
    assert!(!once.is_empty());
    assert_eq!(once, twice, "pathological budgets must still be deterministic");
}

#[test]
fn splitting_empty_input_is_empty() {
    let out = format_table::<String>(&[], &FormatOptions { split_until_width: Some(10), ..Default::default() }).unwrap();
    assert!(out.is_empty());
}

#[test]
fn blank_lines_inside_a_split_table_survive_without_panic() {
    // a blank record is a no-data cell, so the base layer fills it with `-`; splitting and
    // unsplitting keep the record count and every record's content
    let input = to_strings(&["a  bbbbbbbb", "", "c  d"]);
    let split = format_table(&input, &FormatOptions { split_until_width: Some(8), ..Default::default() }).unwrap();
    for line in &split {
        assert!(visible_len(line) <= 8, "{line:?} overflows");
    }

    let restored = format_table(&split, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    assert_eq!(restored.len(), input.len(), "record count changed across the round trip");
    let squash = |s: &String| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(squash(&restored[0]), "a bbbbbbbb");
    assert_eq!(squash(&restored[1]), "-", "the blank record carries the no-data filler");
    assert_eq!(squash(&restored[2]), "c d");
}

// ——— Documented limitations (in-band markers vs. data that equals them) ————

#[test]
fn a_lone_sentinel_data_cell_reads_back_as_empty() {
    // a data cell whose entire content equals the sentinel is indistinguishable from a
    // placeholder, so unsplit reads it as an empty cell. If your data uses `·`, pick a
    // different --sentinel.
    let input = to_strings(&["x  data one two three", "y  \u{b7}"]);
    let split = format_table(&input, &FormatOptions { split_until_width: Some(12), ..Default::default() }).unwrap();
    let restored = format_table(&split, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    assert_eq!(restored.last().unwrap().trim(), "y", "the lone-sentinel cell is read as empty");
}

#[test]
fn a_data_word_ending_with_the_break_hyphen_can_merge_on_unsplit() {
    // a data word that ends with the break marker `‐` (U+2010) and lands at the end of a
    // fragment is indistinguishable from a forced mid-word break, so unsplit joins it to
    // the next word with no space: "ab‐ cd" comes back as "abcd".
    let input = to_strings(&["id  note", "7  ab\u{2010} cd"]);
    let split = format_table(&input, &FormatOptions { split_until_width: Some(9), ..Default::default() }).unwrap();
    assert!(split.len() > 2, "expected the note column to wrap");
    let restored = format_table(&split, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    let squash = |s: &String| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(squash(&restored[1]), "7 abcd", "the trailing data hyphen merges the words");
}

// ——— A configurable sentinel, shared by split and unsplit ————————————————————

#[test]
fn custom_sentinel_splits_and_unsplits() {
    let input = to_strings(&[
        "name  detail",
        "foo   a fairly long detail that must wrap over here",
        "bar   short",
    ]);
    let plain = format_table(&input, &FormatOptions::default()).unwrap();

    // split with '#': the marker is '#', never the default '·'
    let split = format_table(&input, &FormatOptions { split_until_width: Some(24), sentinel: '#', ..Default::default() }).unwrap();
    assert!(split.iter().any(|l| l.contains('#')), "custom sentinel not used");
    assert!(!split.iter().any(|l| l.contains('\u{b7}')), "default sentinel leaked");

    // unsplit with the SAME '#' recovers the table exactly
    let restored = format_table(&split, &FormatOptions { unsplit: true, sentinel: '#', ..Default::default() }).unwrap();
    assert_eq!(restored, plain);
}

#[test]
fn unsplitting_with_the_wrong_sentinel_does_not_round_trip() {
    let input = to_strings(&[
        "name  detail",
        "foo   a fairly long detail that must wrap over here",
        "bar   short",
    ]);
    let plain = format_table(&input, &FormatOptions::default()).unwrap();
    let split = format_table(&input, &FormatOptions { split_until_width: Some(24), sentinel: '#', ..Default::default() }).unwrap();

    // unsplitting with the DEFAULT '·' can't recognize the '#' gutters/placeholders
    let wrong = format_table(&split, &FormatOptions { unsplit: true, ..Default::default() }).unwrap();
    assert_ne!(wrong, plain, "a mismatched sentinel must not round-trip");

    // …but unsplitting with the correct '#' does
    let right = format_table(&split, &FormatOptions { unsplit: true, sentinel: '#', ..Default::default() }).unwrap();
    assert_eq!(right, plain);
}

// ——— Validation ———————————————————————————————————————————————————————————

#[test]
fn split_until_width_below_the_delimiter_is_rejected() {
    let input = to_strings(&["a  b", "c  d"]);
    // < 2 is rejected (the default "  " delimiter is 2 wide)
    assert_eq!(
        format_table(&input, &FormatOptions { split_until_width: Some(1), ..Default::default() }).unwrap_err(),
        FormatError::SplitWidthTooSmall { width: 1, minimum: 2 }
    );
    // narrower than the -j (output) delimiter is rejected: " | " is 3 wide, so 2 is too small
    let narrow = FormatOptions { join_with: " | ".to_string(), split_until_width: Some(2), ..Default::default() };
    assert_eq!(
        format_table(&input, &narrow).unwrap_err(),
        FormatError::SplitWidthTooSmall { width: 2, minimum: 3 }
    );
    // exactly the minimum is accepted
    let ok = FormatOptions { join_with: " | ".to_string(), split_until_width: Some(3), ..Default::default() };
    assert!(format_table(&input, &ok).is_ok());
}

#[test]
fn whitespace_sentinel_is_rejected() {
    let input = to_strings(&["a  b"]);
    let err = format_table(&input, &FormatOptions { split_until_width: Some(10), sentinel: ' ', ..Default::default() })
        .unwrap_err();
    assert_eq!(err, FormatError::InvalidSentinel { value: " ".to_string() });
}

#[test]
fn split_until_width_conflicts_with_emit_frame() {
    // wrapping stacks a record across lines, so a per-record frame can't stay intact
    let input = to_strings(&["a  b", "c  d"]);
    let opts = FormatOptions {
        join_with: " | ".to_string(),
        emit_frame: true,
        split_until_width: Some(40),
        ..Default::default()
    };
    assert_eq!(
        format_table(&input, &opts).unwrap_err(),
        FormatError::ConflictingOptions { first: "--emit-frame", second: "--split-until-width" }
    );
}

// ——— Real-world wide Markdown table (cylindrical_batteries.md) ————————————
// A 10-column Markdown table (`-d " | "`). Cells with no data carry a `-` marker (the
// fixture was filled deliberately: truly empty cells and in-band placeholders are
// ambiguous). Characteristics only, not exact bytes.

const BATTERIES: &str = "testing/cylindrical_batteries.md";

fn pipe_opts(split: Option<usize>) -> FormatOptions {
    FormatOptions {
        divide_by: " | ".to_string(),
        join_with: " | ".to_string(),
        split_until_width: split,
        ..Default::default()
    }
}

/// The data words of each line, ANSI- and separator-stripped — for comparing that content
/// survives a round trip, independent of exact spacing or empty-cell/separator counts.
fn content(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| console::strip_ansi_codes(l).replace('|', " ").split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn batteries_split_no_line_overflows() {
    let raw = read_lines(BATTERIES).unwrap();
    let width = 160;
    for line in &format_table(&raw, &pipe_opts(Some(width))).unwrap() {
        assert!(visible_len(line) <= width, "{} cols: {line:?}", visible_len(line));
    }
}

#[test]
fn batteries_split_leaves_no_empty_cell() {
    // every column on every visual line carries content or a `·` placeholder — nothing blank
    let raw = read_lines(BATTERIES).unwrap();
    let split = format_table(&raw, &pipe_opts(Some(160))).unwrap();
    let sep = Divider::new(" | ");
    for line in &split {
        let rest: String = line.chars().skip(1).collect(); // drop the one-char gutter
        for cell in sep.split(rest.trim()) {
            assert!(!cell.trim().is_empty(), "empty cell in {line:?}");
        }
    }
}

#[test]
fn batteries_split_then_unsplit_preserves_content() {
    // exact spacing may shift, but every cell's content survives split → unsplit
    let raw = read_lines(BATTERIES).unwrap();
    let wide = format_table(&raw, &pipe_opts(None)).unwrap();
    let split = format_table(&raw, &pipe_opts(Some(160))).unwrap();
    let restored = format_table(&split, &FormatOptions { unsplit: true, ..pipe_opts(None) }).unwrap();
    assert_eq!(content(&restored), content(&wide));
}

#[test]
fn batteries_unsplitting_the_unsplit_original_is_safe() {
    // --unsplit on a table that was never split must not panic and stays one record per line
    let raw = read_lines(BATTERIES).unwrap();
    let out = format_table(&raw, &FormatOptions { unsplit: true, ..pipe_opts(None) }).unwrap();
    assert_eq!(out.len(), raw.len());
}
