#!/usr/bin/python3

"""
reorganizes table-intended text (such as in .csv or .wsv formats) so that columns align, for proper human readability
flags: --prepend: prefix the spaces instead of suffixing them
"""

# TODO - handle quotes better
#   lone double-quotes crash the program
#   maybe user doesn't want the quotes removed (during arg separation evaluation, by shlex.split()) - have a flag that adds-in quotes to "words" with spaces within (needs to be done early)
# TODO - accept a single space as non-separator (need at least 2, or a tab). That way a formatted table output would be the same as its re-formatting


import sys
import os
import shlex
import re

COLUMN_SPACING = '  '  # 2 spaces
NUMERICALLY_NEUTRAL = '-'


# allows spacing without consideration of special invisible characters (like coloring), so it'll align WITH colors
def strip_ansi(text):
    ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
    return ansi_escape.sub('', text)


def is_num_or_unit(input):
    pattern = r'^[+-]?[0-9]+(\.[0-9]+)?\s?[pKkMmGgTt]?((i?[bB]?(/s)?)|(%?)|((@[0-9]+)?(Hz)?))$'
    return bool(re.match(pattern, input))


def format_table(text: str, align_left=True):
    lines = text.splitlines()
    matrix = [shlex.split(line) for line in lines]

    col_word_lengths = {}  # column_index: max_word_length
    col_numerical_majority = {}  # column_index: int (positive means more numbers, negative means more strings)

    # get max length of words in each column
    for row in matrix:
        for j, word in enumerate(row):
            col_word_lengths[j] = max(col_word_lengths.get(j, 0), len(strip_ansi(word)))
            if word != NUMERICALLY_NEUTRAL:  # ignore word if it could be either a number or not
                col_numerical_majority[j] = col_numerical_majority.get(j, 0) + (1 if is_num_or_unit(word) else -1)

    def pad_word(a_word, index):
        removed_chars_count = len(a_word) - len(strip_ansi(a_word))  # char-count ignores colors and other unseen chars
        padding_total = col_word_lengths.get(index, 0) + removed_chars_count
        # numbers need to be RTL, because it makes the MSB (most significant bit) stand out rather than the LSB.
        # the config doesn't matter; if a number/neutral-char is in a majority-numerical column, align it right
        is_align_right_anyway = col_numerical_majority.get(index, 0) > 0 and (is_num_or_unit(a_word) or a_word == NUMERICALLY_NEUTRAL)
        return a_word.rjust(padding_total) if not align_left or is_align_right_anyway else a_word.ljust(padding_total)

    # pad all words to make columns uniform
    for i, row in enumerate(matrix):
        matrix[i] = [pad_word(word, i) for i, word in enumerate(row)]

    # Print the padded matrix without commas or brackets
    for row in matrix:
        print(COLUMN_SPACING.join(row))


if __name__ == "__main__":

    # checking for '--prepend' flag
    align_left = True  # by default, spaces are suffixed
    if '--prepend' in sys.argv:
        del sys.argv[sys.argv.index("--prepend")]  # get rid of the flag, so it won't cause problems later
        align_left = False

    # determining what's the data we'll work with
    input_text = ''
    if len(sys.argv) == 2 and sys.argv[1]:  # arg 0 is this file
        input_text = sys.argv[1]
        if os.path.isfile(input_text):
            with open(input_text, 'r') as file:
                input_text = file.read()
    elif not sys.stdin.isatty():
        input_text = sys.stdin.read().strip()  # stdin input, not args

    if not input_text:
        print("You need to provide text as an arg or stdin")
        exit(1)

    format_table(input_text, align_left)
    exit(0)

# Add tests below this line
import unittest


class TestingFunctionality(unittest.TestCase):
    SAMPLE_TABLE = """
num word a b long_word
1 one
2 "very long" a b c d e f
5k a b c
"""
    SAMPLE_OUTPUT = """
num  word       a  b  long_word
  1  one      
  2  very long  a  b  c          d  e  f
 5k  a          b  c
"""
    SAMPLE_RTL_OUTPUT = """
num       word  a  b  long_word
  1        one
  2  very long  a  b          c  d  e  f
 5k          a  b  c
"""

    CURRENT_FILE_PATH = os.path.abspath(__file__)

    def strip_ends(self, string):
        """ we're not picky enough (yet) to care about newlines at start or end """
        # TODO - be more picky
        return string.lstrip('\n').rstrip('\n')

    def assert_as_bash_cmd(self, bash_command, expected_output):
        import subprocess
        print(f"running: {bash_command}")
        result = subprocess.run(bash_command, shell=True, capture_output=True, text=True, executable='/bin/bash').stdout
        self.assertEqual(self.strip_ends(expected_output), self.strip_ends(result))

    def test_regular(self):
        self.assert_as_bash_cmd(f"'{self.CURRENT_FILE_PATH}' '{self.SAMPLE_TABLE}'", self.SAMPLE_OUTPUT)

    def test_prepended(self):
        self.assert_as_bash_cmd(f"'{self.CURRENT_FILE_PATH}' --prepend '{self.SAMPLE_TABLE}'", self.SAMPLE_RTL_OUTPUT)

    def test_prepended2(self):
        self.assert_as_bash_cmd(f"'{self.CURRENT_FILE_PATH}' '{self.SAMPLE_TABLE}' --prepend", self.SAMPLE_RTL_OUTPUT)

    def test_echoed(self):
        self.assert_as_bash_cmd(f"echo '{self.SAMPLE_TABLE}' | {self.CURRENT_FILE_PATH}", self.SAMPLE_OUTPUT)

    def test_streamed(self):
        self.assert_as_bash_cmd(f"'{self.CURRENT_FILE_PATH}' <<< '{self.SAMPLE_TABLE}'", self.SAMPLE_OUTPUT)

    def test_given_filename(self):
        import tempfile
        with tempfile.NamedTemporaryFile(delete=True, mode='w', encoding='utf-8') as temp_file:
            temp_file.write(self.SAMPLE_TABLE)
            temp_file.seek(0)
            self.assert_as_bash_cmd(f"'{self.CURRENT_FILE_PATH}' '{temp_file.name}'", self.SAMPLE_OUTPUT)

    def test_ansi_stripping(self):
        strings_w_ansi = [
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
            'this is my text',  # just checking that Identity functionality works
        ]
        for ansi_str in strings_w_ansi:
            print(f"testing: {ansi_str}")
            self.assertEqual('this is my text', strip_ansi(ansi_str))

    def test_is_number(self):
        numerical_inputs = ["123", "123K", "123.45M", "2MB", "-1.23Gi", "5TiB", "1K", "1k", "2.5G", "10MiB",  "4.5",
                            "2.000", "5 TiB", "+12.5", "10%", "2k%", "1.3 k", "1.12 kb/s", "2 MB/s", "4.4GB/s",
                            "4K", "1080p", "60Hz", "1440p@120Hz"
                            ]
        anumerical_inputs = ["abc", "1.2X", "1.2.3", "1 0", "2/2", "kB", "2%k", "1440p@Hz", "5950X" ]

        for num in numerical_inputs:
            print(f"testing num: {num}")
            self.assertTrue(is_num_or_unit(num))

        for non_num in anumerical_inputs:
            print(f"testing non-num: {non_num}")
            self.assertFalse(is_num_or_unit(non_num))

