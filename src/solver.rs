//! Constraint propagation and backtracking search.
//!
//! Propagation is AC-3 specialised to all-different constraints. For a binary
//! `!=` arc, revising `D(x)` against `D(y)` can only ever remove a value when
//! `D(y)` is a singleton, so the generic queue of arcs collapses into placing
//! newly-solved cells. On top of that arc consistency we run hidden singles
//! -- a digit with only one home left in a unit -- which is the cheapest rule
//! that plain arc consistency cannot see.
//!
//! Inside, the search works on a [`Grid`] rather than a [`Board`]: the same
//! information transposed, so that the two things propagation does most often
//! are single machine words of work instead of loops over cells. A [`Board`]
//! is built only at the boundary -- once on the way in, and once per solution
//! handed to a visitor.

use std::ops::ControlFlow;

use crate::board::{ALL, ALL_UNITS, Board, CELL_UNIT_IDS, CELL_UNITS, PEERS, UNITS};
use crate::rng::Rng;

/// The board cannot be completed: some cell has no candidates left, or some
/// digit has nowhere to go in a unit.
#[derive(Debug, PartialEq, Eq)]
pub struct Contradiction;

/// Work done while solving. Useful for rating a puzzle's difficulty.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SolveStats {
    /// Cells fixed to a digit by propagation.
    pub assignments: usize,
    /// Candidates removed from domains.
    pub eliminations: usize,
    /// Cells search had to guess at, because propagation stalled.
    pub branch_nodes: usize,
    /// Guesses propagation then refuted.
    pub backtracks: usize,
}

/// Which of [`SolveStats`] the solver bothers to fill in.
///
/// `branch_nodes` and `backtracks` are counted once per search node, and are
/// always kept. `assignments` and `eliminations` are counted inside the
/// propagation loop, which on a puzzle with many solutions runs millions of
/// times; keeping them costs roughly a tenth of the solve.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Every counter, exactly.
    #[default]
    Full,
    /// Only the search's own counters. `assignments` and `eliminations` come
    /// back zero, which is what a caller wanting a difficulty rating -- or no
    /// stats at all -- should ask for.
    Search,
}

/// Every cell of the grid, as a bit each.
const FULL: u128 = (1u128 << 81) - 1;

/// The 20 peers of each cell, as a set of cells.
const PEER_MASK: [u128; 81] = build_peer_masks();

/// Each of the 27 units, as a set of cells.
const UNIT_MASK: [u128; 27] = build_unit_masks();

const fn build_peer_masks() -> [u128; 81] {
    let mut masks = [0u128; 81];
    let mut cell = 0;
    while cell < 81 {
        let mut k = 0;
        while k < 20 {
            masks[cell] |= 1u128 << PEERS[cell][k];
            k += 1;
        }
        cell += 1;
    }
    masks
}

const fn build_unit_masks() -> [u128; 27] {
    let mut masks = [0u128; 27];
    let mut unit = 0;
    while unit < 27 {
        let mut k = 0;
        while k < 9 {
            masks[unit] |= 1u128 << UNITS[unit][k];
            k += 1;
        }
        unit += 1;
    }
    masks
}

/// A board transposed: one 81-bit set of cells per digit, instead of one
/// 9-bit set of digits per cell.
///
/// A [`Board`] answers "what may this cell be?" in one load, which is what
/// makes it a good thing to read, print, and parse. Propagation asks the other
/// question far more often -- "where may this digit still go?" -- and asks it
/// of twenty cells at a time. Under this layout that answer is one word, so
/// striking a digit from every peer of a cell is a single `AND` rather than a
/// walk down `PEERS`, and testing all 81 cells for a lone candidate is a
/// nine-step fold rather than 81 population counts.
///
/// At 160 bytes it is still `Copy` and still cheaper to duplicate than to
/// journal, so search keeps branching on copies.
#[derive(Clone, Copy)]
struct Grid {
    /// `digits[d]` holds a bit per cell where digit `d + 1` is still possible.
    digits: [u128; 9],
    /// Cells already placed: narrowed to one digit *and* that digit struck
    /// from their peers. Propagation uses it to tell a cell it has already
    /// dealt with from one that has only just come down to a single digit.
    solved: u128,
    /// The digits already placed in each unit, as a 9-bit mask.
    ///
    /// A placed digit has exactly one home in its unit and always will, so it
    /// is the hidden-single rule's largest source of wasted work -- and deep in
    /// the search, most digits of most units are placed. Keeping the set costs
    /// three stores per placement and lets a sweep look only at the digits
    /// still in play.
    placed: [u16; 27],
}

