//! An edit to the build's header line, applied by `BuildState::apply`.
//! Lifted from the desktop build bar so the engine-facing state has no UI
//! crate behind it.

/// A change the user made. Applied by the caller so the engine can
/// recalculate and every panel sees the result.
pub enum BuildEdit {
    Level(i64),
    LevelAuto,
    Class(u8),
    Ascendancy(u8),
    /// Switch every set to the one belonging to this loadout.
    Loadout(String),
    /// Create a loadout: one set of each kind, all sharing a name.
    NewLoadout,
}
