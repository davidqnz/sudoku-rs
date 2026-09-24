//! The bindings, exercised under a real JS host. Run with
//! `wasm-pack test --node crates/sudoku-wasm`.

#![cfg(target_arch = "wasm32")]

use js_sys::Function;
use sudoku_wasm::{count_solutions, for_each_solution, generate, solve, to_pretty};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

const EASY: &str =
    "53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79";
const EASY_SOLUTION: &str =
    "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

/// Four blanks in a rectangle, holding two digits that can swap.
const TWO: &str =
    "534..8912672195348198342567859..1423426853791713924856961537284287419635345286179";

#[wasm_bindgen_test]
fn solves_a_puzzle() {
    let solution = solve(EASY).unwrap().expect("the puzzle has a solution");
    assert_eq!(solution.grid, EASY_SOLUTION);
    assert_eq!(solution.stats.branch_nodes, 0);
}

#[wasm_bindgen_test]
fn an_unsolvable_puzzle_gives_none() {
    // Parses without a conflict, but the first cell has no digit left.
    let puzzle = format!(".23456789{}1{}", ".".repeat(9), ".".repeat(81 - 19));
    assert!(solve(&puzzle).unwrap().is_none());
}

#[wasm_bindgen_test]
fn counts_solutions_up_to_a_limit() {
    assert_eq!(count_solutions(TWO, None).unwrap(), 2);
    assert_eq!(count_solutions(TWO, Some(1)).unwrap(), 1);
    assert_eq!(count_solutions(EASY, Some(2)).unwrap(), 1);
}

#[wasm_bindgen_test]
fn visits_each_solution() {
    let seen = js_sys::Array::new();
    let visit = Function::new_with_args("s", "this.push(s)").bind0(&seen);
    for_each_solution(TWO, &visit, None).unwrap();
    assert_eq!(seen.length(), 2);
    assert_eq!(seen.get(0).as_string().unwrap().len(), 81);
}

#[wasm_bindgen_test]
fn returning_false_or_hitting_the_limit_stops_the_walk() {
    let seen = js_sys::Array::new();
    let stop = Function::new_with_args("s", "this.push(s); return false").bind0(&seen);
    for_each_solution(TWO, &stop, None).unwrap();
    assert_eq!(seen.length(), 1);

    let seen = js_sys::Array::new();
    let visit = Function::new_with_args("s", "this.push(s)").bind0(&seen);
    for_each_solution(TWO, &visit, Some(1)).unwrap();
    assert_eq!(seen.length(), 1);
}

#[wasm_bindgen_test]
fn a_throwing_visitor_rethrows() {
    let boom = Function::new_with_args("s", "throw new Error('boom')");
    let error = for_each_solution(TWO, &boom, None).unwrap_err();
    let error: js_sys::Error = error.into();
    assert_eq!(error.message(), "boom");
}

#[wasm_bindgen_test]
fn generates_a_unique_puzzle_reproducibly() {
    let first = generate("easy", Some(7)).unwrap();
    assert_eq!(first.seed, 7);
    assert_eq!(count_solutions(&first.puzzle, Some(2)).unwrap(), 1);
    assert_eq!(generate("easy", Some(7)).unwrap().puzzle, first.puzzle);

    let unseeded = generate("medium", None).unwrap();
    assert_eq!(count_solutions(&unseeded.puzzle, Some(2)).unwrap(), 1);
}

#[wasm_bindgen_test]
fn bad_input_is_an_error() {
    assert!(generate("impossible", None).is_err());
    let _: JsValue = for_each_solution("123", &Function::new_no_args(""), None).unwrap_err();
    assert!(to_pretty("x").is_err());
}
