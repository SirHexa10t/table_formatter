use std::fs::File;
use crate::{
    format_table, is_numeric_or_neutral, parse_numeric, read_from, read_lines, run_from,
    split_pattern, split_row, visible_len, FormatError, FormatOptions,
};
use test_case::test_case;

// numerical column needs to align right
// extra excessive spaces need to be trimmed off
// tabs need to be deleted (including '	')
// 1-spaced words need to stay together
// colored word needs to avoid padding the whole column with invisible
const SAMPLE_INPUT: &[&str] = &[
    "num  word\ta  long_word   b",
    "   1  one   ",
    "2  very long spaced  a  c  d  e	f\tg  h  i  j  k",
    "5k  a  b  c  \u{1b}[31mcolored\u{1b}[0m  d",
];

const SAMPLE_OUTPUT: &[&str] = &[
    "num  word              a  long_word  b                           ",
    "  1  one                                                         ",
    "  2  very long spaced  a  c          d        e  f  g  h  i  j  k",
    " 5k  a                 b  c          \u{1b}[31mcolored\u{1b}[0m  d                  ",
];

const SMTOUHOU_DATA: &[&str] = &[
    "  #      Name            Lv.   HP      MP      ATK   DEF",
    "1      Reimu            40      193   211   63      82   ",
    "2      Marisa         28      125   166   46      57   ",
    "3      Shingyoku      89      620   505   202   182",
    "4      Yugenmagan   87      628   576   176   189",
    "5      Elis            78      495   448   215   145",
    "6      Sariel         90      690   630   164   217",
    "7      Mima            74      494   472   146   166",
];

const SMTOUHOU_DATA_ORGANIZED: &[&str] = &[
    "#  Name        Lv.   HP   MP  ATK  DEF",
    "1  Reimu        40  193  211   63   82",
    "2  Marisa       28  125  166   46   57",
    "3  Shingyoku    89  620  505  202  182",
    "4  Yugenmagan   87  628  576  176  189",
    "5  Elis         78  495  448  215  145",
    "6  Sariel       90  690  630  164  217",
    "7  Mima         74  494  472  146  166",
];

const LONG_TABLE: &[&str] = &[
    "A  B",
    " 1      X",
    "2    X",
    "3    X",
    "4     X",
    "5    X",
    "6  X",
    "7    X",
    "7  X",
    "8        X",
    "8      X",
];

const LONG_TABLE_ORGANIZED: &[&str] = &[
    "A  B",
    "1  X",
    "2  X",
    "3  X",
    "4  X",
    "5  X",
    "6  X",
    "7  X",
    "7  X",
    "8  X",
    "8  X",
];

const WIDE_TABLE: &[&str] = &[
    "A  B       c  d  e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u  v       w  x  y  z",
    "A  B  c  d  e  f  g  h  i  j  k  l  m  n       o  p  q  r  s  t  u  v  w  x  y  z",
    "A  B  c  d  e  f  g  h  i  j       k  l  m  n  o  p  q       r  s  t  u  v  w  x  y  z",
    "A       B  c  d       e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u       v  w  x  y  z",
];

const WIDE_TABLE_ORGANIZED: &[&str] = &[
    "A  B  c  d  e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u  v  w  x  y  z",
    "A  B  c  d  e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u  v  w  x  y  z",
    "A  B  c  d  e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u  v  w  x  y  z",
    "A  B  c  d  e  f  g  h  i  j  k  l  m  n  o  p  q  r  s  t  u  v  w  x  y  z",
];

const VARYING_LENGTH_TABLE: &[&str] = &[
    "A            1  c  d  e       f  g ",
    "B       8",
    "C  4  c  d  e  f  g ",
    "D  3       c",
    "E       5  c  d  e  f  g ",
    "H  6",
    "G  7       c  d       e  f       g ",
];

const VARYING_LENGTH_TABLE_ORGANIZED: &[&str] = &[
    "A  1  c  d  e  f  g",
    "B  8               ",
    "C  4  c  d  e  f  g",
    "D  3  c            ",
    "E  5  c  d  e  f  g",
    "H  6               ",
    "G  7  c  d  e  f  g",
];


const MISSING_LINES: &[&str] = &[
    "A  B",
    " 1      X",
    "2    X",
    "3    X",
    "",
    "5    X",
    "",
    "7    X",
    "7  X",
    "8        X",
    "8      X",
];

const MISSING_LINES_ORGANIZED: &[&str] = &[
    "A  B",
    "1  X",
    "2  X",
    "3  X",
    "    ",
    "5  X",
    "    ",
    "7  X",
    "7  X",
    "8  X",
    "8  X",
];

const SPECIAL_CHARS: &[&str] = &[
    "A  B",
    "1  x",
    "🌎     X",
    "🇺🇸     X",
    "3  X",
];

// 🌎 and 🇺🇸 both occupy two terminal cells, so every X lands in the same display column.
// (🌎 is a single char — padding by chars used to give its row one extra space.)
const SPECIAL_CHARS_ORGANIZED: &[&str] = &[
    "A   B",
    "1   x",
    "🌎  X",
    "🇺🇸  X",
    "3   X",
];

