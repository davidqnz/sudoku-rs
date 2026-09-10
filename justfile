_default:
    just --list

bench:
    cargo build --release
    hyperfine './target/release/sudoku-rs solve puzzles/30k.txt --count'

