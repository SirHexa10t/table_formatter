//! Column-width allocation for splitting — the owner's breakpoint-walking greedy
//! (design record: theory/alignment_ideas.md §1).
//!
//! Given natural column widths and a content budget, [`allocate_caps`] picks a width cap
//! per column in three phases:
//!
//! 1. **Walk down** word breakpoints: a move lowers one column's cap to just below its
//!    current width, forcing its whole top-rank tie group of cells to re-wrap at once;
//!    the column then tightens to the new widest cell — the next *achievable* width.
//!    Free moves first (a split costs no lines when a sibling column already holds the
//!    record tall), then fewest added lines per reclaimed character.
//! 2. **Hyphenation fallback**, only when every column has bottomed out at its widest
//!    word and the budget still isn't met ([`shrink_to_fit`] — the render then
//!    hard-breaks words with the break hyphen).
//! 3. **Relax back up**: the final down-move may overshoot (a breakpoint jump can reclaim
//!    more width than was needed); spend the leftover slack growing columns wherever the
//!    extra width eliminates visual lines — best lines-per-char first.
//!
//! Everything is measured with the real [`wrap_cell`] (via [`wrap_metrics`]), so the
//! search can never disagree with the renderer. Deterministic throughout; greedy on
//! purpose — the exact problem is combinatorial, and close-enough beats perfect here.

use super::wrap_cell;
use crate::visible_len;

/// (widest rendered fragment — hyphen included — and fragment count) for `cell` wrapped
/// at `cap`, measured with the real [`wrap_cell`].
fn wrap_metrics(cell: &str, cap: usize) -> (usize, usize) {
    let frags = wrap_cell(cell, cap);
    let width =
        frags.iter().map(|f| visible_len(f.text) + usize::from(f.hyphen)).max().unwrap_or(0);
    (width, frags.len())
}

/// Per-record height statistics across columns: (tallest, how many columns tie for it,
/// second-tallest). Lets a candidate's cost be priced in O(1) per record: the height
/// "without column c" is `top1` unless `c` alone holds it, in which case `top2`.
struct HeightStats {
    top1: usize,
    top1_count: usize,
    top2: usize,
}

fn height_stats(metrics: &[Vec<(usize, usize)>], record: usize) -> HeightStats {
    let (mut top1, mut top1_count, mut top2) = (1usize, 0usize, 1usize);
    for column in metrics {
        let rows = column[record].1;
        if rows > top1 {
            top2 = top1;
            top1 = rows;
            top1_count = 1;
        } else if rows == top1 {
            top1_count += 1;
        } else if rows > top2 {
            top2 = rows;
        }
    }
    HeightStats { top1, top1_count, top2 }
}

/// A cached down-move for one column: what happens at `cap - 1`. Only invalidated when
/// that column itself moves — other columns' moves change *costs* (heights), never these
/// wraps — so each column is re-wrapped once per cap change, not once per iteration.
struct Candidate {
    new_cap: usize,
    gain: usize,
    trial: Vec<(usize, usize)>,
}