impl Grid {
    /// Transposes a board. No propagation is done, and nothing is placed, so
    /// the caller must follow with [`Grid::seed`].
    fn from_board(board: &Board) -> Self {
        let mut digits = [0u128; 9];
        for cell in 0..81 {
            let mask = board.candidates(cell);
            for (digit, board_of_digit) in digits.iter_mut().enumerate() {
                *board_of_digit |= u128::from(mask >> digit & 1) << cell;
            }
        }
        Grid {
            digits,
            solved: 0,
            placed: [0; 27],
        }
    }

    /// Transposes back. Called once per solution, and once for the public
    /// [`propagate`], so it walks candidates rather than cells: a solved grid
    /// has 81 of them, where a cell-major loop would do 81 gathers of nine.
    fn to_board(self) -> Board {
        let mut cells = [0u16; 81];
        for (digit, board_of_digit) in self.digits.into_iter().enumerate() {
            let mut rest = board_of_digit;
            while rest != 0 {
                let cell = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                cells[cell] |= 1u16 << digit;
            }
        }
        Board::from_cells(cells)
    }

    /// The digits still open at `cell`, as a 9-bit mask.
    #[inline]
    fn candidates(&self, cell: usize) -> u16 {
        let bit = 1u128 << cell;
        let mut mask = 0u16;
        for (digit, &board_of_digit) in self.digits.iter().enumerate() {
            mask |= u16::from(board_of_digit & bit != 0) << digit;
        }
        mask
    }

    /// Cells with at least one candidate, and cells with at least two.
    ///
    /// Nine rounds of two `OR`s and an `AND` answer both questions for the
    /// whole grid: a cell reaching `at_least_two` was already in
    /// `at_least_one` when a second digit claimed it. `!at_least_one` is then
    /// the empty domains, and `at_least_one & !at_least_two` the single ones,
    /// which between them are everything arc consistency needs to look at.
    #[inline]
    fn spans(&self) -> (u128, u128) {
        let mut at_least_one = 0u128;
        let mut at_least_two = 0u128;
        for &board_of_digit in self.digits.iter() {
            at_least_two |= at_least_one & board_of_digit;
            at_least_one |= board_of_digit;
        }
        (at_least_one, at_least_two)
    }

    /// Fixes `cell` to `digit` when the cell still holds other candidates, as
    /// a guess or a hidden single does: those digits lose the cell first.
    #[inline]
    fn place<const DETAILED: bool>(
        &mut self,
        cell: usize,
        digit: usize,
        stats: &mut SolveStats,
        dirty: &mut u32,
    ) {
        debug_assert!(self.digits[digit] & (1u128 << cell) != 0, "digit not open");
        let cell_bit = 1u128 << cell;
        for board_of_digit in self.digits.iter_mut() {
            *board_of_digit &= !cell_bit;
        }
        self.digits[digit] |= cell_bit;
        self.place_single::<DETAILED>(cell, digit, stats, dirty);
    }

