//! A Dota 2 map's navigation grid (`maps/<map>.gnv`): one byte of flags for each square cell
//! of the ground, the first bit telling whether a unit may stand on it (checked cell by cell
//! against the game's own walkability of the main map).

use crate::bytes::Cursor;
use crate::error::{Result, malformed};

/// The first four bytes of every grid.
const MAGIC: u32 = 0xFADE_BEAD;
/// The most cells a side may have, and the furthest the first cell may lie from the origin.
pub const MAX_SIDE: u32 = 4096;
/// The longest cell edge, in world units: a grid of [`MAX_SIDE`] such cells spans far more than
/// any map.
pub const MAX_EDGE: f32 = 4096.0;
/// The bytes before the cells.
const HEADER_LEN: usize = 32;

/// A map's navigation grid: `width` by `height` cells of `edge` units, the first of them the cell
/// `min`. Cell `(x, y)` covers the ground from `x * edge` to `(x + 1) * edge` on the first axis
/// (and so on the second), in the map's own frame.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
    /// When the bytes are not a navigation grid (another magic), the cell edge is not a finite
    /// number in `(0, MAX_EDGE]` or the centre not finite (NaN and infinities are refused), a side
    /// is zero or longer than [`MAX_SIDE`], the first cell lies further than [`MAX_SIDE`] from the
    /// origin, or the cells do not fill the grid exactly.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes, "gnv");
        let magic = c.u32()?;
        if magic != MAGIC {
            return Err(malformed("gnv", format!("magic {magic:08x}")));
        }
        let mut float = || -> Result<f32> { c.array().map(f32::from_le_bytes) };
        let edge = float()?;
        let center = [float()?, float()?];
        if !(edge.is_finite() && edge > 0.0 && edge <= MAX_EDGE) {
            return Err(malformed("gnv", format!("cell edge {edge}")));
        }
        if !center.iter().all(|v| v.is_finite()) {
            return Err(malformed("gnv", format!("cell centre {center:?}")));
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

    /// A grid with floats `edge` and `center` and the given cells.
    fn grid_with(edge: f32, center: [f32; 2], width: u32, height: u32, cells: &[u8]) -> Vec<u8> {
        let mut out = grid(width, height, [0, 0], cells);
        out[4..8].copy_from_slice(&edge.to_le_bytes());
        out[8..12].copy_from_slice(&center[0].to_le_bytes());
        out[12..16].copy_from_slice(&center[1].to_le_bytes());
        out
    }

    #[test]
    fn a_cell_edge_or_centre_that_is_not_a_finite_size_is_an_error() {
        for edge in [0.0, -64.0, f32::NAN, f32::INFINITY, MAX_EDGE * 2.0] {
            let err = GridNav::read(&grid_with(edge, [32.0, 32.0], 1, 1, &[1])).unwrap_err();
            assert!(
                err.to_string().ends_with(&format!("cell edge {edge}")),
                "{edge}: {err}"
            );
        }
        for centre in [f32::NAN, f32::NEG_INFINITY] {
            let err = GridNav::read(&grid_with(64.0, [32.0, centre], 1, 1, &[1])).unwrap_err();
            assert!(
                err.to_string()
                    .ends_with(&format!("cell centre [32.0, {centre:?}]")),
                "{centre}: {err}"
            );
        }
        assert!(GridNav::read(&grid_with(MAX_EDGE, [32.0, 32.0], 1, 1, &[1])).is_ok());
    }

    #[test]
    fn a_side_of_no_cells_or_beyond_the_limit_is_an_error() {
        for (width, height) in [(0, 1), (1, 0), (1, MAX_SIDE + 1)] {
            let err = GridNav::read(&grid(width, height, [0, 0], &[])).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("{width} by {height} cells")),
                "{err}"
            );
        }
    }

    #[test]
    fn a_first_cell_beyond_the_limit_is_an_error_and_one_at_it_is_read() {
        let far = MAX_SIDE as i32 + 1;
        for min in [[far, 0], [0, -far], [i32::MIN, 0]] {
            let err = GridNav::read(&grid(1, 1, min, &[1])).unwrap_err();
            assert!(err.to_string().contains("first cell"), "{min:?}: {err}");
        }
        let edge = MAX_SIDE as i32;
        let nav = GridNav::read(&grid(1, 1, [-edge, edge], &[1])).unwrap();
        assert!(nav.walkable(-edge, edge));
    }

    #[test]
    fn a_grid_of_the_largest_size_is_read() {
        let side = MAX_SIDE as usize;
        let nav = GridNav::read(&grid(MAX_SIDE, MAX_SIDE, [0, 0], &vec![1; side * side])).unwrap();
        assert!(nav.walkable(MAX_SIDE as i32 - 1, MAX_SIDE as i32 - 1));
    }

    #[test]
    fn a_header_cut_short_is_an_error() {
        let bytes = grid(1, 1, [0, 0], &[1]);
        for len in [0, 3, 16, HEADER_LEN - 1] {
            assert!(GridNav::read(&bytes[..len]).is_err(), "{len} bytes");
        }
    }
}
