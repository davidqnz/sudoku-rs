//! Text interface to the Sudoku engine. All I/O, argument handling, and
//! formatting live here; `sudoku_rs` itself stays free of them.

use std::io::Read;
use std::ops::ControlFlow;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sudoku_rs::board::Board;
use sudoku_rs::generator::{Difficulty, generate};
use sudoku_rs::solver::{SolveStats, count_solutions, for_each_solution, solve_first};

const USAGE: &str = "\
sudoku-rs -- a constraint-propagation Sudoku solver

USAGE:
    sudoku-rs solve [FILE] [OPTIONS]   solve a puzzle (reads stdin if no FILE)
    sudoku-rs gen [OPTIONS]            generate a puzzle with one solution

OPTIONS:
    --line                  print the grid as one 81-character line
    --stats                 report solver effort on stderr
    --count                 solve: print how many solutions there are, no grid
    --all                   solve: print every solution, not just the first
    --limit N               solve: stop counting or printing after N solutions
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

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--line" => opts.line = true,
            "--stats" => opts.stats = true,
            "--count" => opts.count = true,
            "--all" => opts.all = true,
            "--limit" => {
                let value = rest.next().ok_or("--limit needs a number")?;
                let limit = value
                    .parse::<usize>()
                    .map_err(|e| format!("bad limit: {e}"))?;
                if limit == 0 {
                    return Err("--limit needs a number of 1 or more".into());
                }
                opts.limit = Some(limit);
            }
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

    if opts.count {
        return print_solution_count(&puzzle, &opts);
    }
    if opts.all || opts.limit.is_some() {
        return print_every_solution(&puzzle, &opts);
    }

    let started = Instant::now();
    let solved = solve_first(&puzzle);
    let elapsed = started.elapsed();

    let Some((solution, stats)) = solved else {
        return Err("this puzzle has no solution".into());
    };
    print_board(&solution, opts.line);

    // Counting is a second search, so the reported stats stay those of the one
    // solution above. That keeps `guesses` a difficulty measure of the puzzle.
    let counted = count_solutions(&puzzle, PLAIN_COUNT_CAP + 1);
    if counted > PLAIN_COUNT_CAP {
        eprintln!(">{PLAIN_COUNT_CAP} solutions");
    } else {
        eprintln!("{}", solutions_phrase(counted));
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
    all: bool,
    limit: Option<usize>,
}

/// How many solutions a plain `solve` counts before it reports ">1000". A
/// puzzle this ambiguous is under-constrained, so an exact figure tells the
/// user nothing an exhaustive `--count` would not tell them better.
const PLAIN_COUNT_CAP: usize = 1000;

/// Counts solutions without printing any. The count is this mode's whole
/// output, so it goes to stdout as a bare number a script can read.
fn print_solution_count(puzzle: &Board, opts: &Options) -> Result<(), String> {
    let limit = opts.limit.unwrap_or(usize::MAX);
    let mut count = 0usize;

    let started = Instant::now();
    let stats = for_each_solution(puzzle, |_| {
        count += 1;
        if count >= limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    let elapsed = started.elapsed();

    println!("{count}");
    if count >= limit {
        eprintln!("stopped at the --limit of {limit}");
    }
    if opts.stats {
        report_stats(&stats, puzzle.clue_count());
        eprintln!("  elapsed:      {elapsed:?}");
    }
    Ok(())
}

/// "1 solution" or "4 solutions".
fn solutions_phrase(count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} solution{plural}")
}

/// Prints each solution as the search finds it. Nothing is held in memory, so
/// a puzzle with millions of solutions costs no more than a puzzle with one.
/// The count and any warning go to stderr, to keep stdout a stream of grids.
fn print_every_solution(puzzle: &Board, opts: &Options) -> Result<(), String> {
    let limit = opts.limit.unwrap_or(usize::MAX);
    let mut count = 0usize;

    let started = Instant::now();
    let stats = for_each_solution(puzzle, |solution| {
        if count > 0 && !opts.line {
            println!();
        }
        print_board(solution, opts.line);
        count += 1;
        if count >= limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    let elapsed = started.elapsed();

    if count == 0 {
        return Err("this puzzle has no solution".into());
    }
    if count >= limit {
        eprintln!("{}; stopped at the --limit", solutions_phrase(count));
    } else {
        eprintln!("{}", solutions_phrase(count));
    }
    if opts.stats {
        report_stats(&stats, puzzle.clue_count());
        eprintln!("  elapsed:      {elapsed:?}");
    }
    Ok(())
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