fn to_strings(arr: &[&str]) -> Vec<String> {
    arr.iter().map(|s| s.to_string()).collect()
}

fn options_with_sort(col: usize) -> FormatOptions {
    FormatOptions { sort: Some(col), ..Default::default() }
}

fn format_default(input: &[&str]) -> Vec<String> {
    format_table(&to_strings(input), &FormatOptions::default()).unwrap()
}

fn format_sorted(input: &[&str], col: usize) -> Vec<String> {
    format_table(&to_strings(input), &options_with_sort(col)).unwrap()
}

#[test]
fn join_with_sets_the_output_gap() {
    // two-space input splits into two columns; the join string is the between-column gap
    let input = to_strings(&["a  b"]);
    let opts = FormatOptions { join_with: "    ".to_string(), ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["a    b"]));
}

#[test]
fn wider_whitespace_divider_keeps_smaller_gaps_together() {
    // "   " (3 spaces) divides only on 3+ whitespace, so a 2-space gap stays one cell
    // — the string-delimiter form of the old `--threshold 3`.
    let input = to_strings(&["a  b"]);
    let opts = FormatOptions { divide_by: "   ".to_string(), ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["a  b"]));
}

#[test]
fn single_space_divider_is_rejected_and_default_keeps_multiword_cells() {
    let input = to_strings(&["one two  three"]);
    // you can't ask for a single-space column break — it's an invalid delimiter
    let opts = FormatOptions { divide_by: " ".to_string(), ..Default::default() };
    assert_eq!(
        format_table(&input, &opts).unwrap_err(),
        FormatError::InvalidDelimiter { flag: "--divide-by", value: " ".to_string() }
    );
    // and by default, single interior spaces stay glued (only the 2-space run splits)
    assert_eq!(
        format_table(&input, &FormatOptions::default()).unwrap(),
        to_strings(&["one two  three"])
    );
}

#[test]
fn delimiters_without_surrounding_whitespace_are_rejected() {
    let input = to_strings(&["aa  b", "c  dd"]);
    for bad in ["", "|", " ", "x", "a|b"] {
        let join = FormatOptions { join_with: bad.to_string(), ..Default::default() };
        assert_eq!(
            format_table(&input, &join).unwrap_err(),
            FormatError::InvalidDelimiter { flag: "--join-with", value: bad.to_string() }
        );
        let divide = FormatOptions { divide_by: bad.to_string(), ..Default::default() };
        assert_eq!(
            format_table(&input, &divide).unwrap_err(),
            FormatError::InvalidDelimiter { flag: "--divide-by", value: bad.to_string() }
        );
    }
    // the error message names the flag and shows the offending value + a fix
    let err = FormatError::InvalidDelimiter { flag: "--join-with", value: "|".to_string() };
    assert_eq!(err.to_string(), "--join-with \"|\" must have leading and trailing whitespace (e.g. \" | \")");
}

// ——— Custom delimiters (--divide-by / --join-with) ————————————————————————

#[test]
fn divide_by_pipe_splits_pipe_delimited_input() {
    // " | " divides input into columns; output re-joins with the default "  "
    let input = to_strings(&["ab | cd", "ef | gh"]);
    let opts = FormatOptions { divide_by: " | ".to_string(), ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["ab  cd", "ef  gh"]));
}

#[test]
fn divide_by_matches_whitespace_flexibly_around_the_core() {
    // one space, several spaces, or a tab around the pipe all divide identically
    let opts = FormatOptions { divide_by: " | ".to_string(), ..Default::default() };
    for line in ["x | y", "x  |  y", "x\t|\ty", "x   |\ty"] {
        assert_eq!(
            format_table(&to_strings(&[line]), &opts).unwrap(),
            to_strings(&["x  y"]),
            "{line:?} should divide into [x, y]"
        );
    }
}

#[test]
fn join_with_renders_a_visible_delimiter() {
    // default 2-space input, joined for display with " | " (padding still aligns columns)
    let input = to_strings(&["ab  cd", "ef  gh"]);
    let opts = FormatOptions { join_with: " | ".to_string(), ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["ab | cd", "ef | gh"]));
}

#[test]
fn matching_divide_and_join_round_trips() {
    // dividing and joining on the same " | " makes formatting idempotent: a messy table
    // organizes once, then re-formats to itself — the stitching guarantee, for pipes.
    let opts = FormatOptions {
        divide_by: " | ".to_string(),
        join_with: " | ".to_string(),
        ..Default::default()
    };
    let messy = to_strings(&["a | bbbb", "cccc | d"]);
    let organized = format_table(&messy, &opts).unwrap();
    assert_eq!(organized, to_strings(&["a    | bbbb", "cccc | d   "]));
    // second pass is a no-op
    assert_eq!(format_table(&organized, &opts).unwrap(), organized);
}

