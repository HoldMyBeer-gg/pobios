//! Accessibility state.
//!
//! This lives at the root of the style system rather than bolted on later:
//! UI scale, font family, motion, and the colorblind palette all feed the
//! egui `Style` and are re-applied whenever any of them changes.

/// Colorblind palette selection. Recolors damage types *and* gem/socket
/// colors. Socket colors additionally carry R/G/B letter chips, so meaning is
/// never conveyed by hue alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum ColorMode {
    #[default]
    Default,
    Deuteranopia,
    Protanopia,
    Tritanopia,
    Monochrome,
}

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct A11y {
    /// UI scale, 0.8..=2.0 per the accessibility spec.
    pub ui_scale: f32,
    pub high_contrast: bool,
    pub color_mode: ColorMode,
    /// Swap Inter for OpenDyslexic throughout.
    pub dyslexic_font: bool,
    /// Zeroes all animation durations.
    pub reduced_motion: bool,
}

impl Default for A11y {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            high_contrast: false,
            color_mode: ColorMode::default(),
            dyslexic_font: false,
            reduced_motion: false,
        }
    }
}

impl A11y {
    pub const SCALE_MIN: f32 = 0.8;
    pub const SCALE_MAX: f32 = 2.0;

    /// Scale a motion duration, honouring reduced-motion.
    pub fn motion(&self, seconds: f32) -> f32 {
        if self.reduced_motion { 0.0 } else { seconds }
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.ui_scale = scale.clamp(Self::SCALE_MIN, Self::SCALE_MAX);
    }
}
