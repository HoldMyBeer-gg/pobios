//! egui `Style` bootstrap for the RustyDemon skin.
//!
//! Dark only; there is no light theme. Elevation is a 1px border, never a
//! shadow: drop shadows appear on popovers/tooltips/modals and nowhere else.

pub mod a11y;
pub mod fonts;
pub mod tokens;

pub use a11y::A11y;

use self::tokens::{color, radius, space};
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals, epaint::Shadow,
};

/// Text roles. `Heading` is the engraved Cinzel caps used by section headers;
/// `Monospace` carries every number, per the type contract.
fn text_styles(style: &mut Style) {
    use FontFamily::{Monospace, Proportional};

    style.text_styles = [
        (TextStyle::Heading, FontId::new(11.0, fonts::display())),
        (TextStyle::Body, FontId::new(12.5, Proportional)),
        (TextStyle::Button, FontId::new(12.5, Proportional)),
        (TextStyle::Small, FontId::new(11.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
    ]
    .into();
}

fn visuals(a11y: &A11y) -> Visuals {
    let mut v = Visuals::dark();

    v.override_text_color = Some(color::FG_PRIMARY);
    v.panel_fill = color::BG_PANEL;
    v.window_fill = color::BG_APP;
    v.extreme_bg_color = color::BG_INSET;

    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::BORDER_DEFAULT);
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.bg_fill = color::BG_HOVER;
    v.widgets.hovered.weak_bg_fill = color::BG_HOVER;

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(radius::BUTTON);
    }

    // Ember selection, never OS blue.
    v.selection.bg_fill = color::EMBER_500;
    v.selection.stroke = Stroke::new(1.0, color::EMBER_600);
    v.hyperlink_color = color::RUNE_500;

    // Borders, not shadows. Popovers re-enable their own.
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(153), // rgba(0,0,0,.6)
    };

    v.window_stroke = Stroke::new(1.0, color::BORDER_DEFAULT);
    v.window_corner_radius = CornerRadius::same(radius::PANEL);

    if a11y.high_contrast {
        v.override_text_color = Some(Color32::WHITE);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::FROST_700);
    }

    v
}

/// Apply the full skin. Call once at startup, and again whenever [`A11y`]
/// changes, since scale/font/contrast all feed into it.
pub fn apply(ctx: &egui::Context, a11y: &A11y) {
    ctx.set_fonts(fonts::definitions(a11y.dyslexic_font));

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = visuals(a11y);
    text_styles(&mut style);

    style.spacing.item_spacing = egui::vec2(space::CONTROL_GAP, space::CONTROL_GAP);
    style.spacing.button_padding = egui::vec2(space::PANEL_PAD, space::TOOLBAR_PAD);
    style.spacing.menu_margin = egui::Margin::same(space::TOOLBAR_PAD as i8);

    // egui floats scrollbars over the content by default, which covers
    // right-aligned stat values. Allocate real space instead: the numbers
    // stay readable, and a bar that occupies width is also a usable touch
    // target on iPad, which a 2px floating overlay is not.
    style.spacing.scroll.floating = false;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.bar_inner_margin = space::CONTROL_GAP;
    style.spacing.scroll.bar_outer_margin = 0.0;

    // Focus rings are drawn by widgets themselves (2px ember, 2px offset), so
    // suppress egui's own focus stroke rather than letting it double up.
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, color::BORDER_DEFAULT);

    // Dark only: there is no light theme. Pin the theme and give both slots
    // the same style so a system theme change can never leak a light surface.
    let style: std::sync::Arc<Style> = style.into();
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(egui::Theme::Dark);

    // UI scale multiplies the display's native scale factor; it does not
    // replace it. Setting it absolutely would render half-size on retina.
    ctx.set_zoom_factor(a11y.ui_scale);
}
