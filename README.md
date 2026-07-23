# table_formatter

A fast Rust CLI (and library) that aligns whitespace-delimited text into a neat table: columns padded to a uniform width, numeric columns right-aligned, text columns left-aligned. Pipe any ragged command output or file through it and get something you can actually read.

## Features

- **Align any delimited input** — by default, runs of 2+ spaces or any tabs count as column breaks, so multi-word cells with single spaces stay intact. Point `--divide-by " | "` at pipe-delimited (or any other) data.
- **Numbers line up right-aligned automatically** — a column counts as numeric even with units and scales: `3.5K`, `900M`, `2GiB/s`, `10%`, `60Hz`, `1080p` (`p` is pixels, not peta), plus neutral markers (`-`, `=`, `y`/`n`, empty).
- **Sort by any column** with `--sort <idx>`: numeric columns descending (biggest first), text ascending. The header row is auto-detected and kept on top; override with `--header` / `--no-header`.
- **ANSI-color transparent** — styled cells (`\x1b[32m…\x1b[0m`) never disturb layout, alignment, classification, or sort order; the escape codes just ride along. Emoji, CJK, and other wide glyphs align by their real terminal width.
- **Tables stitch together** — output lines are padded to full width (including the last column), so consecutive tables printed one after another share one visual grid. Opt out with `--remove-trailing-spaces`.
- **Fast on big inputs** — parallelized with rayon; ~300k rows format in a few tenths of a second. A benchmark harness ships in `experiments/`.
- **Embeddable** — use it as a Rust library: call `format_table` directly, or embed the clap `Args` in your own CLI and delegate to `run_with`.

## Tech Stack Setup

The only requirement is Rust (edition 2021; any recent stable toolchain works). Install it with the official [rustup](https://rustup.rs/) one-liner, which also installs `cargo`:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows, download `rustup-init.exe` from the same page. No other system configuration is needed. Verify the toolchain:

```sh
cargo --version
```

## How to Run

Build the release binary (first build compiles dependencies and takes a minute; later builds are fast):

```sh
cargo build --release
```

The binary lands at `target/release/table_formatter`. To put it on your `PATH`:

```sh
cargo install --path .
```

### CLI usage

Input comes from stdin, a file path, or an inline string — the first positional argument (`-` or empty means stdin):

```sh
$ printf '#   Name        Lv.   HP    MP\n1   Reimu     40    193   211\n2   Marisa   28   125    166\n3   Shingyoku    89   620  505\n' | table_formatter
#  Name       Lv.   HP   MP
1  Reimu       40  193  211
2  Marisa      28  125  166
3  Shingyoku   89  620  505
```

Sorting by a suffixed size column (descending, header pinned automatically):

```sh
$ printf 'file  size\nlogs.tar  1.2G\nnotes.txt  4K\nbackup.img  900M\nvideo.mp4  2.1G\n' | table_formatter --sort 1
file        size
video.mp4   2.1G
logs.tar    1.2G
backup.img  900M
notes.txt     4K
```

Re-parsing pipe-delimited input and rendering it back with aligned pipes:

```sh
$ printf 'name | size\nlogs.tar | 1.2G\nnotes.txt | 4K\n' | table_formatter --divide-by ' | ' --join-with ' | '
name      | size
logs.tar  | 1.2G
notes.txt |   4K
```

Round-tripping a Markdown-style table — the `| … |` frame is peeled on input and re-emitted with `--emit-frame`, so the result is stable if fed back in:

```sh
$ printf '| name | size |\n| logs.tar | 1.2G |\n| notes.txt | 4K |\n' | table_formatter -d ' | ' -j ' | ' --emit-frame
| name      | size |
| logs.tar  | 1.2G |
| notes.txt |   4K |
```

All options:

| Flag | Effect |
|---|---|
| `-d, --divide-by <STR>` | column delimiter in the **input** (default `"  "`); must have leading + trailing whitespace, so `" \| "` is valid but `"\|"` is not. Whitespace around the core is flexible. |
| `-j, --join-with <STR>` | string placed between columns in the **output** (default `"  "`); same whitespace requirement, so the result stays re-parseable. |
| `--sort <IDX>` | sort by 0-based column; numeric descending, text ascending |
| `--header` / `--no-header` | force the first row to be (or not be) a pinned header; default auto-detects |
| `--remove-trailing-spaces` | trim the padding after the last column (disables table stitching) |
| `--emit-frame` | wrap each output line in the `--join-with` edge characters, e.g. `\| … \|` for `--join-with " \| "` — emitting a framed (Markdown-style) table. Mutually exclusive with `--remove-trailing-spaces` (the frame needs that padding to stay aligned). |

Both delimiters require leading and trailing whitespace, and `--emit-frame` can't be combined with `--remove-trailing-spaces`. Errors are reported cleanly — e.g. `--join-with '|'` prints `table_formatter: --join-with "|" must have leading and trailing whitespace (e.g. " | ")` to stderr and exits non-zero.

### Library usage

The crate isn't on crates.io; add it as a path or git dependency, then either format lines directly:

```rust
use table_formatter::{format_table, FormatOptions};

let lines: Vec<String> = std::fs::read_to_string("data.txt")?
    .lines().map(String::from).collect();
let opts = FormatOptions { sort: Some(1), ..Default::default() };
for line in format_table(&lines, &opts)? {
    println!("{line}");
}
```

…or embed the whole CLI in your own clap interface: include `table_formatter::Args` as a subcommand and pass it to `run_with(args)` (see the doc comments on `run`, `run_from`, and `run_with` in `src/lib.rs`).

### Tests and benchmarks

```sh
cargo test                                # 35 tests: goldens, sorting, ANSI/Unicode invariants
experiments/benchmark.sh my-label         # timed scenarios → experiments/bench_<date>_my-label.txt
```

The benchmark harness generates its own fixtures and covers large-file throughput, suffixed-numeric sorting, ANSI-colored input, and interactive startup latency. Reports from past runs live in `experiments/` for before/after comparison.

## License

[GPL-3.0](LICENSE)