    /// Fixes `cell` to `digit`, striking the digit from all 20 peers and
    /// marking every unit whose domains moved. The cell must already be down
    /// to `digit` alone, which is how propagation finds every cell it places.
    ///
    /// This does not count an assignment: callers know whether the cell came
    /// from a clue, a guess, or propagation, and only the last is one.
    #[inline]
    fn place_single<const DETAILED: bool>(
        &mut self,
        cell: usize,
        digit: usize,
        stats: &mut SolveStats,
        dirty: &mut u32,
    ) {
        let cell_bit = 1u128 << cell;
        let peers = PEER_MASK[cell];
        let mut removed = self.digits[digit] & peers;
        self.digits[digit] &= !peers;
        self.solved |= cell_bit;

        *dirty |= CELL_UNITS[cell];
        let digit_bit = 1u16 << digit;
        for &unit in &CELL_UNIT_IDS[cell] {
            self.placed[usize::from(unit)] |= digit_bit;
        }

        if DETAILED {
            stats.eliminations += removed.count_ones() as usize;
        }
        while removed != 0 {
            let peer = removed.trailing_zeros() as usize;
            removed &= removed - 1;
            *dirty |= CELL_UNITS[peer];
        }
    }

    /// The digit at `cell` when it is down to one, or `Err` when it is down to
    /// none. A cell only ever loses candidates, so a cell that was a single
    /// when it was spotted is still a single or already empty.
    #[inline]
    fn lone_digit(&self, cell: usize) -> Result<usize, Contradiction> {
        let bit = 1u128 << cell;
        for (digit, &board_of_digit) in self.digits.iter().enumerate() {
            if board_of_digit & bit != 0 {
                return Ok(digit);
            }
        }
        Err(Contradiction)
    }

    /// Places every cell that already holds a single digit.
    ///
    /// Clues arrive this way, so a puzzle whose givens conflict is refuted
    /// here: placing the first of them empties the second.
    fn seed<const DETAILED: bool>(
        &mut self,
        stats: &mut SolveStats,
        dirty: &mut u32,
    ) -> Result<(), Contradiction> {
        let (at_least_one, at_least_two) = self.spans();
        let mut todo = at_least_one & !at_least_two;
        while todo != 0 {
            let cell = todo.trailing_zeros() as usize;
            todo &= todo - 1;
            self.place_single::<DETAILED>(cell, self.lone_digit(cell)?, stats, dirty);
        }
        Ok(())
    }

    /// Runs propagation to a fixpoint.
    ///
    /// `dirty` is the set of units whose domains have moved since they were
    /// last swept for hidden singles. A unit nobody touched cannot have grown
    /// one, so carrying this set is what keeps a node from re-reading all 27
    /// units after a cascade that reached only a handful of them.
    fn propagate<const DETAILED: bool>(
        &mut self,
        stats: &mut SolveStats,
        mut dirty: u32,
    ) -> Result<(), Contradiction> {
        loop {
            self.naked_singles::<DETAILED>(stats, &mut dirty)?;
            if dirty == 0 {
                return Ok(());
            }
            self.hidden_singles::<DETAILED>(stats, &mut dirty)?;
        }
    }

    /// Arc consistency: place every cell down to a single digit, until none
    /// are left. Also where an emptied domain is caught.
    fn naked_singles<const DETAILED: bool>(
        &mut self,
        stats: &mut SolveStats,
        dirty: &mut u32,
    ) -> Result<(), Contradiction> {
        loop {
            let (at_least_one, at_least_two) = self.spans();
            if at_least_one != FULL {
                return Err(Contradiction);
            }
            let mut todo = at_least_one & !at_least_two & !self.solved;
            if todo == 0 {
                return Ok(());
            }
            while todo != 0 {
                let cell = todo.trailing_zeros() as usize;
                todo &= todo - 1;
                // An earlier placement in this batch may have emptied the cell.
                let digit = self.lone_digit(cell)?;
                if DETAILED {
                    stats.assignments += 1;
                }
                self.place_single::<DETAILED>(cell, digit, stats, dirty);
            }
        }
    }

