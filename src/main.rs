//! Text interface to the Sudoku engine. All I/O, argument handling, and
//! formatting live here; `sudoku_rs` itself stays free of them.

use std::io::Read;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sudoku_rs::board::Board;
use sudoku_rs::generator::{Difficulty, generate};
use sudoku_rs::solver::{SolveStats, count_solutions, solve_first};

const USAGE: &str = "\
sudoku-rs -- a constraint-propagation Sudoku solver

USAGE:
    sudoku-rs solve [FILE] [OPTIONS]   solve a puzzle (reads stdin if no FILE)
    sudoku-rs gen [OPTIONS]            generate a puzzle with one solution

OPTIONS:
    --line                  print the grid as one 81-character line
    --stats                 report solver effort on stderr
    --count                 solve: warn if the puzzle has more than one solution
    --difficulty LEVEL      gen: easy, medium, or hard (default medium)
    --seed N                gen: seed the generator for a reproducible puzzle
    -h, --help              show this message

A puzzle is 81 cells reading left to right, top to bottom, with '.', '0', or
'_' for blanks. Whitespace and grid separators are ignored, so both a single
line and a drawn 9-line grid are accepted.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("solve") => solve_command(&args[1..]),
        Some("gen" | "generate") => generate_command(&args[1..]),
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
        None => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

fn solve_command(args: &[String]) -> Result<(), String> {
    let mut opts = Options::default();
    let mut path: Option<&str> = None;

    for arg in args {
        match arg.as_str() {
            "--line" => opts.line = true,
            "--stats" => opts.stats = true,
            "--count" => opts.count = true,
            flag if flag.starts_with('-') => return Err(unknown_flag(flag)),
            file if path.is_none() => path = Some(file),
            extra => return Err(format!("unexpected argument {extra:?}")),
        }
    }

    let input = match path {
        Some(file) => {
            std::fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?
        }
        None => read_stdin()?,
    };
    let puzzle: Board = input.parse().map_err(|e| format!("{e}"))?;

    let started = Instant::now();
    let solved = solve_first(&puzzle);
    let elapsed = started.elapsed();

    let Some((solution, stats)) = solved else {
        return Err("this puzzle has no solution".into());
    };
    print_board(&solution, opts.line);

    if opts.count && count_solutions(&puzzle, 2) > 1 {
        eprintln!("warning: this puzzle has more than one solution");
    }
    if opts.stats {
        report_stats(&stats, puzzle.clue_count());
        eprintln!("  elapsed:      {elapsed:?}");
    }
    Ok(())
}

fn generate_command(args: &[String]) -> Result<(), String> {
    let mut opts = Options::default();
    let mut difficulty = Difficulty::Medium;
    let mut seed = None;

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--line" => opts.line = true,
            "--stats" => opts.stats = true,
            "--difficulty" => {
                let value = rest.next().ok_or("--difficulty needs a level")?;
                difficulty = value.parse()?;
            }
            "--seed" => {
                let value = rest.next().ok_or("--seed needs a number")?;
                seed = Some(value.parse::<u64>().map_err(|e| format!("bad seed: {e}"))?);
            }
            other => return Err(unknown_flag(other)),
        }
    }

    let seed = seed.unwrap_or_else(random_seed);
    let started = Instant::now();
    let (puzzle, stats) = generate(difficulty, seed);
    let elapsed = started.elapsed();

    print_board(&puzzle, opts.line);
    if opts.stats {
        eprintln!("  seed:         {seed}");
        eprintln!("  difficulty:   {difficulty}");
        report_stats(&stats, puzzle.clue_count());
        eprintln!("  elapsed:      {elapsed:?}");
    }
    Ok(())
}

#[derive(Default)]
struct Options {
    line: bool,
    stats: bool,
    count: bool,
}

fn print_board(board: &Board, as_line: bool) {
    if as_line {
        println!("{}", board.to_line());
    } else {
        println!("{}", board.to_pretty());
    }
}

/// Stats go to stderr so a `--line` grid can still be piped onward.
fn report_stats(stats: &SolveStats, clues: usize) {
    eprintln!("  clues:        {clues}");
    eprintln!("  assignments:  {}", stats.assignments);
    eprintln!("  eliminations: {}", stats.eliminations);
    eprintln!("  guesses:      {}", stats.branch_nodes);
    eprintln!("  backtracks:   {}", stats.backtracks);
}

fn read_stdin() -> Result<String, String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("cannot read stdin: {e}"))?;
    Ok(buf)
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn unknown_flag(flag: &str) -> String {
    format!("unknown option {flag:?}\n\n{USAGE}")
}
