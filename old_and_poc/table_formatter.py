#!/usr/bin/python3

import argparse
import re
import sys

# ——— Configuration ——————————————————————————————
DEFAULT_SEPARATOR = 2

# Split on 2+ spaces OR 1+ tabs
SPLIT_PATTERN = re.compile(r"\s{2,}|\t+")

# Strip ANSI escape codes
ANSI_ESCAPE = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')

# Regex for detecting numeric values with units
NUMERIC_PATTERN = re.compile(
    r'^[+-]?[0-9]+(?:\.[0-9]+)?'         # integer or decimal number
    r'\s?[pKkMmGgTt]?'                   # optional prefix (e.g. 5K, 10M)
    r'(?:i?[bB]?(/s)?|%|Hz|@[0-9]+Hz)?$' # units: MiB, %, Hz, @60Hz
)

NEUTRAL_VALUES = {'', '-', '--', '---', '*', '−', '=', 'y', 'n'}


# ——— Utilities ——————————————————————————————————————
def strip_ansi(text: str) -> str:
    return ANSI_ESCAPE.sub('', text)


def is_numeric_or_neutral(text: str) -> bool:
    return (clean := strip_ansi(text).strip()) in NEUTRAL_VALUES or bool(NUMERIC_PATTERN.match(clean))


def split_row(line: str) -> list[str]:
    return SPLIT_PATTERN.split(line.strip())


def detect_column_properties(rows: list[list[str]]) -> tuple[list[int], list[bool]]:
    if not rows:
        return [], []

    num_cols = max(map(len, rows))
    is_numeric = [True] * num_cols
    widths = [0] * num_cols

    # Determine column widths
    for row_index, row in enumerate(rows):
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(strip_ansi(cell)))                           # largest value found
            is_numeric[i] &= (is_numeric_or_neutral(cell) if row_index > 0 else True)   # false if any value isn't numeric. Don't check numeric if on first row, i.e. header

    return widths, is_numeric
    

def format_row(cells: list[str], widths: list[int], is_numeric: list[bool], sep_width: int) -> str:
    spacer = ' ' * sep_width
    formatted = []

    for i, width in enumerate(widths):
        cell = cells[i] if i < len(cells) else ''
        vis_len = len(strip_ansi(cell))
        pad = (width - vis_len) * ' '
        if is_numeric[i]:
            formatted.append(pad + cell)  # right-align
        else:
            formatted.append(cell + pad)  # left-align

    return spacer.join(formatted)


# ——— Formatter class ——————————————————————————————
class TableFormatter:
    def __init__(self, lines: list[str], separator: int = DEFAULT_SEPARATOR):
        self.separator = separator
        self.rows: list[list[str]] = [split_row(line) for line in lines]

    def format(self) -> list[str]:
        widths, is_numeric = detect_column_properties(self.rows)
        return [format_row(row, widths, is_numeric, self.separator) for row in self.rows]

    def print(self) -> None:
        for line in self.format():
            print(line)


# ——— CLI Entry ———————————————————————————————————————
def main():
    parser = argparse.ArgumentParser(
        description="Align whitespace-delimited columns into a neat table."
    )
    parser.add_argument(
        'input',
        nargs='?',
        type=argparse.FileType('r'),
        default=sys.stdin,
        help='Input file path or STDIN'
    )
    parser.add_argument(
        '-s', '--separator',
        type=int,
        default=DEFAULT_SEPARATOR,
        help='Number of spaces to separate columns'
    )
    args = parser.parse_args()
    lines = args.input.readlines()

    formatter = TableFormatter(lines=lines, separator=args.separator)
    formatter.print()


if __name__ == "__main__":
    main()