    /// One sweep of the hidden-single rule over the units marked dirty.
    ///
    /// A digit's homes in a unit are one `AND` away, so the rule reads
    /// directly: no homes refutes the board, and one home is a placement. Only
    /// digits still open in the unit are asked; a digit already placed there
    /// has one home too, and is the case worth not looking at.
    fn hidden_singles<const DETAILED: bool>(
        &mut self,
        stats: &mut SolveStats,
        dirty: &mut u32,
    ) -> Result<(), Contradiction> {
        while *dirty != 0 {
            let index = dirty.trailing_zeros() as usize;
            *dirty &= *dirty - 1;
            let unit = UNIT_MASK[index];
            let mut open = u32::from(!self.placed[index] & ALL);
            while open != 0 {
                let digit = open.trailing_zeros() as usize;
                open &= open - 1;
                let homes = self.digits[digit] & unit;
                if homes == 0 {
                    return Err(Contradiction);
                }
                if homes & (homes - 1) == 0 {
                    if DETAILED {
                        stats.assignments += 1;
                    }
                    // Reopens this unit as well: the digit's neighbours here
                    // were read before the placement.
                    self.place::<DETAILED>(homes.trailing_zeros() as usize, digit, stats, dirty);
                }
            }
        }
        Ok(())
    }

    /// The unsolved cell with the fewest candidates (minimum remaining
    /// values), or `None` when every cell is fixed.
    ///
    /// Two candidates is by far the common answer, and the fold hands that
    /// case over directly. Anything else falls back to counting, over only the
    /// cells that could still win.
    fn most_constrained(&self) -> Option<usize> {
        let mut at_least_one = 0u128;
        let mut at_least_two = 0u128;
        let mut at_least_three = 0u128;
        for &board_of_digit in self.digits.iter() {
            at_least_three |= at_least_two & board_of_digit;
            at_least_two |= at_least_one & board_of_digit;
            at_least_one |= board_of_digit;
        }

        let exactly_two = at_least_two & !at_least_three;
        if exactly_two != 0 {
            return Some(exactly_two.trailing_zeros() as usize);
        }

        let mut best = None;
        let mut best_len = 10;
        let mut rest = at_least_three;
        while rest != 0 {
            let cell = rest.trailing_zeros() as usize;
            rest &= rest - 1;
            let len = self.candidates(cell).count_ones();
            if len < best_len {
                best_len = len;
                best = Some(cell);
                if len == 3 {
                    break;
                }
            }
        }
        best
    }
}

/// Where the search sends the solutions it finds.
///
/// Transposing a grid back into a [`Board`] costs more than finding it did,
/// once a puzzle has solutions in the thousands, and a caller that only counts
/// them never looks. So the sink, not the search, decides whether a board gets
/// built at all.
trait Sink {
    fn solution(&mut self, grid: &Grid) -> ControlFlow<()>;
}

impl<S: Sink> Sink for &mut S {
    #[inline]
    fn solution(&mut self, grid: &Grid) -> ControlFlow<()> {
        (*self).solution(grid)
    }
}

/// Hands every solution to a visitor, as the [`Board`] it expects.
struct Visitor<F>(F);

impl<F> Sink for Visitor<F>
where
    F: FnMut(&Board) -> ControlFlow<()>,
{
    #[inline]
    fn solution(&mut self, grid: &Grid) -> ControlFlow<()> {
        (self.0)(&grid.to_board())
    }
}

/// Counts solutions and stops at a limit, building nothing.
struct Counter {
    count: usize,
    limit: usize,
}

