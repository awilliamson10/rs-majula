/// Represents the eight cardinal and ordinal directions plus a sentinel `None` value.
///
/// Direction values `0..=7` map to the same indices used by `dir_delta` and `face_dir`
/// in the pathing module. The `repr(i8)` layout allows `-1` for `None`, indicating
/// no direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum Direction {
    /// No direction; sentinel value indicating the entity is not facing or moving.
    None = -1,
    /// North-west (dx=-1, dz=+1).
    NorthWest = 0,
    /// North (dx=0, dz=+1).
    North = 1,
    /// North-east (dx=+1, dz=+1).
    NorthEast = 2,
    /// West (dx=-1, dz=0).
    West = 3,
    /// East (dx=+1, dz=0).
    East = 4,
    /// South-west (dx=-1, dz=-1).
    SouthWest = 5,
    /// South (dx=0, dz=-1).
    South = 6,
    /// South-east (dx=+1, dz=-1).
    SouthEast = 7,
}
