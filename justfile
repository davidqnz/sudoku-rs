_default:
    just --list

bench:
    cargo bench -p sudoku-core --bench solver

# Build the npm package into crates/sudoku-wasm/pkg (target: bundler, web, nodejs, ...)
wasm target="bundler":
    wasm-pack build crates/sudoku-wasm --release --target {{target}}

test-wasm:
    wasm-pack test --node crates/sudoku-wasm --release
