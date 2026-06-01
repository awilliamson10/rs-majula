/// Describes the lifecycle behavior of a world entity (location or object).
///
/// Entities created from the map data use `Respawn` -- they are permanent fixtures that
/// reappear after being changed or removed. Entities spawned at runtime use `Despawn` --
/// they are temporary and disappear after their timer expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntityLifeTime {
    /// Permanent entity loaded from the map. Reverts to its base state after modification.
    Respawn = 0,
    /// Temporary entity spawned at runtime. Removed once its lifetime expires.
    Despawn = 1,
}