#[test]
fn bordered_pipe_table_divides_joins_and_round_trips() {
    // testing/freq_tables.txt is a Markdown-style table: every row is framed as `| … |`,
    // with ANSI color inside cells. Dividing on " | " must peel that outer frame instead
    // of fusing it onto the edge cells.
    let raw = read_lines("testing/freq_tables.txt").unwrap();

    let divide = FormatOptions { divide_by: " | ".to_string(), ..Default::default() };
    let rejoin = FormatOptions { join_with: " | ".to_string(), ..Default::default() };
    let both = FormatOptions {
        divide_by: " | ".to_string(),
        join_with: " | ".to_string(),
        ..Default::default()
    };

    // Pipeline A: divide " | " (default join), then on that output join " | " (default divide).
    let a1 = format_table(&raw, &divide).unwrap();
    let a2 = format_table(&a1, &rejoin).unwrap();
    // Pipeline B: divide and join " | " in a single pass.
    let b = format_table(&raw, &both).unwrap();

    // The two pipelines must agree, line for line.
    assert_eq!(a2, b, "two-pass and one-pass results must match");
    assert_eq!(b.len(), raw.len(), "one output line per input line");

    // The frame is consumed, not carried as content: no cell begins or ends with a pipe.
    for line in &b {
        assert!(!line.starts_with('|'), "leading frame leaked into column 0: {line:?}");
        assert!(!line.trim_end().ends_with('|'), "trailing frame leaked into last column: {line:?}");
    }

    // ANSI content survives the round trip (the colored cells are still colored).
    assert!(b.iter().any(|line| line.contains('\u{1b}')), "styling was lost");
}

#[test]
fn divide_by_pipe_strips_the_markdown_frame() {
    // a lone framed row divides into exactly its inner cells — no empty edges, no stuck pipes
    let opts = FormatOptions { divide_by: " | ".to_string(), ..Default::default() };
    assert_eq!(
        format_table(&to_strings(&["| a | bb | c |"]), &opts).unwrap(),
        to_strings(&["a  bb  c"])
    );
    // a frame is optional per line: an unframed row divides the same way
    assert_eq!(
        format_table(&to_strings(&["a | bb | c"]), &opts).unwrap(),
        to_strings(&["a  bb  c"])
    );
}

#[test]
fn emit_frame_wraps_lines_in_the_join_delimiter_edges() {
    // joining with " | " and framing turns "a | b" into "| a | b |"
    let input = to_strings(&["ab  cd", "ef  gh"]);
    let opts = FormatOptions {
        join_with: " | ".to_string(),
        emit_frame: true,
        ..Default::default()
    };
    assert_eq!(
        format_table(&input, &opts).unwrap(),
        to_strings(&["| ab | cd |", "| ef | gh |"])
    );
}

#[test]
fn emit_frame_is_a_noop_without_a_join_core() {
    // the default join is whitespace-only, so it has no edge characters to add
    let input = to_strings(&["a  b", "c  d"]);
    let opts = FormatOptions { emit_frame: true, ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["a  b", "c  d"]));
}

#[test]
fn framed_markdown_table_round_trips_through_matching_divide_join() {
    // peel the frame on input, re-emit it on output: a Markdown table organizes once,
    // then re-formats to itself. `--emit-frame` is the exact inverse of frame-peeling.
    let opts = FormatOptions {
        divide_by: " | ".to_string(),
        join_with: " | ".to_string(),
        emit_frame: true,
        ..Default::default()
    };
    let messy = to_strings(&["| a | bbbb |", "| cccc | d |"]);
    let organized = format_table(&messy, &opts).unwrap();
    assert_eq!(organized, to_strings(&["| a    | bbbb |", "| cccc | d    |"]));
    // and it's stable: a second pass is a no-op
    assert_eq!(format_table(&organized, &opts).unwrap(), organized);
}

#[test]
fn emit_frame_and_trim_trailing_are_mutually_exclusive() {
    // the frame needs the trailing padding to stay aligned, so the combination is refused
    let input = to_strings(&["ab  cd", "ef  gh"]);
    let opts = FormatOptions {
        join_with: " | ".to_string(),
        emit_frame: true,
        trim_trailing: true,
        ..Default::default()
    };
    let err = format_table(&input, &opts).unwrap_err();
    assert_eq!(
        err,
        FormatError::ConflictingOptions {
            first: "--emit-frame",
            second: "--remove-trailing-spaces",
        }
    );
    assert_eq!(err.to_string(), "--emit-frame cannot be combined with --remove-trailing-spaces");

    // via the CLI, the conflict surfaces as a clean InvalidInput error, not a panic
    let cli = run_from([
        "table_formatter", "a  b", "--emit-frame", "--remove-trailing-spaces",
    ])
    .unwrap_err();
    assert_eq!(cli.kind(), std::io::ErrorKind::InvalidInput);
    assert!(cli.to_string().contains("cannot be combined with"));
}

#[test]
fn run_from_reports_invalid_delimiter_as_clean_io_error() {
    // the CLI path wraps the delimiter FormatError as InvalidInput instead of panicking
    let err = run_from(["table_formatter", "a  b", "--join-with", "|"]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("leading and trailing whitespace"));
}

