# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo test                          # every crate: unit, integration, and doctests
cargo test -p sudoku-core --test solver   # only crates/sudoku-core/tests/solver.rs
cargo test escargot                 # one test, by substring of its name
cargo test -p sudoku-core --lib     # only the #[cfg(test)] modules in the engine
cargo clippy --all-targets
cargo clippy -p sudoku-wasm --all-targets --target wasm32-unknown-unknown
cargo fmt
cargo run -- gen --difficulty hard --stats
cargo run --release -- solve puzzle.txt --count --stats
just bench                          # criterion: solutions per second
just test-wasm                      # wasm-pack test --node: the bindings under a JS host
just wasm [target]                  # wasm-pack build into crates/sudoku-wasm/pkg
just web                            # the React game in web/, on the Vite dev server
(cd web && npm run build)           # rebuilds the wasm package, type-checks, bundles
```

The wasm recipes need the `wasm32-unknown-unknown` target and `wasm-pack`.
`cargo test` builds the wasm crate natively but runs none of its tests: they
are `wasm_bindgen_test`s gated on `target_arch = "wasm32"`.

Generator tests solve many puzzles. Use `cargo test --release` if they feel slow.

Do not point an unbounded `--count` at `puzzles/two.txt` or
`puzzles/multiple.txt`: they are under-constrained and have far too many
solutions to enumerate. Pass `--limit` if you need to.

## Measuring

The benchmark uses a 20-clue grid and enumerates its 31610 solutions -- a
puzzle with one solution would only measure how fast the search gets lucky.

`just bench` runs `crates/sudoku-core/benches/solver.rs` under criterion and reports a rate.
It has two cases: `count` is the search alone, and `visit` adds the `Board` built
per solution, which is a good third of the work on a grid with this many. The
puzzle is inlined there, next to the `SOLUTIONS` count criterion divides by to
get a rate; both cases assert it, so that the divisor cannot go stale against
the puzzle above it and quietly rescale every result. That assert is the
benchmark keeping its own arithmetic honest -- the solver is the test suite's
business, not the benchmark's.

`cargo test` does not build bench targets; `cargo clippy --all-targets` does.
Criterion is a dev-dependency, so the engine itself still has none, but it does
pull about 30 crates into the lockfile.

## Architecture

A Cargo workspace of three crates under `crates/`:

- `sudoku-core` (lib `sudoku_core`): the 9x9 engine. It does no I/O and has no runtime
  dependencies. Paths below like `src/solver.rs` are relative to this crate.
- `sudoku-cli` (binary `sudoku-rs`): the text interface. All printing, argument
  handling, and error messages stay in its `main.rs`.
- `sudoku-wasm`: JS bindings over the engine via `wasm-bindgen` (see the end of this file).

The release profile lives in the root `Cargo.toml`, since Cargo ignores profiles
set in member crates.

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
- naked singles and empty domains, for all 81 cells at once, come from a fold over the
  nine boards (`Grid::ranks`) rather than 81 population counts. The fold merges in pairs,
  not left to right: "has a candidate, has two, has three" is associative, so the nine
  boards go four deep instead of nine and the dependency chain stops being the limit.
  `most_constrained` reads all three ranks; `spans` reads the first two, and the compiler
  drops the third once the fold is inlined;
- the digit of each naked single comes out of `Grid::digit_planes`: four unions of the
  boards, one per bit of a digit's index, which for a one-candidate cell spell that digit
  out in binary. Four bit tests, in place of searching the boards for the one holding the
  cell;
- a digit's homes in a unit are `digits[d] & UNIT_MASK[unit]`. No homes refutes the board
  and one home is a placement, and `homes == homes.isolate_lowest_one()` is exactly one or
  the other, so the common case takes no branch at all -- and the lone home is already the
  cell's own bit, so the placement needs nothing looked up.

Scan an 81-bit set as two words, never as one: a `u128` has no trailing-zero count of its
own, and the high half is empty most times it is asked. `for_each_cell` does this; use it
when only the index is wanted. Carry a cell's bit alongside its
index wherever both are wanted -- the low bit of the mask being scanned already is it.

A `Grid` is 224 bytes and `Copy`, so `search` still branches on a copy rather than a trail
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

   A sweep that places nothing has left the domains where the naked singles before it
   found them, so `hidden_singles` reports whether it placed anything and that ends the
   loop. Without it every propagation closes on a pass that can only confirm the one
   before it.
2. `search` does depth-first search over propagated states, picking the branch cell by
   minimum remaining values. The parent is already at a fixpoint, so a child propagates
   from the one cell it guessed -- never re-seed a child from every solved cell. At a
   fixpoint every one-candidate cell has been placed, so `solved == FULL` is exactly the
   grid with nothing left to branch on: `most_constrained` answers half its calls, the one
   per solution, without folding anything.

**Solutions go to a `Sink`, not a closure.** `for_each_solution` builds a `Board` per
solution; `count_solutions` does not, because counting never looks. A grid only ever
reaches a sink complete, which is what lets that `Board` be built by `to_solution_board`,
writing each cell once instead of reading it back to merge into. On a puzzle with tens
of thousands of solutions that transposition is a fifth of the run. Pass `limit = 2` to
`count_solutions` to test uniqueness. Enumerate with `for_each_solution`, never into a
`Vec` -- a grid with few clues has more solutions than memory can hold, so the caller must
break out or accept an endless stream.

**`Detail` chooses what gets counted.** `branch_nodes` and `backtracks` are per-node and
free. `assignments` and `eliminations` are counted inside propagation, which runs millions
of times, and cost about a tenth of a solve; `Detail::Search` drops them. It is a const
generic (`DETAILED`) threaded through propagation and search, so the counters compile
away rather than being branched over. The CLI's `main.rs` asks for `Detail::Full` only when
`--stats` was passed.

`SolveStats.branch_nodes` is the difficulty measure. It counts the guesses propagation
could not avoid. The generator rates difficulty by it, so a solver change that moves
`branch_nodes` also changes what each seed produces. That is allowed while the algorithm
is still changing; say so when a change does it. `assignments` and `eliminations` are
diagnostics and may shift, since propagation stops the moment it finds a contradiction
and the order it gets there in is not fixed.

**The generator digs** (`src/generator.rs`): draw a random complete grid with
`solve_first_with_rng` on an empty board, then remove clues in random order and keep a
removal only while exactly one solution remains and `branch_nodes` stays inside the
difficulty band. `Difficulty` bands must not overlap; a test enforces this. If no attempt
of `MAX_ATTEMPTS` lands in the band, `generate` returns the closest puzzle, so it always
gives back a valid unique puzzle.

**Determinism is per build.** Within one build, the same seed and difficulty must always
give the same puzzle; `generation_is_reproducible_from_its_seed` checks this. Across
builds, seed output may change. Do not add golden or snapshot tests that fix it. A change
to `Rng`, to the shuffle, to the order the generator draws numbers, to which cell
`most_constrained` picks, or to what propagation can deduce changes seed output. That is
expected, but report it: diff `gen --seed N --line --stats` across a few seeds and
difficulties before and after.

`rng::Rng` is SplitMix64, written in-crate so the engine keeps zero runtime dependencies
(criterion, in `[dev-dependencies]`, is the only one). Do not add a runtime dependency
without a reason.

**The release profile is tuned** (`lto = "fat"`, `codegen-units = 1`,
`panic = "abort"`), which is worth about 8% and matters because the solver is one crate
of small hot functions. Benchmark with `--release`; a debug build is not comparable.

**Hints are for people** (`src/hint.rs`). They read a board as a player sees it: a
one-candidate cell is filled, and any other cell is blank. `candidates` strikes only the
digits of filled peers, and never chains one deduction into the next the way propagation
does. That way every pencil mark and every hint is something a person can check on the
grid in front of them. `hint` offers hidden singles in boxes first, then in rows and
columns, then naked singles. When none is left it falls back to `Technique::Solution`:
the MRV cell's digit, taken from a solve. Placing a single and re-deriving candidates is
the same fixpoint `propagate` reaches, so a puzzle rated `Easy` (`branch_nodes == 0`)
always falls to singles alone, and a test holds that. `hint` returns `None` for a board
it cannot solve, which on a unique puzzle is how a wrong entry shows up.
`Board::conflicts` marks both cells of every clash, for highlighting.

`Difficulty::for_branch_nodes` maps a solve's cost onto the generator's bands, and
`generator::rate` applies it to any puzzle with exactly one solution. `generate` can fall
back to the closest puzzle, so what it returns should be labelled with `rate`, not with
the difficulty that was asked for. Many "hard" requests currently come back medium.

**Board is the I/O boundary.** `FromStr` reads 81 cells, accepts `.`, `0`, `_`, or `*` for
blanks, and skips whitespace and grid separators, so a one-line puzzle and a drawn grid
both parse. It rejects a puzzle whose given digits already conflict. `Display` and
`to_line` write a compact form; `to_pretty` writes the boxed grid, which parses back.

**The wasm crate is a thin boundary** (`crates/sudoku-wasm`). Like the CLI, it adds no
logic of its own. Puzzles cross it as strings, in any form `FromStr` accepts, and come
back as `to_line` strings. Parse failures throw a JS `Error` with the `ParseError` message.
It exports `solve`, `countSolutions`, `forEachSolution`, `generate`, `rate`, `toLine`,
and `toPretty` over puzzles. A player's grid is different: it may break the rules,
and `FromStr` would reject it. So `conflicts`, `candidates`, and `hint` take a
`Uint8Array` of 81 digits instead, `0` for blank, built with `Board::assign`.
Difficulty, technique, and house kind are string enums, so they come out as TS unions.
`forEachSolution` streams solutions to a JS callback rather than returning an array,
for the same reason `for_each_solution` never collects: returning `false` stops it, and
a callback that throws stops the search and is rethrown. Its dependencies are
`wasm-bindgen` and `js-sys`. It needs `js-sys` for `Function` and for `Math.random`,
which seeds an unseeded `generate`, because wasm32-unknown-unknown has no clock to
seed from. Keep those dependencies out of `sudoku-core`.

**The web app** (`web/`) is React + Vite + TypeScript, deliberately small: `game.ts`
holds the game as plain, JSON-serializable data and every move as a pure function, and
`App.tsx` is the whole UI. It links the wasm package with `file:../crates/sudoku-wasm/pkg`,
built with `--target web`, so there are no Vite wasm plugins; `main.tsx` awaits `init()`
before rendering. Sudoku rules come from the engine -- the app never decides a conflict,
a candidate, or a hint itself. It only tracks notes, undo history, the timer, and which
entries differ from the solution.
