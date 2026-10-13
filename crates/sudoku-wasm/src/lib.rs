//! JavaScript bindings to the Sudoku engine in `sudoku_core`.
//!
//! Puzzles cross the boundary as strings, in any form `Board`'s `FromStr`
//! accepts, and come back as 81-character lines with `.` for blanks. A puzzle
//! that does not parse throws an `Error` carrying the parser's message.
//!
//! A game in progress is different: a player's grid may break the rules, which
//! a puzzle string may not. The functions that read one -- `conflicts`,
//! `candidates`, and `hint` -- take it as a `Uint8Array` of 81 digits instead,
//! `0` for a blank, and never reject it for a clash.
//!
//! Build with `wasm-pack build crates/sudoku-wasm`.

use std::ops::ControlFlow;

use js_sys::Function;
use sudoku_core::generator::{self, Difficulty as CoreDifficulty};
use sudoku_core::hint::{self as core_hint, House as CoreHouse, Technique as CoreTechnique};
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

/// How much guessing a puzzle takes: `easy` falls to singles alone.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Easy = "easy",
    Medium = "medium",
    Hard = "hard",
}

impl From<CoreDifficulty> for Difficulty {
    fn from(difficulty: CoreDifficulty) -> Self {
        match difficulty {
            CoreDifficulty::Easy => Difficulty::Easy,
            CoreDifficulty::Medium => Difficulty::Medium,
            CoreDifficulty::Hard => Difficulty::Hard,
        }
    }
}

/// A generated puzzle with exactly one solution.
#[wasm_bindgen(getter_with_clone)]
pub struct Generated {
    /// 81 cells, `.` for blanks.
    pub puzzle: String,
    /// The difficulty the puzzle rates as. Usually the one asked for, but when
    /// no attempt lands in that band the closest puzzle found comes back.
    pub difficulty: Difficulty,
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
    #[wasm_bindgen(unchecked_param_type = "Difficulty")] difficulty: &str,
    seed: Option<u64>,
) -> Result<Generated, JsError> {
    let difficulty: CoreDifficulty = difficulty.parse().map_err(|e: String| JsError::new(&e))?;
    let seed = seed.unwrap_or_else(random_seed);
    let (puzzle, stats) = generator::generate(difficulty, seed);
    Ok(Generated {
        puzzle: puzzle.to_line(),
        difficulty: CoreDifficulty::for_branch_nodes(stats.branch_nodes).into(),
        seed,
        stats: stats.into(),
    })
}

/// The puzzle drawn as a boxed 9-line grid, which parses back.
#[wasm_bindgen(js_name = toPretty)]
pub fn to_pretty(puzzle: &str) -> Result<String, JsError> {
    Ok(parse(puzzle)?.to_pretty())
}

/// The puzzle as one 81-character line, `.` for blanks: the form every other
/// function returns, whatever form it was given in.
#[wasm_bindgen(js_name = toLine)]
pub fn to_line(puzzle: &str) -> Result<String, JsError> {
    Ok(parse(puzzle)?.to_line())
}

/// How hard the puzzle is, or `undefined` if it does not have exactly one
/// solution.
#[wasm_bindgen]
pub fn rate(puzzle: &str) -> Result<Option<Difficulty>, JsError> {
    Ok(generator::rate(&parse(puzzle)?).map(Difficulty::from))
}

/// Reads a player's grid: 81 digits, `0` for a blank. Clashes are allowed.
fn grid(cells: &[u8]) -> Result<Board, JsError> {
    if cells.len() != 81 {
        return Err(JsError::new(&format!(
            "expected 81 cells, found {}",
            cells.len()
        )));
    }
    let mut board = Board::empty();
    for (cell, &digit) in cells.iter().enumerate() {
        match digit {
            0 => {}
            1..=9 => board.assign(cell, digit),
            _ => return Err(JsError::new(&format!("cell {cell} holds {digit}, not 0-9"))),
        }
    }
    Ok(board)
}

/// For each cell, `1` if its digit repeats in its row, column, or box, and
/// `0` otherwise. Both cells of a clash are marked.
#[wasm_bindgen]
pub fn conflicts(cells: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(grid(cells)?.conflicts().map(u8::from).to_vec())
}

/// For each cell, the digits it may take as a bitmask, bit 0 for `1` up to bit
/// 8 for `9`. A blank cell loses the digits of its filled peers, so this is
/// what a player would pencil in; a filled cell's mask is its own digit.
#[wasm_bindgen]
pub fn candidates(cells: &[u8]) -> Result<Vec<u16>, JsError> {
    Ok(core_hint::candidates(&grid(cells)?).to_vec())
}

/// How a [`Hint`] was found.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Technique {
    /// The digit has only one home left in `house`.
    HiddenSingle = "hiddenSingle",
    /// The cell's row, column, and box rule out every digit but one.
    NakedSingle = "nakedSingle",
    /// No single is left; the digit comes from the solution.
    Solution = "solution",
}

/// The kind of house a hidden single was found in.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HouseKind {
    Row = "row",
    Column = "column",
    Box = "box",
}

/// A digit to write into a blank cell, and why.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub struct Hint {
    /// 0..81, left to right, top to bottom.
    pub cell: u8,
    pub digit: u8,
    pub technique: Technique,
    /// For a hidden single, the kind of house it was found in.
    pub house: Option<HouseKind>,
    /// For a hidden single, which row, column, or box: 0..9 from the top left.
    #[wasm_bindgen(js_name = houseIndex)]
    pub house_index: Option<u8>,
}

/// The next step for a player, or `undefined` if the grid is full or cannot
/// be completed. On a puzzle with one solution, that second case means some
/// entry is wrong, even if it clashes with nothing yet.
#[wasm_bindgen]
pub fn hint(cells: &[u8]) -> Result<Option<Hint>, JsError> {
    let Some(step) = core_hint::hint(&grid(cells)?) else {
        return Ok(None);
    };
    let (technique, house) = match step.technique {
        CoreTechnique::HiddenSingle(house) => (Technique::HiddenSingle, Some(house)),
        CoreTechnique::NakedSingle => (Technique::NakedSingle, None),
        CoreTechnique::Solution => (Technique::Solution, None),
    };
    let house = house.map(|house| match house {
        CoreHouse::Row(i) => (HouseKind::Row, i),
        CoreHouse::Column(i) => (HouseKind::Column, i),
        CoreHouse::Box(i) => (HouseKind::Box, i),
    });
    Ok(Some(Hint {
        cell: step.cell as u8,
        digit: step.digit,
        technique,
        house: house.map(|(kind, _)| kind),
        house_index: house.map(|(_, index)| index),
    }))
}

/// `Math.random` gives 52 random bits at most, so take two draws of 32.
fn random_seed() -> u64 {
    let draw = || (js_sys::Math::random() * 4_294_967_296.0) as u64;
    (draw() << 32) | draw()
}
