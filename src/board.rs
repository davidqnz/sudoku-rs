//! The 9x9 board and its constraint structure.
//!
//! A cell is a `u16` bitmask of the digits it may still take: bit 0 means "1",
//! bit 8 means "9". A cell with exactly one bit set is solved, and a cell with
//! no bits set is a contradiction. Everything the solver does to a domain --
//! assigning, eliminating, testing for a single -- is a bit operation.

use std::fmt;
use std::str::FromStr;

/// Every digit 1..=9 as a bitmask.
pub const ALL: u16 = 0x1FF;

/// The bitmask for a single digit.
///
/// # Panics
///
/// If `digit` is not in `1..=9`. Without this check, a release build turns `0`
/// into bit 15, which is outside [`ALL`] and makes the cell look empty.
#[inline]
pub const fn digit_bit(digit: u8) -> u16 {
    assert!(digit >= 1 && digit <= 9, "digit must be in 1..=9");
    1u16 << (digit - 1)
}

/// The digit held by a singleton mask.
#[inline]
pub const fn mask_digit(mask: u16) -> u8 {
    mask.trailing_zeros() as u8 + 1
}

/// The 27 all-different constraints: 9 rows, then 9 columns, then 9 boxes.
pub const UNITS: [[u8; 9]; 27] = build_units();

/// The 20 cells each cell shares a unit with. This is the arc structure of the
/// CSP; the solver walks it instead of materialising arcs.
pub const PEERS: [[u8; 20]; 81] = build_peers();

/// The three units -- row, column, box -- each cell belongs to, as a mask of
/// bits into [`UNITS`]. Propagation uses it to remember which units a changed
/// domain has made worth re-examining.
pub const CELL_UNITS: [u32; 81] = build_cell_units();

/// Every unit at once, for a sweep that cannot assume anything is clean.
pub const ALL_UNITS: u32 = (1 << 27) - 1;

/// The same three units as [`CELL_UNITS`], as indices rather than a mask, for
/// the callers that want to visit them rather than test them.
pub const CELL_UNIT_IDS: [[u8; 3]; 81] = build_cell_unit_ids();

const fn build_cell_unit_ids() -> [[u8; 3]; 81] {
    let mut ids = [[0u8; 3]; 81];
    let mut cell = 0;
    while cell < 81 {
        let row = cell / 9;
        let col = cell % 9;
        ids[cell] = [
            row as u8,
            (9 + col) as u8,
            (18 + (row / 3) * 3 + col / 3) as u8,
        ];
        cell += 1;
    }
    ids
}

const fn build_cell_units() -> [u32; 81] {
    let mut masks = [0u32; 81];
    let mut cell = 0;
    while cell < 81 {
        let row = cell / 9;
        let col = cell % 9;
        let boxx = (row / 3) * 3 + col / 3;
        masks[cell] = (1 << row) | (1 << (9 + col)) | (1 << (18 + boxx));
        cell += 1;
    }
    masks
}

const fn build_units() -> [[u8; 9]; 27] {
    let mut units = [[0u8; 9]; 27];
    let mut i = 0;
    while i < 9 {
        let mut j = 0;
        while j < 9 {
            units[i][j] = (i * 9 + j) as u8;
            units[9 + i][j] = (j * 9 + i) as u8;
            let box_row = (i / 3) * 3 + j / 3;
            let box_col = (i % 3) * 3 + j % 3;
            units[18 + i][j] = (box_row * 9 + box_col) as u8;
            j += 1;
        }
        i += 1;
    }
    units
}

const fn build_peers() -> [[u8; 20]; 81] {
    let mut peers = [[0u8; 20]; 81];
    let mut cell = 0;
    while cell < 81 {
        let row = cell / 9;
        let col = cell % 9;
        let mut n = 0;
        let mut k = 0;
        while k < 9 {
            let other = row * 9 + k;
            if other != cell {
                peers[cell][n] = other as u8;
                n += 1;
            }
            k += 1;
        }
        k = 0;
        while k < 9 {
            let other = k * 9 + col;
            if other != cell {
                peers[cell][n] = other as u8;
                n += 1;
            }
            k += 1;
        }
        // The box contributes the 4 cells not already covered by row or column.
        k = 0;
        while k < 9 {
            let other = ((row / 3) * 3 + k / 3) * 9 + (col / 3) * 3 + k % 3;
            if other / 9 != row && other % 9 != col {
                peers[cell][n] = other as u8;
                n += 1;
            }
            k += 1;
        }
        cell += 1;
    }
    peers
}

/// A Sudoku grid as 81 candidate sets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Board {
    cells: [u16; 81],
}

impl Board {
    /// A board with every digit still possible everywhere.
    pub const fn empty() -> Self {
        Board { cells: [ALL; 81] }
    }

    /// A board straight from 81 candidate masks.
    ///
    /// This is how the solver hands back a grid it worked on in its own
    /// representation. It is deliberately not public: outside the crate a
    /// board is built by parsing or by assigning, both of which keep the
    /// invariant that a mask is a subset of [`ALL`].
    pub(crate) const fn from_cells(cells: [u16; 81]) -> Self {
        Board { cells }
    }

    /// The candidate mask at `idx`.
    #[inline]
    pub fn candidates(&self, idx: usize) -> u16 {
        self.cells[idx]
    }

    /// The digit at `idx`, if the cell has been narrowed to one.
    #[inline]
    pub fn get_digit(&self, idx: usize) -> Option<u8> {
        let mask = self.cells[idx];
        if mask.count_ones() == 1 {
            Some(mask_digit(mask))
        } else {
            None
        }
    }

    /// Narrow `idx` to exactly `digit`.
    #[inline]
    pub fn assign(&mut self, idx: usize, digit: u8) {
        self.cells[idx] = digit_bit(digit);
    }

