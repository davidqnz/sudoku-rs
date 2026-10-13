import { useEffect, useState } from "react";
import * as G from "./game";
import type { Difficulty, Game, Hint } from "./game";

const SAVE_KEY = "sudoku-game";
const DIGITS = [1, 2, 3, 4, 5, 6, 7, 8, 9];

function load(): Game | null {
  try {
    const game: unknown = JSON.parse(localStorage.getItem(SAVE_KEY) ?? "null");
    return G.isGame(game) ? game : null;
  } catch {
    return null;
  }
}

function save(game: Game) {
  try {
    localStorage.setItem(SAVE_KEY, JSON.stringify(game));
  } catch {
    // Storage is full or blocked. The game continues; it is not saved.
  }
}

/** Row and column steps for each arrow key. Movement wraps within the line. */
const ARROWS: Record<string, [number, number] | undefined> = {
  ArrowUp: [-1, 0],
  ArrowDown: [1, 0],
  ArrowLeft: [0, -1],
  ArrowRight: [0, 1],
};

const clock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

export default function App() {
  const [game, setGame] = useState<Game>(() => load() ?? G.generate("easy"));
  const [difficulty, setDifficulty] = useState<Difficulty>(game.difficulty);
  const [selected, setSelected] = useState<number | null>(null);
  const [notesMode, setNotesMode] = useState(false);
  const [check, setCheck] = useState(false);
  const [hint, setHint] = useState<{ hint: Hint; revealed: boolean } | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const solved = G.isSolved(game);
  const clashes = G.conflicts(game);
  const wrong = G.mistakes(game);
  const selectedDigit = selected === null ? 0 : game.cells[selected];

  useEffect(() => save(game), [game]);

  useEffect(() => {
    if (solved) return;
    const timer = setInterval(() => {
      if (!document.hidden) setGame((g) => ({ ...g, seconds: g.seconds + 1 }));
    }, 1000);
    return () => clearInterval(timer);
  }, [solved]);

  function move(change: (game: Game) => Game) {
    if (solved) return;
    setGame(change);
    setHint(null);
    setMessage(null);
  }

  function begin(next: Game) {
    setGame(next);
    setSelected(null);
    setHint(null);
    setMessage(null);
  }

  function newGame() {
    const started = game.history.length > 0 && !solved;
    if (!started || confirm("Abandon this game?")) begin(G.generate(difficulty));
  }

  function importPuzzle() {
    const text = prompt("Paste a puzzle: 81 cells, with . or 0 for blanks.");
    if (!text) return;
    const problem = G.problemWith(text);
    if (problem) setMessage(problem);
    else begin(G.start(text));
  }

  function enter(digit: number) {
    if (selected === null) return;
    move((g) => (notesMode ? G.toggleNote(g, selected, digit) : G.place(g, selected, digit)));
  }

  /** One button, three steps: where to look, what goes there, fill it in. */
  function nextHint() {
    if (!hint || !hint.hint.answer) {
      const found = G.hint(game);
      if (found) setHint({ hint: found, revealed: false });
      else setMessage("Nothing left to hint.");
    } else if (!hint.revealed) {
      setHint({ ...hint, revealed: true });
      setSelected(hint.hint.answer.cell);
    } else {
      const { cell, digit } = hint.hint.answer;
      move((g) => G.place(g, cell, digit));
    }
  }

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      // Leave browser shortcuts such as Ctrl+1 or Cmd+H to the browser.
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.target instanceof HTMLSelectElement) return;
      const key = e.key.toLowerCase();
      const arrow = ARROWS[e.key];
      if (arrow) {
        e.preventDefault();
        const [down, right] = arrow;
        setSelected((cell) =>
          cell === null
            ? 0
            : ((G.row(cell) + down + 9) % 9) * 9 + ((G.col(cell) + right + 9) % 9),
        );
      } else if (/^[1-9]$/.test(key)) enter(Number(key));
      else if (["0", "backspace", "delete"].includes(key)) {
        if (selected !== null) move((g) => G.erase(g, selected));
      } else if (key === "n") setNotesMode((on) => !on);
      else if (key === "z" || key === "u") move(G.undo);
      else if (key === "h") nextHint();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const hintLabel = !hint?.hint.answer ? "Hint" : hint.revealed ? "Fill it in" : "Show answer";
  const highlighted = hint?.revealed ? [hint.hint.answer!.cell] : (hint?.hint.cells ?? []);

  return (
    <main>
      <header>
        <h1>Sudoku</h1>
        <div className="controls">
          <select value={difficulty} onChange={(e) => setDifficulty(e.target.value as Difficulty)}>
            <option value="easy">Easy</option>
            <option value="medium">Medium</option>
            <option value="hard">Hard</option>
          </select>
          <button onClick={newGame}>New game</button>
          <button onClick={importPuzzle}>Import</button>
        </div>
      </header>

      <div className="status">
        <span className="difficulty">{game.difficulty}</span>
        <span>{clock(game.seconds)}</span>
      </div>

      <div className="board">
        {game.cells.map((digit, cell) => {
          const classes = ["cell"];
          if (G.isGiven(game, cell)) classes.push("given");
          if (cell === selected) classes.push("selected");
          else if (selected !== null && G.sees(cell, selected)) classes.push("peer");
          if (digit && digit === selectedDigit) classes.push("same");
          if (clashes[cell]) classes.push("clash");
          if (check && wrong[cell]) classes.push("wrong");
          if (highlighted.includes(cell)) classes.push("hinted");
          return (
            <div key={cell} className={classes.join(" ")} onClick={() => setSelected(cell)}>
              {digit ? digit : <Notes mask={game.notes[cell]} digit={selectedDigit} />}
            </div>
          );
        })}
      </div>

      {solved && (
        <div className="banner">
          Solved in {clock(game.seconds)}! <button onClick={() => begin(G.generate(difficulty))}>Play again</button>
        </div>
      )}
      {hint && (
        <div className="banner">{hint.revealed ? hint.hint.answer!.text : hint.hint.nudge}</div>
      )}
      {message && <div className="banner">{message}</div>}

      <div className="pad">
        {DIGITS.map((d) => {
          const left = 9 - game.cells.filter((c) => c === d).length;
          return (
            <button key={d} className={left <= 0 ? "used" : ""} onClick={() => enter(d)}>
              {d}
              <small>{Math.max(left, 0)}</small>
            </button>
          );
        })}
      </div>

      <div className="tools">
        <button onClick={() => move(G.undo)} disabled={game.history.length === 0}>Undo</button>
        <button onClick={() => selected !== null && move((g) => G.erase(g, selected))}>Erase</button>
        <button aria-pressed={notesMode} onClick={() => setNotesMode(!notesMode)}>Notes</button>
        <button onClick={() => move(G.fillNotes)}>Fill notes</button>
        <button aria-pressed={check} onClick={() => setCheck(!check)}>Check</button>
        <button onClick={nextHint} disabled={solved}>{hintLabel}</button>
      </div>

      <p className="keys">Keys: 1–9 enter · 0 erase · arrows move · N notes · Z undo · H hint</p>
    </main>
  );
}

function Notes({ mask, digit }: { mask: number; digit: number }) {
  return (
    <div className="notes">
      {DIGITS.map((d) => (
        <span key={d} className={d === digit ? "same" : ""}>
          {mask & G.bit(d) ? d : ""}
        </span>
      ))}
    </div>
  );
}
