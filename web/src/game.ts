// Game state and moves, as pure functions over plain data so a game can be
// saved to localStorage as-is. The rules -- conflicts, candidates, hints,
// ratings -- all come from the engine; this file only keeps score.

import * as engine from "sudoku-wasm";
import type { Difficulty, HouseKind } from "sudoku-wasm";

export type { Difficulty };

interface Snapshot {
  cells: number[];
  notes: number[];
}

export interface Game extends Snapshot {
  /** The givens, 81 characters with `.` for blanks. */
  puzzle: string;
  solution: string;
  difficulty: Difficulty;
  /** Earlier states, most recent last, for undo. */
  history: Snapshot[];
  seconds: number;
}

export function start(puzzle: string): Game {
  const line = engine.toLine(puzzle);
  const cells = toDigits(line);
  return {
    puzzle: line,
    solution: engine.solve(line)!.grid,
    difficulty: engine.rate(line)!,
    cells,
    notes: cells.map(() => 0),
    history: [],
    seconds: 0,
  };
}

export function generate(difficulty: Difficulty): Game {
  return start(engine.generate(difficulty).puzzle);
}

/** Checks pasted text before it becomes a game. Returns an error message, or null. */
export function problemWith(text: string): string | null {
  try {
    if (engine.rate(text)) return null;
    return engine.countSolutions(text, 1) === 0
      ? "That puzzle has no solution."
      : "That puzzle has more than one solution.";
  } catch (error) {
    return `That isn't a puzzle: ${(error as Error).message}.`;
  }
}

/** Whether `value`, read back from storage, has the shape of a `Game`. */
export function isGame(value: unknown): value is Game {
  const game = value as Partial<Game> | null;
  const isGrid = (grid: unknown) => Array.isArray(grid) && grid.length === 81;
  return (
    typeof game?.puzzle === "string" &&
    game.puzzle.length === 81 &&
    typeof game.solution === "string" &&
    game.solution.length === 81 &&
    typeof game.difficulty === "string" &&
    typeof game.seconds === "number" &&
    isGrid(game.cells) &&
    isGrid(game.notes) &&
    Array.isArray(game.history)
  );
}

export const toDigits =(line: string) => Array.from(line, (c) => (c === "." ? 0 : Number(c)));
export const isGiven = (game: Game, cell: number) => game.puzzle[cell] !== ".";
export const isSolved = (game: Game) => game.cells.join("") === game.solution;
export const bit = (digit: number) => 1 << (digit - 1);

export const conflicts = (game: Game) => engine.conflicts(Uint8Array.from(game.cells));

/** Filled cells that do not match the solution, whether or not they clash. */
export const mistakes = (game: Game) =>
  game.cells.map((digit, cell) => digit !== 0 && String(digit) !== game.solution[cell]);

export const row = (cell: number) => Math.floor(cell / 9);
export const col = (cell: number) => cell % 9;
export const box = (cell: number) => Math.floor(row(cell) / 3) * 3 + Math.floor(col(cell) / 3);
export const sees = (a: number, b: number) =>
  row(a) === row(b) || col(a) === col(b) || box(a) === box(b);

function record(game: Game, change: Partial<Snapshot>): Game {
  const { cells, notes } = game;
  return { ...game, ...change, history: [...game.history, { cells, notes }] };
}

/** Writes `digit` into `cell`, or clears it if it is already there. */
export function place(game: Game, cell: number, digit: number): Game {
  if (isGiven(game, cell)) return game;
  const cells = [...game.cells];
  cells[cell] = cells[cell] === digit ? 0 : digit;
  // A placed digit can no longer be a candidate anywhere it can see.
  const notes = game.notes.map((mask, other) =>
    other === cell ? 0 : cells[cell] && sees(cell, other) ? mask & ~bit(digit) : mask,
  );
  return record(game, { cells, notes });
}

export function toggleNote(game: Game, cell: number, digit: number): Game {
  if (game.cells[cell] !== 0) return game;
  const notes = [...game.notes];
  notes[cell] ^= bit(digit);
  return record(game, { notes });
}

export function erase(game: Game, cell: number): Game {
  if (isGiven(game, cell) || (game.cells[cell] === 0 && game.notes[cell] === 0)) return game;
  const cells = [...game.cells];
  const notes = [...game.notes];
  if (cells[cell] !== 0) cells[cell] = 0;
  else notes[cell] = 0;
  return record(game, { cells, notes });
}

/** Pencils every candidate into every blank cell. */
export function fillNotes(game: Game): Game {
  const candidates = engine.candidates(Uint8Array.from(game.cells));
  const notes = game.cells.map((digit, cell) => (digit ? 0 : candidates[cell]));
  return record(game, { notes });
}

export function undo(game: Game): Game {
  const previous = game.history.at(-1);
  if (!previous) return game;
  return { ...game, ...previous, history: game.history.slice(0, -1) };
}

/**
 * The next step, in two parts: a nudge that says where to look, and the
 * answer. `cells` is what to highlight for the nudge.
 */
export interface Hint {
  cells: number[];
  nudge: string;
  answer?: { cell: number; digit: number; text: string };
}

export function hint(game: Game): Hint | null {
  const wrong = mistakes(game).flatMap((bad, cell) => (bad ? [cell] : []));
  if (wrong.length > 0) {
    const entries = wrong.length === 1 ? "entry is" : `${wrong.length} entries are`;
    return { cells: wrong, nudge: `The highlighted ${entries} wrong.` };
  }
  const step = engine.hint(Uint8Array.from(game.cells));
  if (!step) return null;
  const { cell, digit } = step;
  const answer = { cell, digit, text: `${name(cell)} is ${digit}.` };
  switch (step.technique) {
    case "hiddenSingle": {
      const kind = step.house!;
      const index = step.houseIndex!;
      const house = houseCells(kind, index);
      return {
        cells: house,
        nudge: `Look at ${kind} ${index + 1}: one digit has only one place it can go.`,
        answer: { ...answer, text: `${digit} can only go at ${name(cell)} in this ${kind}.` },
      };
    }
    case "nakedSingle":
      return {
        cells: [cell],
        nudge: `Look at ${name(cell)}: its row, column, and box leave only one digit.`,
        answer,
      };
    case "solution":
      return {
        cells: [cell],
        nudge: `There are no singles left to find. ${name(cell)} has the fewest candidates.`,
        answer,
      };
  }
}

/** "r3c5": row and column, counted from 1. */
const name = (cell: number) => `r${row(cell) + 1}c${col(cell) + 1}`;

function houseCells(kind: HouseKind, index: number): number[] {
  const all = [...Array(81).keys()];
  if (kind === "row") return all.filter((cell) => row(cell) === index);
  if (kind === "column") return all.filter((cell) => col(cell) === index);
  return all.filter((cell) => box(cell) === index);
}