// Exercise the CLI's input handling + formatting in-process: read the input exactly as the
// binary would (via `read_lines` / `read_from`), then format it. `main` is only a thin
// wrapper around `table_formatter::run()`, so this covers the same logic without needing
// the compiled binary — which `cargo test` doesn't build for unit tests.
fn format_arg(arg: &str) -> Vec<String> {
    format_table(&read_lines(arg).unwrap(), &FormatOptions::default()).unwrap()
}

fn format_stdin(piped: &str) -> Vec<String> {
    let lines = read_from(std::io::Cursor::new(piped.as_bytes().to_vec())).unwrap();
    format_table(&lines, &FormatOptions::default()).unwrap()
}
fn direct_test(input: &[&str], expected: &[&str]) {  // call the actual function directly
    assert_eq!(format_default(input), to_strings(expected));
}

fn file_input_test(input: &[&str], expected: &[&str]) {  // run the program through its bin-file and provide a temp-file
    use tempfile::NamedTempFile;
    use std::fs;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(&temp_file, input.join("\n")).unwrap();

    let result = format_arg(temp_file.path().to_str().unwrap());

    assert_eq!(result, to_strings(expected));
}

fn string_input_test(input: &[&str], expected: &[&str]) {
    let result = format_arg(&input.join("\n"));
    assert_eq!(result, to_strings(expected));
}

fn piped_input_test(input: &[&str], expected: &[&str]) {
    let result = format_stdin(&input.join("\n"));
    assert_eq!(result, to_strings(expected));
}

fn check_immutability_on_2nd_run(input: &[&str]) {  // input is a pre-organized table. There's nothing to further organize.
    assert_eq!(format_default(input), to_strings(input));
}

#[test_case(SAMPLE_INPUT, SAMPLE_OUTPUT)]
#[test_case(SMTOUHOU_DATA, SMTOUHOU_DATA_ORGANIZED)]
#[test_case(LONG_TABLE, LONG_TABLE_ORGANIZED)]
#[test_case(WIDE_TABLE, WIDE_TABLE_ORGANIZED)]
#[test_case(VARYING_LENGTH_TABLE, VARYING_LENGTH_TABLE_ORGANIZED)]
#[test_case(MISSING_LINES, MISSING_LINES_ORGANIZED)]
#[test_case(SPECIAL_CHARS, SPECIAL_CHARS_ORGANIZED)]
fn test_sets(input: &[&str], expected: &[&str]) {
    direct_test(input, expected);
    file_input_test(input, expected);
    string_input_test(input, expected);
    piped_input_test(input, expected);
    check_immutability_on_2nd_run(expected);
}



#[test_case("testing/edf4.1_ranger_testfile.csv")]
fn test_with_large_file(input_file: &str) {  // covers test for symbols that take a different number of chars than displayed
    let result = format_arg(input_file);

    let containment_checks = vec![
        ("Type           LV  LV                                 DPS   RDPS     DPM  Ammo  \"Rate of Fire (fire/sec)\"  Damage  \"Reload (sec)\"  \"Range (m)\"  Accuracy                    Zoom  Lock time  -    -        time per mag", "Header line missing or messed-up"),
        ("Sniper         72  Nova Buster ZD                   80000  80000   80000     1                          1   80000               0         1240  S+                          5x            -  -    -                   1", "Line below header missing or messed-up"),
        ("GrenL          37  Splash Grenade α                 20000   2857   20000     1                          1   20000               6           10  Timed / 10sec               -             -  -    -                   7", "Line with 2-char symbol missing or messed-up"),
        ("Sniper          0  MMF40                               77     60     550     5                        0.7     110               2          600  S+                          4x            -  -    -         9.142857143", "Arbitrary late line missing or messed-up"),
    ];

    assert!(!result.is_empty());
    for (expected, errmsg) in containment_checks {
        assert!(result.contains(&expected.to_string()), "{}", errmsg);
    }
}

#[test_case("testing/non_utf8.txt")]
fn test_with_non_utf8_chars(input_file: &str) {
    use std::io::{BufReader, Read};

    let result = format_arg(input_file);

    // Read raw bytes (no UTF-8 assumption)
    let mut buf = Vec::new();
    BufReader::new(File::open(input_file).unwrap())
        .read_to_end(&mut buf)
        .unwrap();

    // Convert lossy so we can compare line-wise
    let file_contents: Vec<String> = String::from_utf8_lossy(&buf)
        .lines()
        .map(|s| s.to_string())
        .collect();

    assert_eq!(result, file_contents);  // see that the output doesn't alter the data (even if it can't be displayed right)
}

