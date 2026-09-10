# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo test                          # unit tests, integration tests, and doctests
cargo test --test solver            # only tests/solver.rs
cargo test escargot                 # one test, by substring of its name
cargo test --lib                    # only the #[cfg(test)] modules in src/
cargo clippy --all-targets
cargo fmt
cargo run -- gen --difficulty hard --stats
cargo run --release -- solve puzzle.txt --count --stats
```

Generator tests solve many puzzles. Use `cargo test --release` if they feel slow.

## Architecture

A 9x9 Sudoku engine (`src/lib.rs`, crate `sudoku_rs`) plus a text interface (`src/main.rs`).
The engine does no I/O. All printing, argument handling, and error messages stay in `main.rs`.

**Domains are bitmasks.** A cell is a `u16`. Bit 0 means digit 1, bit 8 means digit 9.
One bit set means the cell is solved; zero bits set means a contradiction. `Board` holds
81 of these and nothing else, so it is `Copy` and only 162 bytes. This size is why
`solver::search` branches on a copy of the board instead of a trail of undo records.
Do not add heap data or non-`Copy` fields to `Board` without revisiting that choice.

**Constraint structure is compile-time.** `board::UNITS` (27 all-different groups) and
`board::PEERS` (the 20 cells each cell shares a unit with) are `const fn` tables. The
solver walks `PEERS` instead of building arcs at runtime.

**The solver has two layers** (`src/solver.rs`):
1. `propagate` runs to a fixpoint. It is AC-3 specialised to `!=` constraints: an arc can
   only remove a value when the far domain is a singleton, so the arc queue collapses into
   a queue of newly-solved cells. After each drain it sweeps hidden singles, which arc
   consistency alone cannot find.
2. `search` does depth-first search over propagated states, and picks the branch cell by
   minimum remaining values. `solve_first` and `count_solutions` are both thin wrappers
   around it. Pass `limit = 2` to `count_solutions` to test uniqueness.

`SolveStats.branch_nodes` is the difficulty measure. It counts the guesses propagation
could not avoid.

**The generator digs** (`src/generator.rs`): draw a random complete grid with
`solve_first_with_rng` on an empty board, then remove clues in random order and keep a
removal only while exactly one solution remains and `branch_nodes` stays inside the
difficulty band. `Difficulty` bands must not overlap; a test enforces this. If no attempt
of `MAX_ATTEMPTS` lands in the band, `generate` returns the closest puzzle, so it always
gives back a valid unique puzzle.

**Determinism is a contract.** `rng::Rng` is SplitMix64, written in-crate so the project
keeps zero dependencies. The same seed and difficulty must always give the same puzzle.
A change to `Rng`, to the shuffle, or to the order the generator draws numbers will break
that. Do not add a dependency without a reason to give up the zero-dependency property.

**Board is the I/O boundary.** `FromStr` reads 81 cells, accepts `.`, `0`, `_`, or `*` for
blanks, and skips whitespace and grid separators, so a one-line puzzle and a drawn grid
both parse. It rejects a puzzle whose given digits already conflict. `Display` and
`to_line` write a compact form; `to_pretty` writes the boxed grid, which parses back.
