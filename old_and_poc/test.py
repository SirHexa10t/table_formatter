#!/usr/bin/python3

import os
import io
import sys
import subprocess
import pytest

# import from a nearby file
from table_formatter import (  # module under test
    strip_ansi,
    TableFormatter,
    is_numeric_or_neutral,
)


# numerical column needs to align right
# extra excessive spaces need to be trimmed off
# tabs need to be deleted (including '	')
# 1-spaced words need to stay together
# colored word needs to avoid padding the whole column with invisible
sample_input = [
    "num  word\ta  long_word   b",
    "   1  one   ",
    "2  very long spaced  a  c  d  e	f\tg",
    "5k  a  b  c  [31mcolored[0m",
]
    
sample_output = [
    'num  word              a  long_word  b               ',
    '  1  one                                             ',
    '  2  very long spaced  a  c          d        e  f  g',
    ' 5k  a                 b  c          \x1b[31mcolored\x1b[0m         ',
]

smtouhou_data =  [
    '  #      Name            Lv.   HP      MP      ATK   DEF',
    '1      Reimu            40      193   211   63      82   ',
    '2      Marisa         28      125   166   46      57   ',
    '3      Shingyoku      89      620   505   202   182',
    '4      Yugenmagan   87      628   576   176   189',
    '5      Elis            78      495   448   215   145',
    '6      Sariel         90      690   630   164   217',
    '7      Mima            74      494   472   146   166',
]

smtouhou_data_organized =  [ 
    '#  Name        Lv.   HP   MP  ATK  DEF',
    '1  Reimu        40  193  211   63   82',
    '2  Marisa       28  125  166   46   57',
    '3  Shingyoku    89  620  505  202  182',
    '4  Yugenmagan   87  628  576  176  189',
    '5  Elis         78  495  448  215  145',
    '6  Sariel       90  690  630  164  217',
    '7  Mima         74  494  472  146  166',
]

@pytest.fixture
def script_path():
    return os.path.realpath(os.path.join(os.path.dirname(__file__), "table_formatter.py"))

# —— CLI tests ——

def test_piped_input(script_path):
    proc = subprocess.run(
        [sys.executable, script_path],
        input='\n'.join(sample_input),
        text=True,
        capture_output=True,
    )
    assert proc.returncode == 0
    assert proc.stdout.removesuffix('\n') == '\n'.join(sample_output)
    
def test_file_input(tmp_path, script_path):
    # Create a temporary file with sample_table content
    file_path = tmp_path / "input.txt"
    file_path.write_text('\n'.join(sample_input))

    # Run the script with the file as input
    proc = subprocess.run(
        [sys.executable, script_path, str(file_path)],
        text=True,
        capture_output=True,
    )

    assert proc.returncode == 0
    assert proc.stdout.removesuffix('\n') == '\n'.join(sample_output)
    
def test_directly():
    tf = TableFormatter(lines=sample_input)
    assert tf.format() == sample_output
    
    tf = TableFormatter(lines=smtouhou_data)
    assert tf.format() == smtouhou_data_organized
    
def test_solution_unchanging():
    tf = TableFormatter(lines=sample_output)
    assert tf.format() == sample_output         # an organized table should stay as-is
    
    tf = TableFormatter(lines=smtouhou_data_organized)
    assert tf.format() == smtouhou_data_organized

# ——— ANSI‐Stripping Tests —————————————————————————————————————————————————————
@pytest.mark.parametrize("ansi_str", [
    '\033[38;5;208mthis is my text\033[0m',
    '\033[30mthis is my text\033[0m',
    '\033[31mthis is my text\033[0m',
    '\033[32mthis is my text\033[0m',
    '\033[33mthis is my text\033[0m',
    '\033[34mthis is my text\033[0m',
    '\033[35mthis is my text\033[0m',
    '\033[36mthis is my text\033[0m',
    '\033[37mthis is my text\033[0m',
    '\033[90mthis is my text\033[0m',
    '\033[91mthis is my text\033[0m',
    '\033[92mthis is my text\033[0m',
    '\033[93mthis is my text\033[0m',
    '\033[94mthis is my text\033[0m',
    '\033[95mthis is my text\033[0m',
    '\033[96mthis is my text\033[0m',
    '\033[97mthis is my text\033[0m',
    '\033[40mthis is my text\033[0m',
    '\033[41mthis is my text\033[0m',
    '\033[42mthis is my text\033[0m',
    '\033[43mthis is my text\033[0m',
    '\033[44mthis is my text\033[0m',
    '\033[45mthis is my text\033[0m',
    '\033[46mthis is my text\033[0m',
    '\033[47mthis is my text\033[0m',
    '\033[100mthis is my text\033[0m',
    '\033[101mthis is my text\033[0m',
    '\033[102mthis is my text\033[0m',
    '\033[103mthis is my text\033[0m',
    '\033[104mthis is my text\033[0m',
    '\033[105mthis is my text\033[0m',
    '\033[106mthis is my text\033[0m',
    '\033[107mthis is my text\033[0m',
    '\033[1mthis is my text\033[0m',
    '\033[2mthis is my text\033[0m',
    '\033[3mthis is my text\033[0m',
    '\033[4mthis is my text\033[0m',
    '\033[5mthis is my text\033[0m',
    '\033[6mthis is my text\033[0m',
    '\033[7mthis is my text\033[0m',
    '\033[8mthis is my text\033[0m',
    '\033[9mthis is my text\033[0m',
    '\033[22mthis is my text\033[0m',
    '\033[23mthis is my text\033[0m',
    'this is my text\033[0m',
    'this is my text\x1b[0m',
    '\x1b[46m\x1b[23mthis is my text\x1b[0m',
    'this is my text',
])
def test_strip_ansi(ansi_str):
    assert strip_ansi(ansi_str) == 'this is my text'


# ——— Numeric‐Detection Tests ————————————————————————————————————————————————————
@pytest.mark.parametrize("val, expected", [
    ("123", True),
    ("123K", True),
    ("123.45M", True),
    ("2MB", True),
    ("-1.23Gi", True),
    ("5TiB", True), ("1K", True),
    ("1k", True),
    ("2.5G", True),
    ("10MiB", True),
    ("4.5", True),
    ("2.000", True),
    ("5 TiB", True),
    ("+12.5", True),
    ("10%", True),
    ("2k%", True),
    ("1.3 k", True),
    ("1.12 kb/s", True),
    ("2 MB/s", True),
    ("4.4GB/s", True),
    ("4K", True),
    ("1080p", True),
    ("60Hz", True),
    ("1440p@120Hz", True),
    
    ("abc", False),
    ("1.2X", False),
    ("1.2.3", False),
    ("1 0", False),
    ("2/2", False),
    ("kB", False),
    ("2%k", False),
    ("1440p@Hz", False),
    ("5950X", False),
])
def test_is_numeric_or_neutral(val, expected):
    assert is_numeric_or_neutral(val) is expected
    
    