#[test]
fn test_sorting() {
    const VARYING_LENGTH_TABLE_SORT0_ORGANIZED: &[&str] = &[
        "A  1  c  d  e  f  g",
        "B  8               ",
        "C  4  c  d  e  f  g",
        "D  3  c            ",
        "E  5  c  d  e  f  g",
        "G  7  c  d  e  f  g",
        "H  6               ",
    ];

    const VARYING_LENGTH_TABLE_SORT1_ORGANIZED: &[&str] = &[
        "B  8               ",
        "G  7  c  d  e  f  g",
        "H  6               ",
        "E  5  c  d  e  f  g",
        "C  4  c  d  e  f  g",
        "D  3  c            ",
        "A  1  c  d  e  f  g",
    ];

    assert_eq!(format_sorted(VARYING_LENGTH_TABLE, 0), to_strings(VARYING_LENGTH_TABLE_SORT0_ORGANIZED));
    assert_eq!(format_sorted(VARYING_LENGTH_TABLE, 1), to_strings(VARYING_LENGTH_TABLE_SORT1_ORGANIZED));


    const SORT_TESTER: &[&str] = &[
        "X     X     X",
        "2  1000    2M",
        "3     9  3.5K",
        "4     5    9G",
        "5     6    3G",
        "6     8   10T",
        "7     9  288M",
    ];
    // The sort is stable: the two rows tied at 9 keep their input order (row "3" before "7").
    const SORT_TESTER_SORT1: &[&str] = &[
        "X     X     X",
        "2  1000    2M",
        "3     9  3.5K",
        "7     9  288M",
        "6     8   10T",
        "5     6    3G",
        "4     5    9G",
    ];
    const SORT_TESTER_SORT2: &[&str] = &[
        "X     X     X",
        "6     8   10T",
        "4     5    9G",
        "5     6    3G",
        "7     9  288M",
        "2  1000    2M",
        "3     9  3.5K",
    ];

    assert_eq!(format_sorted(SORT_TESTER, 1), to_strings(SORT_TESTER_SORT1));
    assert_eq!(format_sorted(SORT_TESTER, 2), to_strings(SORT_TESTER_SORT2));

}

// ——— Sorting edge cases (regression tests for B1–B4) ————————————————————

#[test]
fn sorting_by_out_of_range_column_is_a_clean_error() {
    // B1: this used to panic with index-out-of-bounds
    let err = format_table(&to_strings(&["a  b", "1  2"]), &options_with_sort(99)).unwrap_err();
    assert_eq!(err, FormatError::SortColumnOutOfRange { requested: 99, num_cols: 2 });
    assert_eq!(err.to_string(), "sort column 99 is out of range: the table has 2 column(s)");
}

#[test]
fn sorting_empty_input_returns_empty() {
    // B1: this used to panic in `rows.remove(0)`
    let out = format_table::<String>(&[], &options_with_sort(0)).unwrap();
    assert!(out.is_empty());
}

#[test]
fn sort_column_missing_from_first_row_must_not_panic() {
    // B1: the header-detection heuristic indexed `rows[0][idx]` unchecked
    let input = &["a  b", "1  2  3", "4  5  6"];
    // header pinned; data rows descending by column 2
    assert_eq!(format_sorted(input, 2), to_strings(&["a  b   ", "4  5  6", "1  2  3"]));
}

#[test]
fn first_data_row_with_value_zero_is_sorted_not_pinned() {
    // B3: a first row whose sort cell evaluates to 0 was mistaken for a header
    let input = &["0  y", "5  x", "3  z"];
    assert_eq!(format_sorted(input, 0), to_strings(&["5  x", "3  z", "0  y"]));
}

#[test]
fn descending_numeric_sort_keeps_tied_rows_in_input_order() {
    // B4: sort-then-reverse used to flip the relative order of equal keys
    let input = &["h  x", "a  5", "b  5", "c  9"];
    assert_eq!(format_sorted(input, 1), to_strings(&["h  x", "c  9", "a  5", "b  5"]));
}

#[test]
fn neutral_cells_sort_below_numbers_in_numeric_columns() {
    // `-` and missing cells carry no value: they belong at the bottom, in input order
    let input = &["id  val", "a  3", "b  -", "c  7", "d"];
    assert_eq!(
        format_sorted(input, 1),
        to_strings(&["id  val", "c     7", "a     3", "b     -", "d      "])
    );
}

#[test]
fn header_flag_pins_first_row_even_when_numeric() {
    let input = to_strings(&["0  y", "5  x", "3  z"]);
    let opts = FormatOptions { sort: Some(0), header: Some(true), ..Default::default() };
    assert_eq!(format_table(&input, &opts).unwrap(), to_strings(&["0  y", "5  x", "3  z"]));
}

#[test]
fn no_header_flag_sorts_first_row_even_when_text() {
    let input = to_strings(&["num  word", "9  a", "10  b"]);
    let opts = FormatOptions { sort: Some(0), header: Some(false), ..Default::default() };
    // "num" doesn't parse as a number, so it sorts below the real values
    assert_eq!(
        format_table(&input, &opts).unwrap(),
        to_strings(&[" 10  b   ", "  9  a   ", "num  word"])
    );
}

#[test]
fn run_from_reports_invalid_sort_as_clean_io_error() {
    // the CLI path wraps FormatError as InvalidInput instead of panicking
    let err = run_from(["table_formatter", "a  b", "--sort", "9"]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("out of range"));
}

