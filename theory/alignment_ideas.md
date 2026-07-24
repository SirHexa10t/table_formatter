# Alignment & splitting — deferred designs

Ideas discussed during the split/alignment work (2026-07-23) that were deliberately **not**
built, recorded so they can be revisited without re-deriving them. Each entry says what it
is, why it was deferred, and what would trigger picking it back up.

## 1. Optimal column-width allocation (replacing the greedy)

**Today:** `allocate_caps` in `src/split.rs` is a deterministic two-phase greedy — shrink
the column that adds the fewest estimated visual lines per character reclaimed (ties:
widest, then leftmost); phase 1 never shrinks a column below its widest word, phase 2
breaks words only if the budget still can't be met. Line-count is estimated as
`ceil(width / cap)` per cell, max over the record, summed.

**The full problem:** choose a cap per column, minimizing total visual lines, subject to
`Σ caps + overhead ≤ budget`. Two properties make it genuinely hard:

- A record's height is the **max** over its columns' fragment counts — so shrinking the
  tallest column often buys nothing (another column keeps the record tall), while shrinking
  a *medium* column can flip several records from 2 lines to 1. Non-convex coupling.
- Real break points are **token-determined** (words), not continuous — `ceil(w/cap)` is an
  approximation of true word-wrapping.

**The cost/debt model (owner's sketch):** each extra visual line costs 1 (global `cost`);
each record has a `debt` (overflowing chars, uint); each cell has `liquidity` (its own
width vs. the column's width); each column has `assets` (column width minus its largest
token). If total assets can't cover a record's debt, that record *will* overflow — accept
it and minimize overflow where it adds no cost. Target: zero debt everywhere at minimum
cost. A fuzzy/local search over this model is a plausible next allocator.

**On LP/simplex:** considered; the max-coupling and discrete token break points don't
linearize cleanly, so a discrete search (branch-and-bound over per-column breakpoints,
or the fuzzy search above) is expected to model it more faithfully than a relaxation.

**Also:** replace `est_lines` with a word-aware estimator (count real wrap points instead
of `ceil(w/cap)`) — cheaper than a full search, tightens the greedy's decisions.

**Interface contract:** any new allocator drops in behind `allocate_caps(rows, natural,
budget) -> caps`; determinism is the hard requirement, optimality is soft. Keep the
"shrink a medium column on purpose" scenario as a regression test when upgrading.

**CHOSEN & IMPLEMENTED (2026-07-23) — the breakpoint-walking greedy.** The owner proposed
the concrete instance and it replaced the 1-char-decrement greedy:

- A *move* lowers one column's cap to just below its current width; the whole top-rank
  tie group of cells re-wraps at once, and the column tightens to the new widest cell —
  i.e., the search steps between *achievable* word-breakpoint widths. The owner's ranked
  per-column cell-width list is implicit in that re-wrap + max (no separate structure).
- **Free splits first**: a split costs no lines when a sibling column already holds the
  record tall; such moves are pure profit and win outright (cost 0 in the ratio order).
- Priced moves: fewest added lines per reclaimed character (cross-multiplied), ties to
  the larger reclaim, then leftmost column. All metrics measured with the real
  `wrap_cell` (`wrap_metrics`), so search and render can never disagree.
- Words stay whole for as long as words *can* fit; the old character-shrink survives only
  as the **hyphenation fallback** for when every column bottoms out at its widest word.
- Complexity stance (owner): the exact problem is likely NP-hard; deterministic
  close-enough is the goal, perfection is not worth its cost.

**Also implemented (same day):**
- **Candidate caching** (owner's incremental-tracking requirement): a column's down-move
  trial depends only on its own cap, so trials are cached and invalidated only when that
  column moves; per-iteration work is then O(records × columns) height bookkeeping, with
  re-wraps only for the column that changed. (Batteries split: 102ms → 26ms.)
- **Post-fit relaxation** (owner's grow-back idea): the last down-move can overshoot — a
  breakpoint jump may reclaim more width than the budget needed. `relax_into_slack`
  spends the leftover slack growing columns wherever extra width eliminates visual lines
  (most lines per char, ties: more lines, leftmost, narrower), never past natural width
  or the budget. Note: for a 2-word cell the merge cost equals its original split gain,
  so relaxation pays off mainly with 3+-word cells and multi-column interactions.
- The allocator lives in its own file, `src/split/alloc.rs`, with its own tests.

**Still open here:** a word-aware `est_lines` for the *fallback* phase.

**Revisit when:** wrapped output looks obviously wasteful (too many visual lines for the
budget) on real tables, or a wide-many-column workload makes the greedy's misses visible.

## 2. Byte-exact reversibility — sentinel-padded fragments ("dot-pad")

**Today:** fragments are padded with spaces (clean look). Consequences: runs of multiple
spaces inside a cell normalize to one space across a split→unsplit round trip, and unsplit
can't distinguish content spaces from padding at wrap points. (Forced mid-word breaks are
already byte-exact via the `‐` break hyphen.)

**The idea (owner's):** pad short/empty fragments with the sentinel instead of spaces, and
keep sentinels *inside* the grid. Unsplit then trims each column's fragments by the
sentinel — never by spaces — so every real space, including multi-space runs and
wrap-point spaces, survives exactly. Split→unsplit becomes byte-exact for all content.

**Cost:** the split view gets visibly dotty (sentinel padding throughout, not just in the
gutter and empty slots). Perhaps as an opt-in flag (`--exact-split`?), keeping the clean
scheme as the default.

**Revisit when:** whitespace-normalization at wrap points actually bites someone, or the
round trip is used as storage (not just display).

## 3. ANSI SGR re-injection per fragment (colour bleed across wraps)

**Today:** a colour opened in one fragment and closed in a later one bleeds across the
wrapped lines between them — escapes ride along byte-exactly, but the *rendered* colouring
of padding/other columns on continuation lines can look wrong.

**The idea:** track active SGR state through each cell; at every fragment boundary, close
(reset) at the end of the line and re-open the active state at the start of the next
fragment. Rendered output looks right on every line.

**Tension:** injected escapes make the split bytes differ from the original codes, so
unsplit must strip exactly the injected ones to stay reversible — doable (they're at known
positions: fragment start/end) but it couples rendering and unsplitting more tightly.

**Revisit when:** split coloured tables are a common use and the bleed is visible enough
to annoy.

## 4. Multi-line *input* (the set-aside half of the original feature)

The split work solved split-lines *output*. The original discussion also covered cells that
are already split-lines in the source; deliberately set aside ("assume input can be
prepared ahead of time"). The candidate designs, still on the table:

- **Escape token in a cell** (e.g. a literal `\n` or a configurable token): stays 1 line =
  1 record, so the parallel zero-copy pipeline is untouched; round-trips in token form;
  an `--expand` display mode could render it stacked. Cheapest to add — the render side
  (split-lines stacking) now exists.
- **Continuation lines with a sentinel first cell** (the old TODO's `.` idea): most natural
  input, but needs a sequential grouping pass (breaks 1:1 line↔record) and a column-mapping
  convention for which columns a continuation feeds. The split gutter is this idea's output
  twin — a future input format could simply *be* split output (gutter + placeholders),
  making "split-lines input" = "accept split tables as input", which `--unsplit` already
  half-implements.

**Revisit when:** real source data with split-lines cells shows up.

## 5. Smaller notes

- **Vertical alignment of stacked fragments:** top-aligned today; bottom/center are
  possible options if tables with tall neighbours read badly.
- **Width-1 columns can't carry the break hyphen** — a mid-word break there is ambiguous
  on unsplit. Inherent to a 1-column budget; documented, low value to fix.
- **Marker/data collisions** (a data cell that *is* the sentinel; a data word ending in the
  break hyphen at a fragment end) are pinned as documented-limitation tests. A stricter
  mode could *reject* the split instead (error: "data contains the sentinel; pick another
  via --sentinel") if silent-ish behavior ever becomes a problem.
- **Rejected, on purpose — positional reverse parsing:** recovering columns on unsplit by
  the head line's character positions (no placeholders needed). Rejected because this
  project is built on whitespace-robust parsing, never column positions; recorded here so
  it isn't re-proposed.
