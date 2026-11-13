//! Font stack.
//!
//! Three roles, per the style contract:
//!   - Cinzel Decorative: masthead + engraved section heads, ALL CAPS only.
//!   - Inter: all UI text (swapped for OpenDyslexic when a11y requests it).
//!   - JetBrains Mono: numbers, stat values, hashes, zoom %.
//!
//! A system/default font rendering anywhere is a bug: it means these were not
//! installed into `FontDefinitions`.

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// Named families beyond egui's built-in Proportional/Monospace.
pub fn display() -> FontFamily {
    FontFamily::Name("cinzel".into())
}

const INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter.ttf");
const JETBRAINS: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono.ttf");
const CINZEL_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/CinzelDecorative-Bold.ttf");
const CINZEL_BLACK: &[u8] = include_bytes!("../../../../assets/fonts/CinzelDecorative-Black.ttf");
const OPENDYSLEXIC: &[u8] = include_bytes!("../../../../assets/fonts/OpenDyslexic-Regular.otf");
const OPENDYSLEXIC_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/OpenDyslexic-Bold.otf");

/// Build the font set. `dyslexic` swaps the proportional family for
/// OpenDyslexic while leaving the mono and display families alone.
pub fn definitions(dyslexic: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();

    let mut insert = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };

    insert("inter", INTER);
    insert("jetbrains", JETBRAINS);
    insert("cinzel", CINZEL_BOLD);
    insert("cinzel-black", CINZEL_BLACK);
    insert("opendyslexic", OPENDYSLEXIC);
    insert("opendyslexic-bold", OPENDYSLEXIC_BOLD);

    let body = if dyslexic { "opendyslexic" } else { "inter" };

    fonts
        .families
        .insert(FontFamily::Proportional, vec![body.to_owned()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["jetbrains".to_owned()]);
    fonts.families.insert(
        FontFamily::Name("cinzel".into()),
        vec!["cinzel".to_owned(), "cinzel-black".to_owned()],
    );

    fonts
}