// ——— Column classification & alignment ———————————————————————————————————

#[test]
fn ragged_header_does_not_make_text_columns_numeric() {
    // B2: numeric detection used to skip each column's first *present* cell instead of
    // the header row — so "foo" below a short header was never inspected
    let input = &["h1  h2", "a  b  foo", "c  d  3"];
    assert_eq!(format_default(input), to_strings(&["h1  h2     ", "a   b   foo", "c   d   3  "]));
}

#[test]
fn resolutions_sort_by_pixel_count_not_si_scale() {
    // 'p' is a unit (pixels): 4K (= 4000) > 1440p > 1080p > 720p. It must not mean peta.
    let input = &["res", "720p", "4K", "1440p", "1080p"];
    assert_eq!(
        format_sorted(input, 0),
        to_strings(&["  res", "   4K", "1440p", "1080p", " 720p"])
    );
}

#[test]
fn remove_trailing_spaces_flag_trims_output_lines() {
    let input = to_strings(&["h1  h2", "a  b  foo", "c  d  3"]);
    let opts = FormatOptions { trim_trailing: true, ..Default::default() };
    assert_eq!(
        format_table(&input, &opts).unwrap(),
        to_strings(&["h1  h2", "a   b   foo", "c   d   3"])
    );
}

#[test]
fn wide_glyphs_align_by_display_width() {
    // 🌎 and 漢 are one *char* but two terminal cells wide; padding must follow cells.
    // (🇺🇸 is two chars and two cells — it aligns either way, so it can't catch this alone.)
    let input = &["a  B", "🌎  X", "🇺🇸  X", "漢  X", "bc  X"];
    assert_eq!(
        format_default(input),
        to_strings(&["a   B", "🌎  X", "🇺🇸  X", "漢  X", "bc  X"])
    );
}

#[test]
fn colored_cells_do_not_absorb_padding() {
    // a colored cell narrower than its column must still get its full padding
    let input = to_strings(&["name  val", "\u{1b}[32mok\u{1b}[0m  1", "longer  2"]);
    let expected = to_strings(&[
        "name    val",
        "\u{1b}[32mok\u{1b}[0m        1",
        "longer    2",
    ]);
    assert_eq!(format_table(&input, &FormatOptions::default()).unwrap(), expected);
}

// ——— Color invariance ————————————————————————————————————————————————————
// Styling characters must never change the layout: the escape sequences are invisible,
// so a colorized table has to come out with the exact spacing of its plain twin — the
// codes just ride along.

/// Tiny deterministic PRNG (an LCG) so the "random" coloring is reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const SGR_STYLES: &[&str] = &["31", "32", "33", "34", "1", "4", "38;5;208"];

/// Wrap a random selection of cells — whole, or just an inner span of characters — in
/// ANSI style codes. Cell separators stay untouched, so the table's cells are identical.
fn colorize_cells(lines: &[String], seed: u64) -> Vec<String> {
    let mut rng = Lcg(seed);
    let pattern = split_pattern("  ");
    lines
        .iter()
        .map(|line| {
            let cells: Vec<String> = split_row(line, &pattern, None)
                .into_iter()
                .map(|cell| {
                    let style = SGR_STYLES[rng.below(SGR_STYLES.len())];
                    match rng.below(3) {
                        0 => cell.to_string(), // leave as-is
                        1 => format!("\u{1b}[{style}m{cell}\u{1b}[0m"), // whole cell
                        _ => {
                            // an inner span of characters
                            let chars: Vec<char> = cell.chars().collect();
                            if chars.is_empty() {
                                return cell.to_string();
                            }
                            let start = rng.below(chars.len());
                            let end = start + 1 + rng.below(chars.len() - start);
                            let head: String = chars[..start].iter().collect();
                            let mid: String = chars[start..end].iter().collect();
                            let tail: String = chars[end..].iter().collect();
                            format!("{head}\u{1b}[{style}m{mid}\u{1b}[0m{tail}")
                        }
                    }
                })
                .collect();
            cells.join("  ")
        })
        .collect()
}

fn strip_ansi_lines(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| console::strip_ansi_codes(line).to_string()).collect()
}

#[test]
fn coloring_cells_never_changes_layout() {
    let tables: &[&[&str]] = &[
        SAMPLE_INPUT, SMTOUHOU_DATA, LONG_TABLE, WIDE_TABLE,
        VARYING_LENGTH_TABLE, MISSING_LINES, SPECIAL_CHARS,
    ];
    for (table_idx, table) in tables.iter().enumerate() {
        let plain = to_strings(table);
        let expected = strip_ansi_lines(&format_table(&plain, &FormatOptions::default()).unwrap());
        let expected_sorted =
            strip_ansi_lines(&format_table(&plain, &options_with_sort(1)).unwrap());

        for seed in [1, 42, 0x00C0_FFEE] {
            let colored = colorize_cells(&plain, seed + table_idx as u64);

            let got = strip_ansi_lines(&format_table(&colored, &FormatOptions::default()).unwrap());
            assert_eq!(got, expected, "layout changed: table {table_idx}, seed {seed}");

            // sorting must be immune too — keys parse straight through the styling
            let got_sorted =
                strip_ansi_lines(&format_table(&colored, &options_with_sort(1)).unwrap());
            assert_eq!(got_sorted, expected_sorted, "sorted layout changed: table {table_idx}, seed {seed}");
        }
    }
}