/// Choose a width cap per column so `Σ caps ≤ budget`. See the module docs for the
/// three-phase design.
pub(super) fn allocate_caps(rows: &[Vec<&str>], natural: &[usize], budget: usize) -> Vec<usize> {
    let cols = natural.len();
    let mut caps = natural.to_vec();
    if caps.iter().sum::<usize>() <= budget {
        return caps; // already fits — no wrapping
    }
    let floors = word_floors(rows, cols);
    let column_metrics = |cap: usize, c: usize| -> Vec<(usize, usize)> {
        rows.iter().map(|row| wrap_metrics(row.get(c).copied().unwrap_or(""), cap)).collect()
    };

    // ——— Phase 1: walk down the word breakpoints ———
    let mut metrics: Vec<Vec<(usize, usize)>> =
        (0..cols).map(|c| column_metrics(caps[c], c)).collect();
    let mut cand: Vec<Option<Candidate>> = (0..cols).map(|_| None).collect();

    while caps.iter().sum::<usize>() > budget {
        let stats: Vec<HeightStats> =
            (0..rows.len()).map(|r| height_stats(&metrics, r)).collect();

        let mut best: Option<(usize, usize, usize)> = None; // (cost, gain, col)
        for c in 0..cols {
            if caps[c] <= floors[c] {
                continue; // at its widest word — only hyphenation could go lower
            }
            if cand[c].is_none() {
                let trial = column_metrics(caps[c] - 1, c);
                let new_cap = trial.iter().map(|m| m.0).max().unwrap_or(0);
                // gain ≥ 1: the binders were forced below the old width
                cand[c] = Some(Candidate { new_cap, gain: caps[c] - new_cap, trial });
            }
            let candidate = cand[c].as_ref().unwrap();
            let cost: usize = (0..rows.len())
                .map(|r| {
                    let s = &stats[r];
                    let without_c = if metrics[c][r].1 == s.top1 && s.top1_count == 1 {
                        s.top2
                    } else {
                        s.top1
                    };
                    without_c.max(candidate.trial[r].1).saturating_sub(s.top1)
                })
                .sum();
            // Fewest lines per reclaimed char, by cross-multiplication; free moves
            // (cost 0) always win. Strict `<` keeps ties on the leftmost column.
            let better = match best {
                None => true,
                Some((bcost, bgain, _)) => {
                    cost * bgain < bcost * candidate.gain
                        || (cost * bgain == bcost * candidate.gain && candidate.gain > bgain)
                }
            };
            if better {
                best = Some((cost, candidate.gain, c));
            }
        }

        match best {
            Some((_, _, c)) => {
                let chosen = cand[c].take().unwrap();
                caps[c] = chosen.new_cap;
                metrics[c] = chosen.trial;
            }
            None => break, // every column is at its word floor — words alone can't fit
        }
    }

    // ——— Phase 2: stuck above budget with whole words — hyphenate as the last resort ———
    if caps.iter().sum::<usize>() > budget {
        let cell_w: Vec<Vec<usize>> = rows
            .iter()
            .map(|row| (0..cols).map(|c| row.get(c).map_or(0, |s| visible_len(s))).collect())
            .collect();
        let ones = vec![1usize; cols];
        shrink_to_fit(&mut caps, &cell_w, budget, &ones);
    }

    // ——— Phase 3: spend any leftover slack where it removes visual lines ———
    relax_into_slack(rows, natural, &mut caps, budget);
    caps
}

/// The final down-move may have overshot (a breakpoint jump can reclaim more width than
/// the budget needed). Grow columns back into the leftover slack wherever extra width
/// eliminates visual lines — most lines saved per char spent first (ties: more lines,
/// then leftmost, then narrower growth). Never grows past a column's natural width or
/// the budget.
fn relax_into_slack(rows: &[Vec<&str>], natural: &[usize], caps: &mut [usize], budget: usize) {
    let cols = caps.len();
    let column_metrics = |cap: usize, c: usize| -> Vec<(usize, usize)> {
        rows.iter().map(|row| wrap_metrics(row.get(c).copied().unwrap_or(""), cap)).collect()
    };

    loop {
        let slack = budget.saturating_sub(caps.iter().sum::<usize>());
        if slack == 0 {
            return;
        }
        let metrics: Vec<Vec<(usize, usize)>> =
            (0..cols).map(|c| column_metrics(caps[c], c)).collect();
        let stats: Vec<HeightStats> =
            (0..rows.len()).map(|r| height_stats(&metrics, r)).collect();

        let mut best: Option<(usize, usize, usize, usize)> = None; // (saved, cost, col, width)
        for c in 0..cols {
            let hi = (caps[c] + slack).min(natural[c]);
            for w in caps[c] + 1..=hi {
                let trial = column_metrics(w, c);
                let saved: usize = (0..rows.len())
                    .map(|r| {
                        let s = &stats[r];
                        let without_c = if metrics[c][r].1 == s.top1 && s.top1_count == 1 {
                            s.top2
                        } else {
                            s.top1
                        };
                        s.top1.saturating_sub(without_c.max(trial[r].1))
                    })
                    .sum();
                if saved == 0 {
                    continue;
                }
                let cost = w - caps[c];
                let better = match best {
                    None => true,
                    Some((bsaved, bcost, _, _)) => {
                        saved * bcost > bsaved * cost || (saved * bcost == bsaved * cost && saved > bsaved)
                    }
                };
                if better {
                    best = Some((saved, cost, c, w));
                }
            }
        }

        match best {
            Some((_, _, c, w)) => caps[c] = w,
            None => return, // no growth within the slack removes a line
        }
    }
}

