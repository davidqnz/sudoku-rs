//! Puzzle generation.
//!
//! Draw a random complete grid, then dig clues out of it in random order,
//! keeping a removal only while the puzzle still has exactly one solution. The
//! result is always a true Sudoku; difficulty is steered by how much digging
//! the search effort budget allows.

use crate::board::Board;
use crate::rng::Rng;
use crate::solver::{SolveStats, count_solutions, solve_first, solve_first_with_rng};

/// How much guessing the solver should need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    /// Falls to propagation alone: no guessing at all.
    Easy,
    /// Needs a handful of guesses.
    Medium,
    /// Needs more search than that.
    Hard,
}

impl Difficulty {
    /// The most branch nodes a puzzle of this difficulty may cost.
    fn max_branches(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => MEDIUM_MAX_BRANCHES,
            Difficulty::Hard => usize::MAX,
        }
    }

    /// The fewest branch nodes a puzzle of this difficulty must cost.
    fn min_branches(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => 1,
            Difficulty::Hard => MEDIUM_MAX_BRANCHES + 1,
        }
    }
}

impl std::str::FromStr for Difficulty {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "easy" => Ok(Difficulty::Easy),
            "medium" => Ok(Difficulty::Medium),
            "hard" => Ok(Difficulty::Hard),
            other => Err(format!("unknown difficulty {other:?}")),
        }
    }
}

impl std::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        };
        f.write_str(name)
    }
}

const MEDIUM_MAX_BRANCHES: usize = 8;

/// How many complete grids to dig before settling for the closest puzzle found.
const MAX_ATTEMPTS: usize = 24;

/// Generates a puzzle with exactly one solution, aiming for `difficulty`.
///
/// The same seed and difficulty always produce the same puzzle. The returned
/// stats are the cost of solving the puzzle, which is what rated it; if no
/// attempt hits the requested band, the closest puzzle generated is returned,
/// so this always terminates with a valid unique puzzle.
pub fn generate(difficulty: Difficulty, seed: u64) -> (Board, SolveStats) {
    let mut rng = Rng::new(seed);
    let mut best: Option<(Board, SolveStats)> = None;

    for _ in 0..MAX_ATTEMPTS {
        let candidate = dig(&mut rng, difficulty);
        let stats = candidate.1;
        if stats.branch_nodes >= difficulty.min_branches()
            && stats.branch_nodes <= difficulty.max_branches()
        {
            return candidate;
        }
        // Otherwise keep whichever attempt came closest to the target band.
        let closer = match &best {
            None => true,
            Some((_, best_stats)) => {
                distance(stats, difficulty) < distance(*best_stats, difficulty)
            }
        };
        if closer {
            best = Some(candidate);
        }
    }

    best.expect("MAX_ATTEMPTS is non-zero")
}

/// How far a puzzle's cost falls outside the requested difficulty band.
fn distance(stats: SolveStats, difficulty: Difficulty) -> usize {
    let branches = stats.branch_nodes;
    if branches < difficulty.min_branches() {
        difficulty.min_branches() - branches
    } else {
        branches.saturating_sub(difficulty.max_branches())
    }
}

/// Digs clues out of one random complete grid, refusing any removal that would
/// make the puzzle ambiguous or push it past the difficulty budget.
fn dig(rng: &mut Rng, difficulty: Difficulty) -> (Board, SolveStats) {
    let (solution, _) =
        solve_first_with_rng(&Board::empty(), rng).expect("the empty board always has a solution");

    let mut puzzle = solution;
    let mut stats = SolveStats::default();
    let mut order: Vec<usize> = (0..81).collect();
    rng.shuffle(&mut order);

    for cell in order {
        let digit = puzzle.get_digit(cell).expect("digging a complete grid");
        puzzle.clear(cell);

        if count_solutions(&puzzle, 2) != 1 {
            puzzle.assign(cell, digit);
            continue;
        }
        let (_, cost) = solve_first(&puzzle).expect("a uniquely solvable puzzle solves");
        if cost.branch_nodes > difficulty.max_branches() {
            puzzle.assign(cell, digit);
            continue;
        }
        stats = cost;
    }

    (puzzle, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_bands_do_not_overlap() {
        assert!(Difficulty::Easy.max_branches() < Difficulty::Medium.min_branches());
        assert!(Difficulty::Medium.max_branches() < Difficulty::Hard.min_branches());
    }

    #[test]
    fn digging_leaves_a_uniquely_solvable_puzzle() {
        let mut rng = Rng::new(3);
        let (puzzle, _) = dig(&mut rng, Difficulty::Hard);
        assert_eq!(count_solutions(&puzzle, 2), 1);
        assert!(puzzle.clue_count() < 81);
    }

    #[test]
    fn digging_is_minimal_for_an_unbounded_budget() {
        // With no difficulty budget, every remaining clue must be load-bearing.
        let mut rng = Rng::new(11);
        let (puzzle, _) = dig(&mut rng, Difficulty::Hard);
        for cell in 0..81 {
            if puzzle.get_digit(cell).is_some() {
                let mut without = puzzle;
                without.clear(cell);
                assert!(
                    count_solutions(&without, 2) > 1,
                    "clue at {cell} was not needed"
                );
            }
        }
    }
}
