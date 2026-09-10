//! Constraint propagation and backtracking search.
//!
//! Propagation is AC-3 specialised to all-different constraints. For a binary
//! `!=` arc, revising `D(x)` against `D(y)` can only ever remove a value when
//! `D(y)` is a singleton, so the generic queue of arcs collapses into a queue
//! of newly-solved cells. On top of that arc consistency we run hidden singles
//! -- a digit with only one home left in a unit -- which is the cheapest rule
//! that plain arc consistency cannot see.

use std::ops::ControlFlow;

use crate::board::{Board, PEERS, UNITS, mask_digit};
use crate::rng::Rng;

/// The board cannot be completed: some cell has no candidates left, or some
/// digit has nowhere to go in a unit.
#[derive(Debug, PartialEq, Eq)]
pub struct Contradiction;

/// Work done while solving. Useful for rating a puzzle's difficulty.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SolveStats {
    /// Cells fixed to a digit by propagation.
    pub assignments: usize,
    /// Candidates removed from domains.
    pub eliminations: usize,
    /// Cells search had to guess at, because propagation stalled.
    pub branch_nodes: usize,
    /// Guesses propagation then refuted.
    pub backtracks: usize,
}

/// Runs propagation to a fixpoint, narrowing domains in place.
///
/// Returning `Ok` does not mean the board is solved, only that it is arc
/// consistent and free of hidden singles.
pub fn propagate(board: &mut Board) -> Result<(), Contradiction> {
    propagate_with(board, &mut SolveStats::default())
}

fn propagate_with(board: &mut Board, stats: &mut SolveStats) -> Result<(), Contradiction> {
    let mut queue: Vec<u8> = (0..81)
        .filter(|&i| board.candidates(i).count_ones() == 1)
        .map(|i| i as u8)
        .collect();

    loop {
        while let Some(cell) = queue.pop() {
            let mask = board.candidates(cell as usize);
            if mask == 0 {
                return Err(Contradiction);
            }
            if mask.count_ones() != 1 {
                // A later elimination widened our view of this cell; it will be
                // requeued if it narrows again.
                continue;
            }
            let digit = mask_digit(mask);
            for &peer in &PEERS[cell as usize] {
                if board.eliminate(peer as usize, digit) {
                    stats.eliminations += 1;
                    let peer_mask = board.candidates(peer as usize);
                    if peer_mask == 0 {
                        return Err(Contradiction);
                    }
                    if peer_mask.count_ones() == 1 {
                        stats.assignments += 1;
                        queue.push(peer);
                    }
                }
            }
        }

        if !hidden_singles(board, stats, &mut queue)? {
            return Ok(());
        }
    }
}

/// One sweep of the hidden-single rule. Returns whether it placed anything.
fn hidden_singles(
    board: &mut Board,
    stats: &mut SolveStats,
    queue: &mut Vec<u8>,
) -> Result<bool, Contradiction> {
    let mut changed = false;
    for unit in UNITS.iter() {
        for digit in 1..=9u8 {
            let bit = 1u16 << (digit - 1);
            let mut count = 0;
            let mut home = 0usize;
            for &cell in unit {
                if board.candidates(cell as usize) & bit != 0 {
                    count += 1;
                    home = cell as usize;
                }
            }
            match count {
                0 => return Err(Contradiction),
                1 if board.candidates(home) != bit => {
                    board.assign(home, digit);
                    stats.assignments += 1;
                    queue.push(home as u8);
                    changed = true;
                }
                _ => {}
            }
        }
    }
    Ok(changed)
}

/// The unassigned cell with the fewest candidates (minimum remaining values),
/// or `None` when every cell is fixed.
fn most_constrained(board: &Board) -> Option<usize> {
    let mut best = None;
    let mut best_len = 10;
    for cell in 0..81 {
        let len = board.candidates(cell).count_ones();
        if len > 1 && len < best_len {
            best_len = len;
            best = Some(cell);
            if len == 2 {
                break;
            }
        }
    }
    best
}

/// Solves the puzzle, returning the first solution found.
pub fn solve_first(board: &Board) -> Option<(Board, SolveStats)> {
    solve_first_ordered(board, None)
}

/// Like [`solve_first`], but tries candidate digits in a random order. Feeding
/// it an empty board is how the generator draws a random complete grid.
pub fn solve_first_with_rng(board: &Board, rng: &mut Rng) -> Option<(Board, SolveStats)> {
    solve_first_ordered(board, Some(rng))
}