/// Widest single word (visible width) in each column — the smallest cap that avoids
/// breaking a word mid-way. At least 1.
fn word_floors(rows: &[Vec<&str>], num_cols: usize) -> Vec<usize> {
    let mut floor = vec![1usize; num_cols];
    for row in rows {
        for (c, cell) in row.iter().take(num_cols).enumerate() {
            for word in cell.split(' ') {
                floor[c] = floor[c].max(visible_len(word));
            }
        }
    }
    floor
}

/// Estimated total visual lines at these caps: per record, the max over columns of the
/// fragment count (a cell of width `w` in a column capped at `cap` needs `ceil(w/cap)`
/// lines). Only used by the hyphenation fallback, where caps go below word widths and
/// the ceil model is a fair stand-in for hard-broken pieces.
fn est_lines(cell_w: &[Vec<usize>], caps: &[usize]) -> usize {
    cell_w
        .iter()
        .map(|row| {
            row.iter()
                .zip(caps)
                .map(|(&w, &cap)| if w == 0 { 1 } else { w.div_ceil(cap) })
                .max()
                .unwrap_or(1)
        })
        .sum()
}

/// Shrink `caps` one character at a time — the choice that adds the fewest estimated
/// visual lines, ties toward the widest then leftmost column — until they fit `budget`
/// or no column can drop below its `lower` bound. Deterministic. This is the
/// *hyphenation fallback*: it runs only when words alone can't fit, pushing caps below
/// word floors (the render then hard-breaks with the break hyphen).
fn shrink_to_fit(caps: &mut [usize], cell_w: &[Vec<usize>], budget: usize, lower: &[usize]) {
    while caps.iter().sum::<usize>() > budget {
        let base = est_lines(cell_w, caps);
        let mut best: Option<((usize, usize, usize), usize)> = None;
        for c in 0..caps.len() {
            if caps[c] <= lower[c].max(1) {
                continue;
            }
            let mut trial = caps.to_vec();
            trial[c] -= 1;
            let added = est_lines(cell_w, &trial) - base;
            let key = (added, usize::MAX - caps[c], c); // fewest lines, widest, leftmost
            if best.is_none_or(|(best_key, _)| key < best_key) {
                best = Some((key, c));
            }
        }
        match best {
            Some((_, c)) => caps[c] -= 1,
            None => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::render_wrapped;
    use crate::{BREAK_HYPHEN, DEFAULT_SENTINEL};

    #[test]
    fn allocation_keeps_words_whole_and_wraps_the_other_column() {
        // col 0 is one unbreakable word; col 1 can wrap at its space. The shrink is taken
        // from col 1 so col 0's word stays intact, even though that costs a line.
        let rows = vec![vec!["xxxxxxxx", "a b"], vec!["y", "c d"]];
        let caps = allocate_caps(&rows, &[8, 3], 9);
        assert_eq!(caps[0], 8, "the single-word column stays whole");
        assert!(caps[1] < 3, "the wrappable column absorbs the shrink");
        assert!(caps.iter().sum::<usize>() <= 9);
    }

    #[test]
    fn allocation_takes_free_splits_first() {
        // Step 1 must wrap col 0 (best reclaim), making record 0 two lines tall. Step 2's
        // col-1 split is then FREE — record 0 is already tall — so the greedy takes it and
        // lands on [4, 2] instead of stopping wider.
        let rows = vec![vec!["aaaa bbbb", "cc dd"], vec!["e", "f"]];
        let caps = allocate_caps(&rows, &[9, 5], 8);
        assert_eq!(caps, vec![4, 2]);
    }

    #[test]
    fn allocation_narrows_a_tie_group_in_one_move() {
        // both col-0 cells pin the column at 5; one move re-wraps the whole tie group and
        // the column tightens straight to the next breakpoint (2), not to 4
        let rows = vec![vec!["aa bb", "x"], vec!["cc dd", "y"]];
        let caps = allocate_caps(&rows, &[5, 1], 4);
        assert_eq!(caps, vec![2, 1]);
    }

    #[test]
    fn equal_moves_prefer_the_leftmost_column() {
        // identical candidates in both columns: determinism demands the leftmost one moves
        let rows = vec![vec!["aa bb", "cc dd"]];
        let caps = allocate_caps(&rows, &[5, 5], 8);
        assert_eq!(caps, vec![2, 5]);
    }

    #[test]
    fn allocation_hyphenates_only_when_stuck_at_word_floors() {
        // a single unbreakable word holds col 0 at 20; no word-level move exists, so the
        // hyphenating fallback engages and the render shows the break marker
        let rows = vec![vec!["supercalifragilistic", "ab"]];
        let caps = allocate_caps(&rows, &[20, 2], 10);
        assert!(caps.iter().sum::<usize>() <= 10, "fallback failed to reach the budget");
        assert!(caps[0] < 20, "the long-word column had to give");
        let out = render_wrapped(&rows, &[20, 2], &[false, false], "  ", 13, DEFAULT_SENTINEL);
        assert!(out.iter().any(|l| l.contains(BREAK_HYPHEN)), "expected a hyphenated break");
    }

    #[test]
    fn a_fitting_table_is_left_untouched() {
        // Σ natural ≤ budget: no wrapping, caps == natural (the early return)
        let rows = vec![vec!["aaaaa", "bbb"]];
        assert_eq!(allocate_caps(&rows, &[5, 3], 8), vec![5, 3]);
    }

    #[test]
    fn relaxation_fires_at_the_end_of_allocation() {
        // End-to-end through allocate_caps. The walk goes:
        //   1. split col X ("ccc ddd", gain 4)          → caps [20, 3]
        //   2. split col L at its small breakpoint (g2) → caps [18, 3]
        //   3. split col L again — a FREE move (r1 is already 2 tall) with a HUGE jump
        //      (18 → 10, gain 8)                        → caps [10, 3] = 13 ≤ 17, slack 4
        // That overshoot slack covers X's regrow (3 → 7, cost 4), which merges r2's cell
        // back to one line. Without the relax phase the answer would be [10, 3].
        let rows = vec![
            vec!["ppppppppp rrrrrrrr q", "z"], // L binds r1
            vec!["w", "ccc ddd"],              // X binds r2
        ];
        let caps = allocate_caps(&rows, &[20, 7], 17);
        assert_eq!(caps, vec![10, 7], "the slack should have regrown col 1");
    }

    #[test]
    fn relaxation_regrows_a_column_when_that_removes_lines() {
        // both columns sit split at 4; the budget leaves slack 4. Growing col 1 back to 7
        // (cost 3) merges record 0's cell to one line — record 0's height drops — while
        // growing col 0 would need 5. The relax pass takes the win.
        let rows = vec![vec!["x", "cccc dd"], vec!["mmmm nnnn", "y"]];
        let mut caps = vec![4, 4];
        relax_into_slack(&rows, &[9, 7], &mut caps, 12);
        assert_eq!(caps, vec![4, 7]);
    }

    #[test]
    fn relaxation_never_grows_past_the_budget_or_natural_width() {
        let rows = vec![vec!["aa bb", "cc"]];
        let mut caps = vec![2, 2];
        relax_into_slack(&rows, &[5, 2], &mut caps, 20);
        assert!(caps.iter().sum::<usize>() <= 20);
        assert!(caps[0] <= 5 && caps[1] <= 2, "grew past natural width: {caps:?}");
    }
}