#[test]
fn text_sort_ignores_ansi_codes() {
    // found by the invariance test: a styled cell must sort by its visible text, not by
    // its escape bytes (which would put every colored cell before every plain one)
    let input = to_strings(&["name  v", "\u{1b}[32mcherry\u{1b}[0m  1", "apple  2", "banana  3"]);
    let out = format_table(&input, &options_with_sort(0)).unwrap();
    assert_eq!(
        strip_ansi_lines(&out),
        to_strings(&["name    v", "apple   2", "banana  3", "cherry  1"])
    );
}

// ——— Text measurement ————————————————————————————————————————————————————

#[test]
fn visible_len_counts_terminal_cells() {
    let cases: &[(&str, usize)] = &[
        ("abc", 3),
        ("🌎", 2),                      // one char, two cells
        ("🇺🇸", 2),                     // two chars, two cells
        ("漢字", 4),                    // CJK: two cells per glyph
        ("α", 1),
        ("cafe\u{301}", 4),             // combining accent takes no cell
        ("\u{1b}[32m🌎\u{1b}[0m", 2),   // styling adds nothing
        ("", 0),
    ];
    for (text, expected) in cases {
        assert_eq!(visible_len(text), *expected, "{text:?}");
    }
}

#[test]
fn measure_text_width_already_ignores_ansi() {
    // P4: `visible_len` used to pre-strip ANSI before `measure_text_width`, which parses
    // ANSI itself. This pins their agreement on every input — including malformed
    // escapes — which is what makes the single-parse `visible_len` safe.
    let corpus = [
        // well-formed
        "plain text",
        "\u{1b}[31mred\u{1b}[0m",
        "\u{1b}[38;5;208mcolored\u{1b}[0m",
        "\u{1b}[46m\u{1b}[23mnested\u{1b}[0m",
        "\u{1b}[31m🌎 wide\u{1b}[0m",
        "🇺🇸 flag",
        "漢字 cjk",
        // malformed / adversarial
        "\u{1b}",                       // lone ESC
        "\u{1b}[",                      // bare CSI opener
        "\u{1b}[31",                    // dangling CSI (no terminator)
        "a\u{1b}[12",                   // dangling CSI mid-text
        "text\u{1b}[",                  // trailing opener
        "\u{1b}[3\u{1b}[0m1m",          // stripping the inner code would recombine "\x1b[31m"
        "\u{1b}]0;title\u{7}body",      // OSC terminated by BEL
        "\u{1b}]0;title\u{1b}\\body",   // OSC terminated by ST
    ];
    for case in corpus {
        assert_eq!(
            console::measure_text_width(case),
            console::measure_text_width(&console::strip_ansi_codes(case)),
            "pre-stripping changes the measured width of {case:?}"
        );
    }
}

#[test]
fn visible_len_sees_through_ansi_codes() {
    let cases = [
        "\u{1b}[38;5;208mthis is my text\u{1b}[0m", "\u{1b}[30mthis is my text\u{1b}[0m",
        "\u{1b}[31mthis is my text\u{1b}[0m", "\u{1b}[32mthis is my text\u{1b}[0m",
        "\u{1b}[33mthis is my text\u{1b}[0m", "\u{1b}[34mthis is my text\u{1b}[0m",
        "\u{1b}[35mthis is my text\u{1b}[0m", "\u{1b}[36mthis is my text\u{1b}[0m",
        "\u{1b}[37mthis is my text\u{1b}[0m", "\u{1b}[90mthis is my text\u{1b}[0m",
        "\u{1b}[91mthis is my text\u{1b}[0m", "\u{1b}[92mthis is my text\u{1b}[0m",
        "\u{1b}[93mthis is my text\u{1b}[0m", "\u{1b}[94mthis is my text\u{1b}[0m",
        "\u{1b}[95mthis is my text\u{1b}[0m", "\u{1b}[96mthis is my text\u{1b}[0m",
        "\u{1b}[97mthis is my text\u{1b}[0m", "\u{1b}[40mthis is my text\u{1b}[0m",
        "\u{1b}[41mthis is my text\u{1b}[0m", "\u{1b}[42mthis is my text\u{1b}[0m",
        "\u{1b}[43mthis is my text\u{1b}[0m", "\u{1b}[44mthis is my text\u{1b}[0m",
        "\u{1b}[45mthis is my text\u{1b}[0m", "\u{1b}[46mthis is my text\u{1b}[0m",
        "\u{1b}[47mthis is my text\u{1b}[0m", "\u{1b}[100mthis is my text\u{1b}[0m",
        "\u{1b}[101mthis is my text\u{1b}[0m", "\u{1b}[102mthis is my text\u{1b}[0m",
        "\u{1b}[103mthis is my text\u{1b}[0m", "\u{1b}[104mthis is my text\u{1b}[0m",
        "\u{1b}[105mthis is my text\u{1b}[0m", "\u{1b}[106mthis is my text\u{1b}[0m",
        "\u{1b}[107mthis is my text\u{1b}[0m", "\u{1b}[1mthis is my text\u{1b}[0m",
        "\u{1b}[2mthis is my text\u{1b}[0m", "\u{1b}[3mthis is my text\u{1b}[0m",
        "\u{1b}[4mthis is my text\u{1b}[0m", "\u{1b}[5mthis is my text\u{1b}[0m",
        "\u{1b}[6mthis is my text\u{1b}[0m", "\u{1b}[7mthis is my text\u{1b}[0m",
        "\u{1b}[8mthis is my text\u{1b}[0m", "\u{1b}[9mthis is my text\u{1b}[0m",
        "\u{1b}[22mthis is my text\u{1b}[0m", "\u{1b}[23mthis is my text\u{1b}[0m",
        "this is my text\u{1b}[0m", "\u{1b}[46m\u{1b}[23mthis is my text\u{1b}[0m",
        "this is my text",
    ];

    for case in cases {
        assert_eq!(visible_len(case), "this is my text".len(), "{case:?}");
    }
}