fn solve_first_ordered(board: &Board, rng: Option<&mut Rng>) -> Option<(Board, SolveStats)> {
    let mut solution = None;
    let mut stats = SolveStats::default();
    let mut rng = rng;
    let _ = search(
        *board,
        &mut |found: &Board| {
            solution = Some(*found);
            ControlFlow::Break(())
        },
        &mut stats,
        &mut rng,
    );
    solution.map(|solution| (solution, stats))
}

/// Calls `visit` with every solution, in search order, and returns the work
/// done. Return [`ControlFlow::Break`] from `visit` to stop the search early.
///
/// Solutions are never collected, so this is the way to enumerate a puzzle with
/// many of them: an under-constrained grid can have billions, which no `Vec`
/// could hold. A caller that wants a bounded number must break out itself.
///
/// ```
/// use std::ops::ControlFlow;
/// use sudoku_rs::{Board, solver};
///
/// // Four blanks in a rectangle, holding two digits that can swap.
/// let puzzle: Board = "534..8912672195348198342567859..1423426853791713924856\
///                      961537284287419635345286179".parse().unwrap();
/// let mut count = 0;
/// solver::for_each_solution(&puzzle, |_| {
///     count += 1;
///     ControlFlow::Continue(())
/// });
/// assert_eq!(count, 2);
/// ```
pub fn for_each_solution<F>(board: &Board, mut visit: F) -> SolveStats
where
    F: FnMut(&Board) -> ControlFlow<()>,
{
    let mut stats = SolveStats::default();
    let _ = search(*board, &mut visit, &mut stats, &mut None);
    stats
}

/// Counts solutions, stopping once `limit` have been found. Pass `2` to test
/// whether a puzzle is a true Sudoku with exactly one solution.
pub fn count_solutions(board: &Board, limit: usize) -> usize {
    if limit == 0 {
        return 0;
    }
    let mut count = 0;
    for_each_solution(board, |_| {
        count += 1;
        if count >= limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    count
}

/// Depth-first search over propagated states. Returns [`ControlFlow::Break`]
/// once the visitor asks to stop, which unwinds the whole recursion.
fn search<F>(
    mut board: Board,
    visit: &mut F,
    stats: &mut SolveStats,
    rng: &mut Option<&mut Rng>,
) -> ControlFlow<()>
where
    F: FnMut(&Board) -> ControlFlow<()>,
{
    if propagate_with(&mut board, stats).is_err() {
        stats.backtracks += 1;
        return ControlFlow::Continue(());
    }

    let Some(cell) = most_constrained(&board) else {
        return visit(&board);
    };

    stats.branch_nodes += 1;
    let mask = board.candidates(cell);
    let mut digits: Vec<u8> = (1..=9u8).filter(|&d| mask & (1 << (d - 1)) != 0).collect();
    if let Some(rng) = rng.as_deref_mut() {
        rng.shuffle(&mut digits);
    }

    for digit in digits {
        // The board is 162 bytes, so branching on a copy is cheaper and far
        // simpler than trailing every elimination for undo.
        let mut child = board;
        child.assign(cell, digit);
        search(child, visit, stats, rng)?;
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EASY: &str = "\
        53..7....\
        6..195...\
        .98....6.\
        8...6...3\
        4..8.3..1\
        7...2...6\
        .6....28.\
        ...419..5\
        ....8..79";

    #[test]
    fn propagation_alone_solves_an_easy_puzzle() {
        let mut board: Board = EASY.parse().unwrap();
        propagate(&mut board).unwrap();
        assert!(board.is_solved());
        assert!(board.is_consistent());
    }

    #[test]
    fn propagation_detects_an_impossible_board() {
        // 1 and 2 are the only candidates left for three cells of the top row.
        let mut board = Board::empty();
        for (cell, digit) in [(3, 3), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8)] {
            board.assign(cell, digit);
        }
        for cell in [9, 18] {
            board.assign(cell, 9);
        }
        assert!(propagate(&mut board).is_err());
    }

    #[test]
    fn most_constrained_prefers_the_smallest_domain() {
        let mut board = Board::empty();
        board.eliminate(40, 1);
        for digit in 1..=7 {
            board.eliminate(20, digit);
        }
        assert_eq!(most_constrained(&board), Some(20));
    }
}
