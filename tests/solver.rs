//! End-to-end tests over the public engine API.

use sudoku_rs::board::{Board, ParseError};
use sudoku_rs::generator::{Difficulty, generate};
use sudoku_rs::solver::{count_solutions, propagate, solve_first};

const EASY: &str =
    "53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79";
const EASY_SOLUTION: &str =
    "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

/// A 17-clue puzzle -- the known minimum for a unique solution.
const MINIMAL_17: &str =
    ".......1.4.........2...........5.4.7..8...3....1.9....3..4..2...5.1........8.6...";

/// "AI Escargot", built to defeat simple propagation.
const ESCARGOT: &str =
    "1....7.9..3..2...8..96..5....53..9...1..8...26....4...3......1..4......7..7...3..";

#[test]
fn parses_a_single_line_and_a_drawn_grid_the_same_way() {
    let line: Board = EASY.parse().unwrap();
    let grid: Board = "\
        5 3 . | . 7 . | . . .\n\
        6 . . | 1 9 5 | . . .\n\
        . 9 8 | . . . | . 6 .\n\
        ------+-------+------\n\
        8 . . | . 6 . | . . 3\n\
        4 . . | 8 . 3 | . . 1\n\
        7 . . | . 2 . | . . 6\n\
        ------+-------+------\n\
        . 6 . | . . . | 2 8 .\n\
        . . . | 4 1 9 | . . 5\n\
        . . . | . 8 . | . 7 9\n"
        .parse()
        .unwrap();
    assert_eq!(line, grid);
}

#[test]
fn display_round_trips_through_parsing() {
    let puzzle: Board = EASY.parse().unwrap();
    let reparsed: Board = puzzle.to_string().parse().unwrap();
    assert_eq!(puzzle, reparsed);
    assert_eq!(puzzle.to_line(), EASY);

    // The pretty grid is parseable too, separators and all.
    let from_pretty: Board = puzzle.to_pretty().parse().unwrap();
    assert_eq!(puzzle, from_pretty);
}

#[test]
fn rejects_malformed_input() {
    assert_eq!("123".parse::<Board>(), Err(ParseError::BadLength(3)));
    assert_eq!(
        format!("{EASY}5").parse::<Board>(),
        Err(ParseError::BadLength(82))
    );
    assert_eq!(
        EASY.replace('7', "x").parse::<Board>(),
        Err(ParseError::BadChar('x'))
    );
}

#[test]
fn rejects_a_digit_repeated_in_a_unit() {
    // Two 5s in the top row.
    let bad = format!("5.5{}", &EASY[3..]);
    assert!(matches!(
        bad.parse::<Board>(),
        Err(ParseError::Conflict { .. })
    ));
}

#[test]
fn an_easy_puzzle_needs_no_search() {
    let mut board: Board = EASY.parse().unwrap();
    propagate(&mut board).unwrap();
    assert!(board.is_solved());
    assert_eq!(board.to_line(), EASY_SOLUTION);

    let (solution, stats) = solve_first(&board).unwrap();
    assert_eq!(solution.to_line(), EASY_SOLUTION);
    assert_eq!(stats.branch_nodes, 0, "no guessing should be needed");
}

#[test]
fn solves_hard_puzzles_correctly() {
    for puzzle in [MINIMAL_17, ESCARGOT] {
        let board: Board = puzzle.parse().unwrap();
        let (solution, _) = solve_first(&board).unwrap();

        assert!(solution.is_solved());
        assert!(solution.is_consistent());
        assert!(agrees_with_clues(&board, &solution));
        assert_eq!(count_solutions(&board, 2), 1, "should be a true Sudoku");
    }
}

#[test]
fn escargot_needs_search_but_stays_cheap() {
    // Built to defeat single-cell reasoning, so propagation must stall here and
    // the search has to guess -- but MRV should keep that bounded.
    let board: Board = ESCARGOT.parse().unwrap();
    let mut propagated = board;
    propagate(&mut propagated).unwrap();
    assert!(!propagated.is_solved(), "propagation should stall");

    let (_, stats) = solve_first(&board).unwrap();
    assert!(stats.branch_nodes > 0);
    assert!(
        stats.branch_nodes < 100,
        "took {} guesses",
        stats.branch_nodes
    );
}

