use std::fs::File;
use assert_cmd::Command;
use crate::{format_table, strip_ansi, is_numeric_or_neutral, DEFAULT_SEPARATOR};
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

const SPECIAL_CHARS_ORGANIZED: &[&str] = &[
    "A   B",
    "1   x",
    "🌎   X",
    "🇺🇸  X",
    "3   X",
];

fn to_strings(arr: &[&str]) -> Vec<String> {
    arr.iter().map(|s| s.to_string()).collect()
}

fn assert_cmd_and_print(command: &mut Command) -> Vec<String> {
    let output = command.output()
        .expect("failed to execute process");

    assert!(
        output.status.success(),
        "program exited with error: {}\n--- stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect::<Vec<_>>()
}

fn run_with_cmdline_arg(arg: &str) -> Vec<String> {
    assert_cmd_and_print(
        Command::cargo_bin("table_formatter").unwrap()
            .arg(arg)
    )
}

fn run_with_piped_data(piped: &str) -> Vec<String> {
    assert_cmd_and_print(
        Command::cargo_bin("table_formatter").unwrap()
            .write_stdin(piped)
    )
}
fn direct_test(input: &[&str], expected: &[&str]) {  // call the actual function directly
    assert_eq!(format_table(&to_strings(input), DEFAULT_SEPARATOR, None), to_strings(expected));
}

fn file_input_test(input: &[&str], expected: &[&str]) {  // run the program through its bin-file and provide a temp-file
    use tempfile::NamedTempFile;
    use std::fs;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(&temp_file, input.join("\n")).unwrap();

    let result = run_with_cmdline_arg(temp_file.path().to_str().unwrap());

    assert_eq!(result, to_strings(expected));
}

fn string_input_test(input: &[&str], expected: &[&str]) {
    let result = run_with_cmdline_arg(&input.join("\n"));
    assert_eq!(result, to_strings(expected));
}

fn piped_input_test(input: &[&str], expected: &[&str]) {
    let result = run_with_piped_data(&input.join("\n"));
    assert_eq!(result, to_strings(expected));
}

fn check_immutability_on_2nd_run(input: &[&str]) {  // input is a pre-organized table. There's nothing to further organize.
    assert_eq!(format_table(&to_strings(input), DEFAULT_SEPARATOR, None), to_strings(input));
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
    let result = run_with_cmdline_arg(input_file);

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

    let result = run_with_cmdline_arg(input_file);

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

    assert_eq!(format_table(&to_strings(VARYING_LENGTH_TABLE), DEFAULT_SEPARATOR, Some(0)), to_strings(VARYING_LENGTH_TABLE_SORT0_ORGANIZED));
    assert_eq!(format_table(&to_strings(VARYING_LENGTH_TABLE), DEFAULT_SEPARATOR, Some(1)), to_strings(VARYING_LENGTH_TABLE_SORT1_ORGANIZED));


    const SORT_TESTER: &[&str] = &[
        "X     X     X",
        "2  1000    2M",
        "3     9  3.5K",
        "4     5    9G",
        "5     6    3G",
        "6     8   10T",
        "7     9  288M",
    ];
    const SORT_TESTER_SORT1: &[&str] = &[
        "X     X     X",
        "2  1000    2M",
        "7     9  288M",
        "3     9  3.5K",
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

    assert_eq!(format_table(&to_strings(SORT_TESTER), DEFAULT_SEPARATOR, Some(1)), to_strings(SORT_TESTER_SORT1));
    assert_eq!(format_table(&to_strings(SORT_TESTER), DEFAULT_SEPARATOR, Some(2)), to_strings(SORT_TESTER_SORT2));

}

#[test]
fn test_strip_ansi() {
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
        assert_eq!(strip_ansi(case), "this is my text");
    }
}

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
