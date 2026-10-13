//! A 9x9 Sudoku engine: a bitmask board, an AC-3 style propagator with
//! backtracking search, and a generator for puzzles with a unique solution.
//!
//! The engine performs no I/O. Puzzles are read through [`Board`]'s `FromStr`
//! and written through its `Display`; anything user-facing belongs to the
//! caller.
//!
//! ```
//! use sudoku_core::{Board, solver};
//!
//! let puzzle: Board = "53..7....6..195....98....6.8...6...34..8.3..17...2...6\
//!                      .6....28....419..5....8..79".parse().unwrap();
//! let (solution, stats) = solver::solve_first(&puzzle).unwrap();
//! assert!(solution.is_solved());
//! assert_eq!(stats.branch_nodes, 0);
//! ```

pub mod board;
pub mod generator;
pub mod hint;
pub mod rng;
pub mod solver;

pub use board::{Board, ParseError};
pub use generator::Difficulty;
pub use solver::{Contradiction, Detail, SolveStats};