impl Sink for Counter {
    #[inline]
    fn solution(&mut self, _grid: &Grid) -> ControlFlow<()> {
        self.count += 1;
        if self.count >= self.limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// Runs propagation to a fixpoint, narrowing domains in place.
///
/// Returning `Ok` does not mean the board is solved, only that it is arc
/// consistent and free of hidden singles.
pub fn propagate(board: &mut Board) -> Result<(), Contradiction> {
    let mut stats = SolveStats::default();
    let mut grid = Grid::from_board(board);
    let mut dirty = ALL_UNITS;
    grid.seed::<false>(&mut stats, &mut dirty)?;
    grid.propagate::<false>(&mut stats, dirty)?;
    *board = grid.to_board();
    Ok(())
}

/// Solves the puzzle, returning the first solution found.
pub fn solve_first(board: &Board) -> Option<(Board, SolveStats)> {
    solve_first_ordered(board, None)
}

/// Like [`solve_first`], but tries candidate digits in a random order. Feeding
/// it an empty board is how the generator draws a random complete grid.
pub fn solve_first_with_rng(board: &Board, rng: &mut Rng) -> Option<(Board, SolveStats)> {
    solve_first_ordered(board, Some(rng))
}

fn solve_first_ordered(board: &Board, rng: Option<&mut Rng>) -> Option<(Board, SolveStats)> {
    let mut solution = None;
    let mut stats = SolveStats::default();
    let mut rng = rng;
    let _ = solve::<_, true>(
        board,
        &mut Visitor(|found: &Board| {
            solution = Some(*found);
            ControlFlow::Break(())
        }),
        &mut stats,
        &mut rng,
    );
    solution.map(|solution| (solution, stats))
}

/// Calls `visit` with every solution, in search order, and returns the work
/// done. Return [`ControlFlow::Break`] from `visit` to stop the search early.
///
/// Solutions are never collected, so this is the way to enumerate a puzzle with
/// many of them: an under-constrained grid can have billions, which no `Vec`
/// could hold. A caller that wants a bounded number must break out itself.
///
/// ```
/// use std::ops::ControlFlow;
/// use sudoku_rs::{Board, solver};
///
/// // Four blanks in a rectangle, holding two digits that can swap.
/// let puzzle: Board = "534..8912672195348198342567859..1423426853791713924856\
///                      961537284287419635345286179".parse().unwrap();
/// let mut count = 0;
/// solver::for_each_solution(&puzzle, |_| {
///     count += 1;
///     ControlFlow::Continue(())
/// });
/// assert_eq!(count, 2);
/// ```
pub fn for_each_solution<F>(board: &Board, visit: F) -> SolveStats
where
    F: FnMut(&Board) -> ControlFlow<()>,
{
    for_each_solution_with_detail(board, visit, Detail::Full)
}

/// [`for_each_solution`], measuring only what `detail` asks for.
pub fn for_each_solution_with_detail<F>(board: &Board, mut visit: F, detail: Detail) -> SolveStats
where
    F: FnMut(&Board) -> ControlFlow<()>,
{
    let mut stats = SolveStats::default();
    let _ = solve_with(
        board,
        &mut Visitor(&mut visit),
        &mut stats,
        &mut None,
        detail,
    );
    stats
}

/// Counts solutions, stopping once `limit` have been found. Pass `2` to test
/// whether a puzzle is a true Sudoku with exactly one solution.
///
/// Counting never inspects a solution, so this does not pay to build one.
pub fn count_solutions(board: &Board, limit: usize) -> usize {
    count_solutions_with_stats(board, limit, Detail::Search).0
}

/// [`count_solutions`], with the work it took.
///
/// The count and the effort come from a single search, unlike counting through
/// [`for_each_solution`], which is the way to get at the solutions themselves.
pub fn count_solutions_with_stats(
    board: &Board,
    limit: usize,
    detail: Detail,
) -> (usize, SolveStats) {
    let mut stats = SolveStats::default();
    if limit == 0 {
        return (0, stats);
    }
    let mut counter = Counter { count: 0, limit };
    let _ = solve_with(board, &mut counter, &mut stats, &mut None, detail);
    (counter.count, stats)
}

/// Transposes the caller's board, propagates it, then hands it to [`search`].
fn solve<S: Sink, const DETAILED: bool>(
    board: &Board,
    sink: &mut S,
    stats: &mut SolveStats,
    rng: &mut Option<&mut Rng>,
) -> ControlFlow<()> {
    let mut grid = Grid::from_board(board);
    let mut dirty = ALL_UNITS;
    let seeded = grid
        .seed::<DETAILED>(stats, &mut dirty)
        .and_then(|()| grid.propagate::<DETAILED>(stats, dirty));
    if seeded.is_err() {
        stats.backtracks += 1;
        return ControlFlow::Continue(());
    }
    search::<S, DETAILED>(grid, sink, stats, rng)
}

/// Picks the instantiation of [`solve`] the caller asked for.
fn solve_with<S: Sink>(
    board: &Board,
    sink: &mut S,
    stats: &mut SolveStats,
    rng: &mut Option<&mut Rng>,
    detail: Detail,
) -> ControlFlow<()> {
    match detail {
        Detail::Full => solve::<S, true>(board, sink, stats, rng),
        Detail::Search => solve::<S, false>(board, sink, stats, rng),
    }
}

/// Depth-first search over propagated states. Returns [`ControlFlow::Break`]
/// once the visitor asks to stop, which unwinds the whole recursion.
///
/// `grid` arrives at a propagation fixpoint, and every child leaves at one, so
/// a child's propagation has only the cell this node guessed to spread.
fn search<S: Sink, const DETAILED: bool>(
    grid: Grid,
    sink: &mut S,
    stats: &mut SolveStats,
    rng: &mut Option<&mut Rng>,
) -> ControlFlow<()> {
    let Some(cell) = grid.most_constrained() else {
        return sink.solution(&grid);
    };

    stats.branch_nodes += 1;
    let mut digits = [0u8; 9];
    let mut count = 0;
    let mut mask = grid.candidates(cell);
    while mask != 0 {
        digits[count] = mask.trailing_zeros() as u8;
        mask &= mask - 1;
        count += 1;
    }
    let digits = &mut digits[..count];
    if let Some(rng) = rng.as_deref_mut() {
        rng.shuffle(digits);
    }

    for &digit in digits.iter() {
        // The grid is 160 bytes, so branching on a copy is cheaper and far
        // simpler than trailing every elimination for undo.
        let mut child = grid;
        let mut dirty = 0;
        child.place::<DETAILED>(cell, usize::from(digit), stats, &mut dirty);
        if child.propagate::<DETAILED>(stats, dirty).is_err() {
            stats.backtracks += 1;
            continue;
        }
        search::<S, DETAILED>(child, sink, stats, rng)?;
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EASY: &str = "\
        53..7....\
        6..195...\
        .98....6.\
        8...6...3\
        4..8.3..1\
        7...2...6\
        .6....28.\
        ...419..5\
        ....8..79";

    #[test]
    fn propagation_alone_solves_an_easy_puzzle() {
        let mut board: Board = EASY.parse().unwrap();
        propagate(&mut board).unwrap();
        assert!(board.is_solved());
        assert!(board.is_consistent());
    }

    #[test]
    fn propagation_detects_an_impossible_board() {
        // 1 and 2 are the only candidates left for three cells of the top row.
        let mut board = Board::empty();
        for (cell, digit) in [(3, 3), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8)] {
            board.assign(cell, digit);
        }
        for cell in [9, 18] {
            board.assign(cell, 9);
        }
        assert!(propagate(&mut board).is_err());
    }

    #[test]
    fn most_constrained_prefers_the_smallest_domain() {
        let mut board = Board::empty();
        board.eliminate(40, 1);
        for digit in 1..=7 {
            board.eliminate(20, digit);
        }
        assert_eq!(Grid::from_board(&board).most_constrained(), Some(20));
    }

    #[test]
    fn transposing_a_board_round_trips() {
        let board: Board = EASY.parse().unwrap();
        assert_eq!(Grid::from_board(&board).to_board(), board);

        // Not just solved cells: a partly narrowed grid must survive too.
        let mut narrowed = board;
        propagate(&mut narrowed).unwrap();
        assert_eq!(Grid::from_board(&narrowed).to_board(), narrowed);
    }

    #[test]
    fn candidates_agree_with_the_board_they_came_from() {
        let mut board: Board = EASY.parse().unwrap();
        board.eliminate(2, 4);
        board.eliminate(2, 7);
        let grid = Grid::from_board(&board);
        for cell in 0..81 {
            assert_eq!(grid.candidates(cell), board.candidates(cell), "cell {cell}");
        }
    }
}
