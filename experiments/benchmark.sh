#!/usr/bin/env bash
# Benchmark harness for table_formatter.
#
# Usage:  experiments/benchmark.sh <label>
#
# Builds the release binary, generates deterministic fixtures in a temp dir,
# times the core scenarios, and writes the report to
#   experiments/bench_<date>_<label>.txt
#
# Scenarios cover the paths that matter for runtime:
#   1. large-file formatting throughput (parallel and single-threaded)
#   2. sorting by a plain-integer column vs a suffixed-numeric column (3.5K, 2M, ...)
#   3. many small invocations (interactive/startup latency)
#
# Note: fixture values come from awk's rand(), whose sequence differs between awk
# implementations. Shape and distribution are identical everywhere, so reports are
# comparable across runs on the same machine — which is what before/after needs.
set -euo pipefail

cd "$(dirname "$0")/.."
LABEL="${1:?usage: experiments/benchmark.sh <label>}"
OUT="experiments/bench_$(date +%F)_${LABEL}.txt"
BIN=target/release/table_formatter

cargo build --release --quiet
mkdir -p experiments
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ——— fixtures ————————————————————————————————————————————————————————————
# 300k rows, plain numbers + words: formatting throughput
awk 'BEGIN{
  srand(42); split("alpha beta gamma delta epsilon zeta_longer_word eta theta", W, " ");
  print "name  count  size  ratio  label";
  for (i = 0; i < 300000; i++)
    printf "%s   %d    %d   %.2f  %s\n",
      W[int(rand()*8)+1], int(rand()*100000), int(rand()*900)+1, rand()*100, W[int(rand()*8)+1];
}' > "$WORK/big_plain.txt"

# 30k rows; column 1 is plain integers, column 2 carries K/M/G/T suffixes
awk 'BEGIN{
  srand(42); split("alpha beta gamma delta epsilon zeta_longer_word eta theta", W, " ");
  split("K M G T", S, " ");
  print "name  count  size";
  for (i = 0; i < 30000; i++)
    printf "%s  %d  %.1f%s\n",
      W[int(rand()*8)+1], int(rand()*100000), rand()*100, S[int(rand()*4)+1];
}' > "$WORK/sort_input.txt"

# 12 rows: typical interactive input
awk 'BEGIN{
  srand(42); split("alpha beta gamma delta epsilon zeta_longer_word eta theta", W, " ");
  print "name  count  size";
  for (i = 0; i < 12; i++)
    printf "%s  %d  %d\n", W[int(rand()*8)+1], int(rand()*1000), int(rand()*90)+1;
}' > "$WORK/small.txt"

# ANSI-styled variants: ~40% of cells wrapped in color codes (worst-case-ish styling;
# real colored tables usually style far fewer cells)
color_cells() { # color_cells <in> <out>
  awk 'BEGIN{ srand(7); split("31 32 33 34 35 36", C, " ") }
  {
    out = "";
    for (f = 1; f <= NF; f++) {
      cell = $f;
      if (rand() < 0.4) cell = "\033[" C[int(rand()*6)+1] "m" cell "\033[0m";
      out = out (f > 1 ? "  " : "") cell;
    }
    print out;
  }' "$1" > "$2"
}
color_cells "$WORK/big_plain.txt"  "$WORK/big_colored.txt"
color_cells "$WORK/sort_input.txt" "$WORK/sort_colored.txt"

# ——— timing helpers ——————————————————————————————————————————————————————
TIMEFORMAT='  real %3R  user %3U  sys %3S'

scenario() { # scenario <title> <runs> <command...>
  local title="$1" runs="$2"; shift 2
  echo
  echo "## $title  (${runs} run(s))"
  for _ in $(seq "$runs"); do
    { time "$@" > /dev/null; } 2>&1
  done
}

# ——— report ——————————————————————————————————————————————————————————————
{
  echo "# table_formatter benchmark — $LABEL"
  echo "date:    $(date +%F)"
  echo "commit:  $(git rev-parse --short HEAD)$(git diff --quiet || echo ' (dirty)')"
  echo "cpu:     $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //' || echo unknown), $(nproc) cores"

  scenario "300k rows, format only"                    3 "$BIN" "$WORK/big_plain.txt"
  scenario "300k rows, format only, RAYON_NUM_THREADS=1" 3 \
      env RAYON_NUM_THREADS=1 "$BIN" "$WORK/big_plain.txt"
  scenario "300k rows, ~40% cells ANSI-colored, format only" 3 "$BIN" "$WORK/big_colored.txt"
  scenario "30k rows, sort by plain-integer column"    3 "$BIN" "$WORK/sort_input.txt" --sort 1
  scenario "30k rows, sort by suffixed column (K/M/G/T)" 1 "$BIN" "$WORK/sort_input.txt" --sort 2
  scenario "30k rows, sort by text column"             3 "$BIN" "$WORK/sort_input.txt" --sort 0
  scenario "30k rows, ~40% colored, sort by text column" 3 "$BIN" "$WORK/sort_colored.txt" --sort 0
  scenario "12-row file, 200 sequential invocations"   2 \
      bash -c "for i in \$(seq 200); do \"$BIN\" \"$WORK/small.txt\" > /dev/null; done"
} | tee "$OUT"

echo
echo "report saved to $OUT"