    /// Widen `idx` back to every digit, undoing a clue.
    #[inline]
    pub fn clear(&mut self, idx: usize) {
        self.cells[idx] = ALL;
    }

    /// Drop `digit` from the domain of `idx`. Returns whether anything changed.
    #[inline]
    pub fn eliminate(&mut self, idx: usize, digit: u8) -> bool {
        let bit = digit_bit(digit);
        if self.cells[idx] & bit == 0 {
            false
        } else {
            self.cells[idx] &= !bit;
            true
        }
    }

    /// Whether every cell is down to a single digit.
    pub fn is_solved(&self) -> bool {
        self.cells.iter().all(|c| c.count_ones() == 1)
    }

    /// How many cells are already fixed to a digit.
    pub fn clue_count(&self) -> usize {
        self.cells.iter().filter(|c| c.count_ones() == 1).count()
    }

    /// Whether the fixed cells respect every all-different constraint.
    pub fn is_consistent(&self) -> bool {
        self.first_conflict().is_none()
    }

    /// The first (cell, digit) that repeats a digit within a unit.
    fn first_conflict(&self) -> Option<(usize, u8)> {
        for unit in UNITS.iter() {
            let mut seen = 0u16;
            for &cell in unit {
                let mask = self.cells[cell as usize];
                if mask.count_ones() == 1 {
                    if seen & mask != 0 {
                        return Some((cell as usize, mask_digit(mask)));
                    }
                    seen |= mask;
                }
            }
        }
        None
    }

    /// The 81-character single-line form, `.` for blanks.
    pub fn to_line(&self) -> String {
        (0..81)
            .map(|i| match self.get_digit(i) {
                Some(d) => char::from(b'0' + d),
                None => '.',
            })
            .collect()
    }

    /// The grid drawn with box separators, for human eyes.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        for row in 0..9 {
            if row % 3 == 0 {
                out.push_str(if row == 0 {
                    "┌───────┬───────┬───────┐\n"
                } else {
                    "├───────┼───────┼───────┤\n"
                });
            }
            for col in 0..9 {
                if col % 3 == 0 {
                    out.push_str("│ ");
                }
                match self.get_digit(row * 9 + col) {
                    Some(d) => out.push(char::from(b'0' + d)),
                    None => out.push('.'),
                }
                out.push(' ');
            }
            out.push_str("│\n");
        }
        out.push_str("└───────┴───────┴───────┘");
        out
    }
}

/// Why a puzzle string could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// A character that is neither a digit, a blank marker, nor whitespace.
    BadChar(char),
    /// The input held a number of cells other than 81.
    BadLength(usize),
    /// A given digit repeats within a row, column, or box.
    Conflict { cell: usize, digit: u8 },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::BadChar(c) => write!(f, "unexpected character {c:?} in puzzle"),
            ParseError::BadLength(n) => write!(f, "expected 81 cells, found {n}"),
            ParseError::Conflict { cell, digit } => write!(
                f,
                "digit {digit} repeats in a unit at row {}, column {}",
                cell / 9 + 1,
                cell % 9 + 1
            ),
        }
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Board {
    type Err = ParseError;

    /// Reads 81 cells, using `.`, `0`, `_`, or `*` for blanks. Whitespace and the
    /// usual grid separators are skipped, so a one-line puzzle and a drawn
    /// 9-line grid both parse.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut board = Board::empty();
        let mut n = 0;
        for ch in s.chars() {
            let blank = match ch {
                c if c.is_whitespace() => continue,
                '|' | '-' | '+' | '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴'
                | '┼' => continue,
                '.' | '0' | '_' | '*' => true,
                '1'..='9' => false,
                other => return Err(ParseError::BadChar(other)),
            };
            if n >= 81 {
                n += 1;
                continue;
            }
            if !blank {
                board.assign(n, ch as u8 - b'0');
            }
            n += 1;
        }
        if n != 81 {
            return Err(ParseError::BadLength(n));
        }
        match board.first_conflict() {
            Some((cell, digit)) => Err(ParseError::Conflict { cell, digit }),
            None => Ok(board),
        }
    }
}

impl fmt::Display for Board {
    /// Nine lines of digits, `.` for blanks.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..9 {
            if row > 0 {
                writeln!(f)?;
            }
            for col in 0..9 {
                match self.get_digit(row * 9 + col) {
                    Some(d) => write!(f, "{d}")?,
                    None => write!(f, ".")?,
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Board({})", self.to_line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_are_the_twenty_cells_sharing_a_unit() {
        for (cell, cell_peers) in PEERS.iter().enumerate() {
            let mut peers = cell_peers.to_vec();
            peers.sort_unstable();
            peers.dedup();
            assert_eq!(
                peers.len(),
                20,
                "cell {cell} has duplicate or missing peers"
            );
            assert!(!peers.contains(&(cell as u8)));
        }
        // Top-left cell: its row, its column, and the rest of its box.
        let mut expected: Vec<u8> = (1..9)
            .chain((1..9).map(|k| k * 9))
            .chain([10, 11, 19, 20])
            .collect();
        expected.sort_unstable();
        let mut actual = PEERS[0].to_vec();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }

    #[test]
    #[should_panic(expected = "digit must be in 1..=9")]
    fn assigning_zero_panics() {
        Board::empty().assign(0, 0);
    }

    #[test]
    #[should_panic(expected = "digit must be in 1..=9")]
    fn eliminating_ten_panics() {
        Board::empty().eliminate(0, 10);
    }

    #[test]
    fn every_cell_belongs_to_exactly_three_units() {
        for cell in 0..81u8 {
            let count = UNITS.iter().filter(|u| u.contains(&cell)).count();
            assert_eq!(count, 3, "cell {cell}");
        }
    }
}
