//! Help for a person solving by hand: the candidates they would pencil in,
//! and the next step to take, named after the technique that finds it.
//!
//! These read a board the way a player sees it: a cell with one candidate is
//! filled in, and any other cell is blank. Unlike the solver's propagation,
//! they deduce only from filled cells, never from each other, so every step
//! they suggest is one a person could spot on the grid in front of them.

use crate::board::{Board, PEERS, UNITS, digit_bit, mask_digit};
use crate::solver::{Detail, solve_first_with_detail};

/// A row, column, or box, numbered 0..9 from the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum House {
    Row(u8),
    Column(u8),
    Box(u8),
}

impl House {
    /// The house at `unit`, an index into [`UNITS`].
    fn of_unit(unit: usize) -> House {
        let index = (unit % 9) as u8;
        match unit / 9 {
            0 => House::Row(index),
            1 => House::Column(index),
            _ => House::Box(index),
        }
    }

    /// The nine cells of the house.
    pub fn cells(self) -> [u8; 9] {
        let unit = match self {
            House::Row(i) => i as usize,
            House::Column(i) => 9 + i as usize,
            House::Box(i) => 18 + i as usize,
        };
        UNITS[unit]
    }
}

/// How a [`Hint`] was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Technique {
    /// The filled cells in this house leave the digit only one home.
    HiddenSingle(House),
    /// The cell's filled peers rule out every digit but one.
    NakedSingle,
    /// No single is left to spot. The step comes from the solution instead, at
    /// the blank cell with the fewest candidates.
    Solution,
}

/// A digit to write into a blank cell, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub cell: usize,
    pub digit: u8,
    pub technique: Technique,
}

/// The candidates of every cell: each blank cell loses the digits of its
/// filled peers, and a filled cell's only candidate is its own digit.
///
/// A blank cell with no candidates left, `0`, is a cell no digit can go in.
pub fn candidates(board: &Board) -> [u16; 81] {
    let mut out = [0u16; 81];
    for (cell, candidates) in out.iter_mut().enumerate() {
        let mask = board.candidates(cell);
        *candidates = if mask.count_ones() == 1 {
            mask
        } else {
            let taken = PEERS[cell]
                .iter()
                .map(|&peer| board.candidates(peer as usize))
                .filter(|peer| peer.count_ones() == 1)
                .fold(0, |taken, peer| taken | peer);
            mask & !taken
        };
    }
    out
}

/// The next step towards solving `board`, or `None` if it is already
/// complete or cannot be completed -- which, for a puzzle with one solution,
/// is how a wrong entry shows up.
///
/// Singles come first, and hidden singles in boxes before the rest, since
/// cross-hatching a box is the step most solvers look for first. A single is
/// forced in every solution, so on a board with several solutions only a
/// [`Technique::Solution`] step is a choice among them.
pub fn hint(board: &Board) -> Option<Hint> {
    let (solution, _) = solve_first_with_detail(board, Detail::Search)?;
    let candidates = candidates(board);
    let blank = |cell: usize| board.candidates(cell).count_ones() != 1;

    for unit in (18..27).chain(0..18) {
        let cells = UNITS[unit].map(usize::from);
        let placed = cells
            .iter()
            .filter(|&&cell| !blank(cell))
            .fold(0, |placed, &cell| placed | board.candidates(cell));
        for digit in 1..=9 {
            let bit = digit_bit(digit);
            if placed & bit != 0 {
                continue;
            }
            let mut homes = cells
                .iter()
                .filter(|&&cell| blank(cell) && candidates[cell] & bit != 0);
            if let (Some(&cell), None) = (homes.next(), homes.next()) {
                let technique = Technique::HiddenSingle(House::of_unit(unit));
                return Some(Hint {
                    cell,
                    digit,
                    technique,
                });
            }
        }
    }

    if let Some(cell) = (0..81).find(|&cell| blank(cell) && candidates[cell].count_ones() == 1) {
        let digit = mask_digit(candidates[cell]);
        return Some(Hint {
            cell,
            digit,
            technique: Technique::NakedSingle,
        });
    }

    let cell = (0..81)
        .filter(|&cell| blank(cell))
        .min_by_key(|&cell| candidates[cell].count_ones())?;
    let digit = solution.get_digit(cell).expect("a solution is complete");
    Some(Hint {
        cell,
        digit,
        technique: Technique::Solution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::ALL;
    use crate::generator::{Difficulty, generate};
    use crate::solver::solve_first;

    const EASY: &str =
        "53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79";

    /// Takes hints until the board is full, returning how many were read off
    /// the solution rather than found.
    fn solve_by_hints(mut board: Board) -> (Board, usize) {
        let mut from_solution = 0;
        while let Some(step) = hint(&board) {
            assert_eq!(
                board.candidates(step.cell).count_ones(),
                9,
                "hinted a filled cell"
            );
            if step.technique == Technique::Solution {
                from_solution += 1;
            }
            board.assign(step.cell, step.digit);
        }
        (board, from_solution)
    }

    #[test]
    fn candidates_drop_the_digits_of_filled_peers() {
        let board: Board = EASY.parse().unwrap();
        let candidates = candidates(&board);
        assert_eq!(candidates[0], digit_bit(5), "a filled cell keeps its digit");
        // Its row holds 5 3 7, its column 8, and its box 5 3 6 9 8.
        let expected = [1, 2, 4].map(digit_bit).iter().fold(0, |a, b| a | b);
        assert_eq!(candidates[2], expected);
        assert_eq!(super::candidates(&Board::empty()), [ALL; 81]);
    }

    #[test]
    fn an_easy_puzzle_falls_to_singles_alone() {
        let board: Board = EASY.parse().unwrap();
        let (solved, from_solution) = solve_by_hints(board);
        assert_eq!(from_solution, 0);
        assert_eq!(solved, solve_first(&board).unwrap().0);
    }

    #[test]
    fn hints_lead_to_the_solution_of_generated_puzzles() {
        for (difficulty, seed) in [
            (Difficulty::Easy, 1),
            (Difficulty::Medium, 2),
            (Difficulty::Hard, 3),
        ] {
            let (puzzle, _) = generate(difficulty, seed);
            let (solved, from_solution) = solve_by_hints(puzzle);
            assert_eq!(solved, solve_first(&puzzle).unwrap().0, "{difficulty}");
            if difficulty == Difficulty::Easy {
                assert_eq!(from_solution, 0, "an easy puzzle needs no guessing");
            }
        }
    }

    #[test]
    fn a_wrong_entry_or_a_full_board_gets_no_hint() {
        let mut board: Board = EASY.parse().unwrap();
        let (solution, _) = solve_first(&board).unwrap();
        // Cell 2 is 4 in the solution. A 2 there breaks no rule, but is wrong.
        board.assign(2, 2);
        assert!(board.conflicts().iter().all(|&c| !c));
        assert_eq!(hint(&board), None);
        assert_eq!(hint(&solution), None);
    }

    #[test]
    fn a_hidden_single_names_its_house() {
        let board: Board = EASY.parse().unwrap();
        let step = hint(&board).unwrap();
        let Technique::HiddenSingle(house) = step.technique else {
            panic!("expected a hidden single, got {step:?}");
        };
        assert!(house.cells().contains(&(step.cell as u8)));
    }
}
