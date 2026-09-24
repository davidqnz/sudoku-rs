//! JavaScript bindings to the Sudoku engine in `sudoku_core`.
//!
//! Puzzles cross the boundary as strings, in any form `Board`'s `FromStr`
//! accepts, and come back as 81-character lines with `.` for blanks. A puzzle
//! that does not parse throws an `Error` carrying the parser's message.
//!
//! Build with `wasm-pack build crates/sudoku-wasm`.

use std::ops::ControlFlow;

use js_sys::Function;
use sudoku_core::generator::Difficulty;
use sudoku_core::solver::{self, Detail};
use sudoku_core::{Board, SolveStats as CoreStats};
use wasm_bindgen::prelude::*;

/// Work the solver did. `branchNodes` is the difficulty measure: the number of
/// guesses propagation could not avoid.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub struct SolveStats {
    /// Cells fixed to a digit by propagation.
    pub assignments: usize,
    /// Candidates removed from domains.
    pub eliminations: usize,
    /// Cells search had to guess at, because propagation stalled.
    #[wasm_bindgen(js_name = branchNodes)]
    pub branch_nodes: usize,
    /// Guesses propagation then refuted.
    pub backtracks: usize,
}

impl From<CoreStats> for SolveStats {
    fn from(stats: CoreStats) -> Self {
        SolveStats {
            assignments: stats.assignments,
            eliminations: stats.eliminations,
            branch_nodes: stats.branch_nodes,
            backtracks: stats.backtracks,
        }
    }
}

/// A solved grid and the work it took to find.
#[wasm_bindgen(getter_with_clone)]
pub struct Solution {
    /// 81 digits, left to right, top to bottom.
    pub grid: String,
    pub stats: SolveStats,
}

/// A generated puzzle with exactly one solution.
#[wasm_bindgen(getter_with_clone)]
pub struct Generated {
    /// 81 cells, `.` for blanks.
    pub puzzle: String,
    /// The seed that reproduces this puzzle within the same build.
    pub seed: u64,
    /// The cost of solving the puzzle, which is what rated its difficulty.
    pub stats: SolveStats,
}

fn parse(puzzle: &str) -> Result<Board, JsError> {
    Ok(puzzle.parse::<Board>()?)
}

fn limit_or_unbounded(limit: Option<u32>) -> usize {
    limit.map_or(usize::MAX, |n| n as usize)
}

/// Solves the puzzle, returning the first solution found, or `undefined` if
/// it has none.
#[wasm_bindgen]
pub fn solve(puzzle: &str) -> Result<Option<Solution>, JsError> {
    let board = parse(puzzle)?;
    Ok(
        solver::solve_first(&board).map(|(solution, stats)| Solution {
            grid: solution.to_line(),
            stats: stats.into(),
        }),
    )
}

/// Counts solutions, stopping once `limit` have been found. Pass `2` to test
/// whether a puzzle has exactly one solution.
///
/// Leaving `limit` out counts them all, which on an under-constrained grid
/// may never finish.
#[wasm_bindgen(js_name = countSolutions)]
pub fn count_solutions(puzzle: &str, limit: Option<u32>) -> Result<usize, JsError> {
    let board = parse(puzzle)?;
    Ok(solver::count_solutions(&board, limit_or_unbounded(limit)))
}

/// Calls `visit` with each solution as the search finds it, and returns the
/// work done. Return `false` from `visit` to stop early; any other return
/// value, including `undefined`, continues. If `visit` throws, the search
/// stops and the exception is rethrown.
///
/// Solutions are never collected, so this is the way to walk a puzzle with
/// very many of them.
#[wasm_bindgen(js_name = forEachSolution)]
pub fn for_each_solution(
    puzzle: &str,
    #[wasm_bindgen(unchecked_param_type = "(solution: string) => boolean | void")] visit: &Function,
    limit: Option<u32>,
) -> Result<SolveStats, JsValue> {
    let board = parse(puzzle)?;
    let limit = limit_or_unbounded(limit);
    if limit == 0 {
        return Ok(CoreStats::default().into());
    }

    let mut found = 0usize;
    let mut thrown = None;
    let stats = solver::for_each_solution_with_detail(
        &board,
        |solution| {
            found += 1;
            match visit.call1(&JsValue::NULL, &JsValue::from_str(&solution.to_line())) {
                Err(error) => {
                    thrown = Some(error);
                    ControlFlow::Break(())
                }
                Ok(keep_going) if keep_going == JsValue::FALSE => ControlFlow::Break(()),
                Ok(_) if found >= limit => ControlFlow::Break(()),
                Ok(_) => ControlFlow::Continue(()),
            }
        },
        Detail::Full,
    );
    match thrown {
        Some(error) => Err(error),
        None => Ok(stats.into()),
    }
}

/// Generates a puzzle with exactly one solution, aiming for `difficulty`.
///
/// The same seed and difficulty give the same puzzle within one build. Leave
/// `seed` out for a random one; the seed used is returned either way.
#[wasm_bindgen]
pub fn generate(
    #[wasm_bindgen(unchecked_param_type = "\"easy\" | \"medium\" | \"hard\"")] difficulty: &str,
    seed: Option<u64>,
) -> Result<Generated, JsError> {
    let difficulty: Difficulty = difficulty.parse().map_err(|e: String| JsError::new(&e))?;
    let seed = seed.unwrap_or_else(random_seed);
    let (puzzle, stats) = sudoku_core::generator::generate(difficulty, seed);
    Ok(Generated {
        puzzle: puzzle.to_line(),
        seed,
        stats: stats.into(),
    })
}

/// The puzzle drawn as a boxed 9-line grid, which parses back.
#[wasm_bindgen(js_name = toPretty)]
pub fn to_pretty(puzzle: &str) -> Result<String, JsError> {
    Ok(parse(puzzle)?.to_pretty())
}

/// `Math.random` gives 52 random bits at most, so take two draws of 32.
fn random_seed() -> u64 {
    let draw = || (js_sys::Math::random() * 4_294_967_296.0) as u64;
    (draw() << 32) | draw()
}
