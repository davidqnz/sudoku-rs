//! Solution rate: how fast the solver enumerates the solutions of a grid.
//!
//! Rating a solver on a puzzle with one solution measures how quickly it gets
//! lucky. This uses an under-constrained grid instead, so the reported figure
//! is the rate the search walks a whole tree -- which is what every change to
//! propagation moves.
//!
//! Run with `just bench-rate`.

use std::hint::black_box;
use std::ops::ControlFlow;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sudoku_rs::Board;
use sudoku_rs::solver::{count_solutions, for_each_solution};

/// `puzzles/30k.txt`, inlined so the benchmark does not depend on the file.
/// Twenty clues, which is few enough to leave tens of thousands of solutions.
const PUZZLE: &str =
    "....5...62.1....9.6.............897....6....5....3...81....2.........3..8...9.12.";

/// How many solutions [`PUZZLE`] has.
///
/// Criterion divides by this to turn a duration into a rate, so left stale
/// against the puzzle above it would quietly rescale every result. Both cases
/// assert it to keep that from happening -- whether the solver is *right* is
/// the test suite's business, not this file's.
const SOLUTIONS: u64 = 31_610;

fn solution_rate(c: &mut Criterion) {
    let puzzle: Board = PUZZLE.parse().expect("the inlined puzzle should parse");

    let mut group = c.benchmark_group("solution_rate");
    group.throughput(Throughput::Elements(SOLUTIONS));

    // The search on its own. Counting never inspects a solution, so no grid is
    // transposed back into a `Board`.
    group.bench_function("count", |b| {
        b.iter(|| {
            let found = count_solutions(black_box(&puzzle), usize::MAX);
            assert_eq!(
                found as u64, SOLUTIONS,
                "SOLUTIONS no longer matches PUZZLE"
            );
            found
        });
    });

    // What a caller that wants the grids pays on top: one `Board` per solution.
    group.bench_function("visit", |b| {
        b.iter(|| {
            let mut found = 0u64;
            for_each_solution(black_box(&puzzle), |solution| {
                black_box(solution);
                found += 1;
                ControlFlow::Continue(())
            });
            assert_eq!(found, SOLUTIONS, "SOLUTIONS no longer matches PUZZLE");
            found
        });
    });

    group.finish();
}

criterion_group!(benches, solution_rate);
criterion_main!(benches);
