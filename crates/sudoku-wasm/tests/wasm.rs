//! The bindings, exercised under a real JS host. Run with
//! `wasm-pack test --node crates/sudoku-wasm`.

#![cfg(target_arch = "wasm32")]

use js_sys::Function;
use sudoku_wasm::{
    Difficulty, Technique, candidates, conflicts, count_solutions, for_each_solution, generate,
    hint, rate, solve, to_line, to_pretty,
};
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
    assert_eq!(first.difficulty, Difficulty::Easy);
    assert_eq!(rate(&first.puzzle).unwrap(), Some(Difficulty::Easy));
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

#[wasm_bindgen_test]
fn a_drawn_grid_comes_back_as_a_line() {
    assert_eq!(to_line(&to_pretty(EASY).unwrap()).unwrap(), EASY);
}

/// `EASY` as the digits a game passes around, `0` for a blank.
fn easy_cells() -> Vec<u8> {
    EASY.bytes()
        .map(|b| if b == b'.' { 0 } else { b - b'0' })
        .collect()
}

#[wasm_bindgen_test]
fn rates_only_puzzles_with_one_solution() {
    assert_eq!(rate(EASY).unwrap(), Some(Difficulty::Easy));
    assert_eq!(rate(TWO).unwrap(), None);
}

#[wasm_bindgen_test]
fn flags_clashing_cells_without_rejecting_them() {
    let mut cells = easy_cells();
    assert!(conflicts(&cells).unwrap().iter().all(|&c| c == 0));
    cells[2] = 5; // cell 0 already holds 5
    let flags = conflicts(&cells).unwrap();
    let clashing: Vec<usize> = (0..81).filter(|&i| flags[i] == 1).collect();
    assert_eq!(clashing, [0, 2]);
}

#[wasm_bindgen_test]
fn pencils_in_candidates() {
    let masks = candidates(&easy_cells()).unwrap();
    assert_eq!(masks.len(), 81);
    assert_eq!(masks[0], 1 << 4, "a filled 5 keeps its own bit");
    assert_eq!(masks[2], 0b1011, "1, 2, or 4");
}

#[wasm_bindgen_test]
fn hints_name_a_technique_and_stop_at_a_wrong_entry() {
    let mut cells = easy_cells();
    let step = hint(&cells)
        .unwrap()
        .expect("an unsolved puzzle has a next step");
    assert_eq!(step.technique, Technique::HiddenSingle);
    assert!(step.house.is_some() && step.house_index.is_some());
    assert_eq!(cells[step.cell as usize], 0);
    assert_eq!(
        EASY_SOLUTION.as_bytes()[step.cell as usize] - b'0',
        step.digit
    );

    cells[2] = 2; // clashes with nothing, but the solution has 4 here
    assert!(hint(&cells).unwrap().is_none());
}

#[wasm_bindgen_test]
fn a_grid_of_the_wrong_shape_is_an_error() {
    assert!(hint(&[0; 80]).is_err());
    assert!(candidates(&[10; 81]).is_err());
}
