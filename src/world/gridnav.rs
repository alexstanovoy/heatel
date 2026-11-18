//! A Dota 2 map's navigation grid (`maps/<map>.gnv`): one byte of flags for each square cell
//! of the ground, the first bit telling whether a unit may stand on it.

use crate::bytes::Cursor;
use crate::error::{Result, malformed};

/// The first four bytes of every grid.
const MAGIC: u32 = 0xFADE_BEAD;
/// The most cells a side may have; Dota's map has 320 by 328.
const MAX_SIDE: u32 = 4096;
/// The bytes before the cells.
const HEADER_LEN: usize = 32;

/// A map's navigation grid: `width` by `height` cells of `edge` units, the first of them the cell
/// `min`. Cell `(x, y)` covers the ground from `x * edge` to `(x + 1) * edge` on the first axis
/// (and so on the second), in the map's own frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GridNav {
    /// A cell's side, in world units (64 on Dota's maps).
    pub edge: f32,
    /// Where in a cell its centre lies, from the cell's lowest corner (half the edge on Dota's
    /// maps).
    pub center: [f32; 2],
    /// The lowest cell on each axis.
    pub min: [i32; 2],
    /// Cells on the first axis.
    pub width: u32,
    /// Cells on the second axis.
    pub height: u32,
    /// Each cell's flags, rows of the first axis from the lowest second coordinate up.
    cells: Vec<u8>,
}

impl GridNav {
    /// The flag of a cell a unit may stand on: what the map's navigation counts as walkable
    /// before trees and buildings are placed on it.
    pub const WALKABLE: u8 = 1;

    /// Reads a `.gnv` file.
    ///
    /// # Errors
    ///
    /// When the bytes are not a navigation grid, are larger than any map's, or end early.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes, "gnv");
        let magic = c.u32()?;
        if magic != MAGIC {
            return Err(malformed("gnv", format!("magic {magic:08x}")));
        }
        let mut float = || -> Result<f32> { c.array().map(f32::from_le_bytes) };
        let edge = float()?;
        let center = [float()?, float()?];
        if !(edge.is_finite() && edge > 0.0) || !center.iter().all(|v| v.is_finite()) {
            return Err(malformed(
                "gnv",
                format!("cell edge {edge}, centre {center:?}"),
            ));
        }
        let (width, height) = (c.u32()?, c.u32()?);
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(malformed("gnv", format!("{width} by {height} cells")));
        }
        let min = [c.i32()?, c.i32()?];
        if min.iter().any(|m| m.unsigned_abs() > MAX_SIDE) {
            return Err(malformed("gnv", format!("first cell {min:?}")));
        }
        debug_assert_eq!(c.pos(), HEADER_LEN);
        let count = width as usize * height as usize;
        if c.remaining() != count {
            return Err(malformed(
                "gnv",
                format!("{} bytes of cells for {width} by {height}", c.remaining()),
            ));
        }
        let cells = c.take(count)?.to_vec();
        Ok(Self {
            edge,
            center,
            min,
            width,
            height,
            cells,
        })
    }

    /// The flags of cell `(x, y)`, absent outside the grid.
    pub fn flags(&self, x: i32, y: i32) -> Option<u8> {
        let column = u32::try_from(i64::from(x) - i64::from(self.min[0])).ok()?;
        let row = u32::try_from(i64::from(y) - i64::from(self.min[1])).ok()?;
        if column >= self.width || row >= self.height {
            return None;
        }
        self.cells
            .get(row as usize * self.width as usize + column as usize)
            .copied()
    }

    /// Whether a unit may stand on cell `(x, y)`; a cell outside the grid is not walkable.
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        self.flags(x, y)
            .is_some_and(|flags| flags & Self::WALKABLE != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(width: u32, height: u32, min: [i32; 2], cells: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_le_bytes().to_vec();
        for f in [64.0f32, 32.0, 32.0] {
            out.extend(f.to_le_bytes());
        }
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        out.extend(min[0].to_le_bytes());
        out.extend(min[1].to_le_bytes());
        out.extend(cells);
        out
    }

    #[test]
    fn a_cell_is_found_by_its_coordinates_from_the_first_cell() {
        let nav = GridNav::read(&grid(3, 2, [-1, -5], &[0, 1, 0, 0, 0, 0x11])).unwrap();
        assert!(nav.walkable(0, -5));
        assert!(nav.walkable(1, -4));
        assert!(!nav.walkable(-1, -5));
        assert_eq!(nav.flags(1, -4), Some(0x11));
    }

    #[test]
    fn a_cell_outside_the_grid_has_no_flags_and_is_not_walkable() {
        let nav = GridNav::read(&grid(1, 1, [0, 0], &[1])).unwrap();
        for (x, y) in [(1, 0), (0, 1), (-1, 0), (i32::MIN, i32::MAX)] {
            assert_eq!(nav.flags(x, y), None);
            assert!(!nav.walkable(x, y));
        }
    }

    #[test]
    fn cells_that_do_not_fill_the_grid_exactly_are_an_error() {
        for cells in [&[1u8, 1, 1][..], &[1, 1, 1, 1, 1]] {
            let err = GridNav::read(&grid(2, 2, [0, 0], cells)).unwrap_err();
            assert!(err.to_string().contains("for 2 by 2"), "{err}");
        }
    }

    #[test]
    fn a_grid_larger_than_any_map_is_refused_before_reading_its_cells() {
        let err = GridNav::read(&grid(MAX_SIDE + 1, 1, [0, 0], &[])).unwrap_err();
        assert!(err.to_string().contains("4097 by 1 cells"), "{err}");
    }

    #[test]
    fn another_format_is_refused_by_its_magic() {
        let mut bytes = grid(1, 1, [0, 0], &[1]);
        bytes[0] = 0;
        let err = GridNav::read(&bytes).unwrap_err();
        assert!(err.to_string().contains("magic fadebe00"), "{err}");
    }
}