#[test]
fn reports_no_solution_for_an_unsolvable_puzzle() {
    // The top-left cell can only be a 1, but its column already holds one. No
    // given digit repeats, so this passes parsing and must fail while solving.
    let unsolvable =
        ".234567891.......................................................................";
    let board: Board = unsolvable.parse().unwrap();

    let mut propagated = board;
    assert!(propagate(&mut propagated).is_err());
    assert!(solve_first(&board).is_none());
    assert_eq!(count_solutions(&board, 1), 0);
}

#[test]
fn counting_stops_at_the_limit() {
    let empty = Board::empty();
    assert_eq!(count_solutions(&empty, 1), 1);
    assert_eq!(count_solutions(&empty, 2), 2);
    assert_eq!(count_solutions(&empty, 5), 5);

    let (solution, _) = solve_first(&empty).unwrap();
    assert!(solution.is_solved());
    assert!(solution.is_consistent());
}

#[test]
fn counts_every_solution_of_an_ambiguous_puzzle() {
    // Dropping a clue from a minimal puzzle must make it ambiguous.
    let board: Board = EASY.parse().unwrap();
    let mut ambiguous = board;
    ambiguous.clear(0);
    ambiguous.clear(1);
    ambiguous.clear(4);
    let found = count_solutions(&ambiguous, 5);
    assert!(found >= 1);
    assert_eq!(count_solutions(&board, 2), 1);
}

#[test]
fn generated_puzzles_are_true_sudokus() {
    for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
        for seed in 0..8u64 {
            let (puzzle, _) = generate(difficulty, seed);

            assert!(puzzle.is_consistent(), "{difficulty} seed {seed}");
            assert!(!puzzle.is_solved(), "{difficulty} seed {seed} was not dug");
            assert_eq!(
                count_solutions(&puzzle, 2),
                1,
                "{difficulty} seed {seed} is not uniquely solvable"
            );

            let (solution, _) = solve_first(&puzzle).unwrap();
            assert!(solution.is_solved());
            assert!(solution.is_consistent());
            assert!(agrees_with_clues(&puzzle, &solution));
        }
    }
}

#[test]
fn difficulty_is_ordered_by_search_effort() {
    let effort = |difficulty| {
        (0..8u64)
            .map(|seed| generate(difficulty, seed).1.branch_nodes)
            .sum::<usize>()
    };
    let easy = effort(Difficulty::Easy);
    let medium = effort(Difficulty::Medium);
    let hard = effort(Difficulty::Hard);

    assert_eq!(easy, 0, "easy puzzles should fall to propagation alone");
    assert!(medium > easy, "medium {medium} should outrank easy {easy}");
    assert!(hard > medium, "hard {hard} should outrank medium {medium}");
}

#[test]
fn easy_puzzles_carry_more_clues_than_hard_ones() {
    let clues = |difficulty| {
        (0..8u64)
            .map(|seed| generate(difficulty, seed).0.clue_count())
            .sum::<usize>()
    };
    assert!(clues(Difficulty::Easy) > clues(Difficulty::Hard));
}

#[test]
fn generation_is_reproducible_from_its_seed() {
    for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
        let (first, stats) = generate(difficulty, 12345);
        let (again, stats_again) = generate(difficulty, 12345);
        assert_eq!(first, again);
        assert_eq!(stats, stats_again);

        let (other, _) = generate(difficulty, 54321);
        assert_ne!(first, other, "different seeds should differ");
    }
}

/// Whether `solution` keeps every clue the puzzle started with.
fn agrees_with_clues(puzzle: &Board, solution: &Board) -> bool {
    (0..81).all(|cell| match puzzle.get_digit(cell) {
        Some(digit) => solution.get_digit(cell) == Some(digit),
        None => true,
    })
}