// ——— Numeric parsing & classification ————————————————————————————————————

#[test]
fn test_is_numeric_or_neutral() {
    let numeric = [
        "10.0", "123", "123K", "123.45M", "2MB", "-1.23Gi", "5TiB", "1K", "1k", "2.5G",
        "10MiB", "4.5", "2.000", "5 TiB", "+12.5", "10%", "2k%", "1.3 k", "1.12 kb/s",
        "2 MB/s", "4.4GB/s", "4K", "1080p", "60Hz", "1440p@120Hz"
    ];

    let non_numeric = [
        "abc", "1.2X", "1.2.3", "1 0", "2/2", "kB", "2%k", "1440p@Hz", "5950X"
    ];

    for val in numeric {
        assert!(is_numeric_or_neutral(val), "{} should be numeric", val);
    }

    for val in non_numeric {
        assert!(!is_numeric_or_neutral(val), "{} should not be numeric", val);
    }
}

#[test]
fn parse_numeric_applies_scales_and_units() {
    let cases: &[(&str, f64)] = &[
        // plain numbers
        ("123", 123.0), ("4.5", 4.5), ("+12.5", 12.5), ("-2", -2.0), ("2.000", 2.0),
        // SI scales, either case, space tolerated
        ("1k", 1e3), ("3.5K", 3.5e3), ("2M", 2e6), ("2.5G", 2.5e9), ("1T", 1e12),
        ("1.3 k", 1.3 * 1e3), ("4K", 4e3), ("2k%", 2e3),
        // binary (1024-based) scales, with and without the B
        ("2Ki", 2048.0), ("10MiB", 10.0 * 1024f64.powi(2)),
        ("-1.23Gi", -1.23 * 1024f64.powi(3)), ("5 TiB", 5.0 * 1024f64.powi(4)),
        // rates, percentages, frequencies
        ("1.12 kb/s", 1.12 * 1e3), ("2 MB/s", 2e6), ("4.4GB/s", 4.4 * 1e9),
        ("10%", 10.0), ("60Hz", 60.0),
        // 'p' is pixels, not peta: the number stands as-is
        ("1080p", 1080.0), ("720p", 720.0), ("1440p@120Hz", 1440.0),
        // ANSI-colored numbers still parse
        ("\u{1b}[31m42\u{1b}[0m", 42.0),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_numeric(text), Some(*expected), "{text}");
    }

    // non-numbers and neutral markers carry no value
    for text in ["abc", "", "-", "--", "*", "1.2X", "1.2.3", "1 0", "2/2", "kB", "2%k", "5950X"] {
        assert_eq!(parse_numeric(text), None, "{text:?} must not parse");
    }
}

// ——— Input plumbing ——————————————————————————————————————————————————————

#[test]
fn test_read_lines_file_inline_and_reader() {
    use std::io::Cursor;

    // inline data (one line or many) is split as-is, never mistaken for a path
    assert_eq!(read_lines("a  b").unwrap(), to_strings(&["a  b"]));
    assert_eq!(read_lines("x\ny").unwrap(), to_strings(&["x", "y"]));

    // an existing file is read
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(&temp_file, "one\ntwo\n").unwrap();
    assert_eq!(read_lines(temp_file.path().to_str().unwrap()).unwrap(), to_strings(&["one", "two"]));

    // the reader path (used for stdin + files) decodes lossily rather than erroring
    let lossy = read_from(Cursor::new(vec![b'a', 0xFF, b'\n', b'b'])).unwrap();
    assert_eq!(lossy, to_strings(&["a\u{fffd}", "b"]));
}
