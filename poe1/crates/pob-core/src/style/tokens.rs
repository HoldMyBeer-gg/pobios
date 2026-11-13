//! RustyDemon design tokens.
//!
//! Every value here comes from `tokens.md` verbatim. Nothing in this file is
//! invented; if a value is missing, measure it off a screenshot and add it
//! here rather than inlining a literal at the call site.

use egui::Color32;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// Surfaces and structure.
pub mod color {
    use super::*;

    pub const BG_APP: Color32 = rgb(0x05080c);
    pub const BG_PANEL: Color32 = rgb(0x0d1420);
    pub const BG_INSET: Color32 = rgb(0x0a1018);
    pub const BG_HOVER: Color32 = rgb(0x16202e);

    pub const BORDER_SUBTLE: Color32 = rgb(0x1a2432);
    pub const BORDER_DEFAULT: Color32 = rgb(0x2a3542);

    pub const FG_PRIMARY: Color32 = rgb(0xe6ecf3);
    pub const FG_SECONDARY: Color32 = rgb(0x8a9aaf);

    pub const FROST_600: Color32 = rgb(0x5a6a7e);
    pub const FROST_700: Color32 = rgb(0xaab6c6);
    pub const FROST_900: Color32 = rgb(0xe8ecf2);

    pub const EMBER_500: Color32 = rgb(0xd85a12);
    pub const EMBER_600: Color32 = rgb(0xf07820);
    pub const EMBER_700: Color32 = rgb(0xffb066);
    pub const FG_ON_EMBER: Color32 = rgb(0x0a0d11);

    pub const RUNE_500: Color32 = rgb(0x5b93e6);
    pub const DANGER: Color32 = rgb(0xe65757);
    /// Positive change: a stat moving the way you want it to.
    ///
    /// Unspecified by the design tokens, which name DANGER and WARNING but no
    /// counterpart — nothing in the screenshots showed a gain. Rather than
    /// invent a hue, this reuses the existing dexterity green so the palette
    /// gains no new colour. Paired with a signed number everywhere it is
    /// used, so the meaning never rests on colour alone.
    pub const SUCCESS: Color32 = rgb(0x6fe07c);
    pub const WARNING: Color32 = rgb(0xe0c060);
}

/// Damage-type palette. These are the *default* (no colorblind mode) values;
/// `pal()` in the prototype recolors them per accessibility mode, so route
/// reads through the palette state rather than referencing these directly
/// once colorblind modes land.
pub mod damage {
    use super::*;

    pub const FIRE: Color32 = rgb(0xE08A3C);
    pub const COLD: Color32 = rgb(0x5B93E6);
    pub const LIGHTNING: Color32 = rgb(0xD6D25A);
    pub const CHAOS: Color32 = rgb(0xE657AE);
    pub const PHYS: Color32 = rgb(0xD8DEE6);
    pub const LIFE: Color32 = rgb(0xF07050);
    pub const MANA: Color32 = rgb(0x5B93E6);
    pub const ES: Color32 = rgb(0x9FD8F0);
    pub const STR: Color32 = rgb(0xF07050);
    pub const DEX: Color32 = rgb(0x6FE07C);
    pub const INT: Color32 = rgb(0x8B93FF);
}

/// Corner radii. Never pill, never fully round.
pub mod radius {
    pub const CHIP: u8 = 3;
    pub const BUTTON: u8 = 4;
    pub const PANEL: u8 = 6;
    pub const MODAL: u8 = 10;
}

/// 4px spacing grid.
pub mod space {
    pub const GRID: f32 = 4.0;
    pub const CONTROL_GAP: f32 = 6.0;
    pub const TOOLBAR_PAD: f32 = 6.0;
    pub const PANEL_PAD: f32 = 8.0;
    /// Masthead row height, measured from `screenshots/01-tree.png`.
    pub const MASTHEAD_H: f32 = 48.0;
}

/// Motion durations in seconds. Reduced-motion sets all of these to zero;
/// read them through the accessibility state, not directly.
pub mod motion {
    pub const HOVER_FADE: f32 = 0.120;
    pub const PANEL_EXPAND: f32 = 0.220;
}
