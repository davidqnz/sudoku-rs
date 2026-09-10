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
just bench                          # hyperfine: wall clock of the whole binary
just bench-rate                     # criterion: solutions per second
```

Generator tests solve many puzzles. Use `cargo test --release` if they feel slow.

Do not point an unbounded `--count` at `puzzles/two.txt` or
`puzzles/multiple.txt`: they are under-constrained and have far too many
solutions to enumerate. Pass `--limit` if you need to.

## Measuring

Two benchmarks, both on the same 20-clue grid, both enumerating its 31610
solutions -- a puzzle with one solution would only measure how fast the search
gets lucky.

`just bench` times the binary end to end. It is the quick check, and includes
about 1ms of process startup.

`just bench-rate` runs `benches/solver.rs` under criterion and reports a rate.
It has two cases: `count` is the search alone, and `visit` adds the `Board` built
per solution, which is a good third of the work on a grid with this many. The
puzzle is inlined there, next to the `SOLUTIONS` count criterion divides by to
get a rate; both cases assert it, so that the divisor cannot go stale against
the puzzle above it and quietly rescale every result. That assert is the
benchmark keeping its own arithmetic honest -- the solver is the test suite's
business, not the benchmark's.

`cargo test` does not build bench targets; `cargo clippy --all-targets` does.
Criterion is a dev-dependency, so the crate itself still has none, but it does
pull about 30 crates into the lockfile.

## Architecture

A 9x9 Sudoku engine (`src/lib.rs`, crate `sudoku_rs`) plus a text interface (`src/main.rs`).
The engine does no I/O. All printing, argument handling, and error messages stay in `main.rs`.

**Domains are bitmasks.** A cell is a `u16`. Bit 0 means digit 1, bit 8 means digit 9.
One bit set means the cell is solved; zero bits set means a contradiction. `Board` holds
81 of these and nothing else, so it is `Copy` and only 162 bytes. It is what the outside
world reads, writes, parses, and prints. Do not add heap data or non-`Copy` fields to it.

**Constraint structure is compile-time.** `board::UNITS` (27 all-different groups),
`board::PEERS` (the 20 cells each cell shares a unit with), and `board::CELL_UNITS` /
`CELL_UNIT_IDS` (the three units a cell belongs to, as a bitmask and as indices) are
`const fn` tables. Nothing about the constraint graph is built at runtime.

**The solver works on a transposed board.** `solver::Grid` is private and holds the same
information the other way round: nine `u128`s, one per digit, a bit per cell where that
digit is still possible. A `Board` answers "what may this cell be?"; propagation asks
"where may this digit go?" far more often, and of twenty cells at a time. Under the
transpose that is one word:

- striking a digit from all 20 peers of a cell is `digits[d] &= !PEER_MASK[cell]`, not a
  walk down `PEERS`;
- naked singles and empty domains, for all 81 cells at once, come from a nine-step fold
  (`Grid::spans`) rather than 81 population counts;
- a digit's homes in a unit are `digits[d] & UNIT_MASK[unit]`.

A `Grid` is 214 bytes and `Copy`, so `search` still branches on a copy rather than a trail
of undo records. A `Board` is built only at the boundary: once on the way in, and once per
solution actually handed to a visitor.

**The solver has two layers** (`src/solver.rs`):
1. `Grid::propagate` runs to a fixpoint, alternating arc consistency (`naked_singles`)
   with `hidden_singles`, which arc consistency alone cannot find. Two sets keep the
   second cheap, and both are why it is not simply re-derived each call:
   - `dirty`, a 27-bit set of units whose domains have moved. A unit nobody touched
     cannot have grown a hidden single. Mark it precisely, from the cells a strike
     actually changed -- marking conservatively from a table is measurably slower.
   - `placed`, the digits already sitting in each unit. Deep in the search most digits of
     most units are placed, and a placed digit has exactly one home forever, so skipping
     them is the difference between 9 tests per unit and 2 or 3.
2. `search` does depth-first search over propagated states, picking the branch cell by
   minimum remaining values. The parent is already at a fixpoint, so a child propagates
   from the one cell it guessed -- never re-seed a child from every solved cell.

**Solutions go to a `Sink`, not a closure.** `for_each_solution` builds a `Board` per
solution; `count_solutions` does not, because counting never looks. On a puzzle with tens
of thousands of solutions that transposition is a fifth of the run. Pass `limit = 2` to
`count_solutions` to test uniqueness. Enumerate with `for_each_solution`, never into a
`Vec` -- a grid with few clues has more solutions than memory can hold, so the caller must
break out or accept an endless stream.

**`Detail` chooses what gets counted.** `branch_nodes` and `backtracks` are per-node and
free. `assignments` and `eliminations` are counted inside propagation, which runs millions
of times, and cost about a tenth of a solve; `Detail::Search` drops them. It is a const
generic (`DETAILED`) threaded through propagation and search, so the counters compile
away rather than being branched over. `main.rs` asks for `Detail::Full` only when
`--stats` was passed.

`SolveStats.branch_nodes` is the difficulty measure. It counts the guesses propagation
could not avoid. It, and `backtracks`, must not change for a given puzzle: the generator
rates difficulty by `branch_nodes`, so a change to either changes what every seed
produces. `assignments` and `eliminations` are diagnostics and may shift, since
propagation stops the moment it finds a contradiction and the order it gets there in is
not fixed.

**The generator digs** (`src/generator.rs`): draw a random complete grid with
`solve_first_with_rng` on an empty board, then remove clues in random order and keep a
removal only while exactly one solution remains and `branch_nodes` stays inside the
difficulty band. `Difficulty` bands must not overlap; a test enforces this. If no attempt
of `MAX_ATTEMPTS` lands in the band, `generate` returns the closest puzzle, so it always
gives back a valid unique puzzle.

**Determinism is a contract.** `rng::Rng` is SplitMix64, written in-crate so the crate
keeps zero runtime dependencies (criterion, in `[dev-dependencies]`, is the only one). The same seed and difficulty must always give the same puzzle.
A change to `Rng`, to the shuffle, to the order the generator draws numbers, to which cell
`most_constrained` picks, or to what propagation can deduce will break that. When
changing the solver, diff `gen --seed N --line --stats` across a range of seeds and
difficulties: the grids, `clues`, `guesses`, and `backtracks` must all be unchanged. Do
not add a runtime dependency without a reason to give up that property.

**The release profile is tuned** (`lto = "fat"`, `codegen-units = 1`,
`panic = "abort"`), which is worth about 8% and matters because the solver is one crate
of small hot functions. Benchmark with `--release`; a debug build is not comparable.

**Board is the I/O boundary.** `FromStr` reads 81 cells, accepts `.`, `0`, `_`, or `*` for
blanks, and skips whitespace and grid separators, so a one-line puzzle and a drawn grid
both parse. It rejects a puzzle whose given digits already conflict. `Display` and
`to_line` write a compact form; `to_pretty` writes the boxed grid, which parses back.
