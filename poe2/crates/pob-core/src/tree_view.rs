//! The passive tree canvas: pan, zoom, and drawing.
//!
//! Everything drawn here comes from PoB's own computed geometry (see
//! `pob_engine::tree`). This module's job is the camera and the paint, not
//! the layout.

use crate::tree_art::{TreeArt, group_background_name};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use pob_engine::tree::{ConnectorState, Node, NodeArt, NodeKind, Tree};
use crate::style::tokens::color;

/// Camera over the tree, in tree units.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct View {
    /// Tree-space point at the centre of the viewport.
    pub center: (f32, f32),
    /// Screen pixels per tree unit.
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            center: (0.0, 0.0),
            zoom: 0.02,
        }
    }
}

/// Zoom bounds. The lower bound fits the whole tree; the upper is about where
/// node art stops gaining detail.
pub const MIN_ZOOM: f32 = 0.008;
pub const MAX_ZOOM: f32 = 1.0;

impl View {
    /// Frame the whole tree in `viewport`, with a small margin.
    ///
    /// Fits to where the nodes actually are rather than to `tree.bounds`.
    /// PoB's reported extent covers space reserved for cluster jewels and
    /// other off-tree geometry, so fitting to it leaves the tree small and
    /// surrounded by emptiness.
    pub fn fit(tree: &Tree, viewport: Rect) -> Self {
        if tree.nodes.is_empty() {
            return Self::default();
        }

        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for n in &tree.nodes {
            min_x = min_x.min(n.x);
            max_x = max_x.max(n.x);
            min_y = min_y.min(n.y);
            max_y = max_y.max(n.y);
        }

        let (w, h) = (max_x - min_x, max_y - min_y);
        if w <= 0.0 || h <= 0.0 {
            return Self::default();
        }

        // 0.94 leaves a little breathing room at the edges.
        let zoom = (viewport.width() / w)
            .min(viewport.height() / h)
            .clamp(MIN_ZOOM, MAX_ZOOM)
            * 0.94;

        Self {
            center: ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0),
            zoom,
        }
    }

    /// Tree space to screen space.
    pub fn to_screen(&self, viewport: Rect, x: f32, y: f32) -> Pos2 {
        let c = viewport.center();
        pos2(
            c.x + (x - self.center.0) * self.zoom,
            c.y + (y - self.center.1) * self.zoom,
        )
    }

    /// Screen space back to tree space.
    pub fn to_tree(&self, viewport: Rect, p: Pos2) -> (f32, f32) {
        let c = viewport.center();
        (
            (p.x - c.x) / self.zoom + self.center.0,
            (p.y - c.y) / self.zoom + self.center.1,
        )
    }

    /// Zoom by `factor`, keeping `anchor` (screen space) over the same tree
    /// point. Without this the view slides away from the cursor as you zoom,
    /// which makes navigating a tree this size unpleasant.
    pub fn zoom_at(&mut self, viewport: Rect, anchor: Pos2, factor: f32) {
        let before = self.to_tree(viewport, anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_tree(viewport, anchor);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
    }

    pub fn pan_pixels(&mut self, delta: Vec2) {
        self.center.0 -= delta.x / self.zoom;
        self.center.1 -= delta.y / self.zoom;
    }

    /// True when this camera is usable.
    ///
    /// A restored camera is not trusted blindly: a zoom of zero or NaN would
    /// divide by zero in the coordinate transforms, and a centre far outside
    /// the tree would restore to empty space with no obvious way back.
    pub fn is_sane(&self, tree: &Tree) -> bool {
        if !self.zoom.is_finite() || self.zoom < MIN_ZOOM || self.zoom > MAX_ZOOM {
            return false;
        }
        if !self.center.0.is_finite() || !self.center.1.is_finite() {
            return false;
        }
        let (min_x, min_y, max_x, max_y) = tree.bounds;
        let margin = 5000.0;
        self.center.0 >= min_x - margin
            && self.center.0 <= max_x + margin
            && self.center.1 >= min_y - margin
            && self.center.1 <= max_y + margin
    }
}

/// A colour multiplied by another, as PoB2's `SetDrawColor` does to art.
fn tinted(colour: Color32, by: Color32) -> Color32 {
    let mul = |a: u8, b: u8| ((a as u16 * b as u16 + 127) / 255) as u8;
    Color32::from_rgb(mul(colour.r(), by.r()), mul(colour.g(), by.g()), mul(colour.b(), by.b()))
}

/// Lay a ribbon of texture along a path, `half` to either side of it.
///
/// The texture is a band across the line: its top row down one side of the
/// ribbon and its bottom row down the other. Corners are mitred, so a curve
/// swept as short segments has no gaps on the outside of its bends.
fn ribbon(mesh: &mut egui::Mesh, path: &[Pos2], half: f32, tint: Color32) {
    let across = |a: Pos2, b: Pos2| {
        let d = b - a;
        let len = d.length();
        if len > 0.0 { vec2(-d.y, d.x) / len } else { Vec2::ZERO }
    };
    let first = mesh.vertices.len() as u32;
    for i in 0..path.len() {
        let before = if i > 0 { across(path[i - 1], path[i]) } else { Vec2::ZERO };
        let after = if i + 1 < path.len() { across(path[i], path[i + 1]) } else { Vec2::ZERO };
        let both = before + after;
        let mitre = if both.length() > 0.0 { both.normalized() } else { before };
        // Wider at a bend, by how far the mitre leans from either side. A
        // hairpin would run off to infinity, so it is held to twice.
        let lean = mitre.dot(if i > 0 { before } else { after }).max(0.5);
        let out = mitre * (half / lean);
        for (side, v) in [(1.0, 0.0), (-1.0, 1.0)] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: path[i] + out * side,
                uv: pos2(0.5, v),
                color: tint,
            });
        }
        if i > 0 {
            let at = first + i as u32 * 2;
            mesh.indices.extend([at - 2, at - 1, at, at, at - 1, at + 1]);
        }
    }
}

/// Fill for a node, standing in for the sprite art until that lands.
fn node_fill(kind: NodeKind, allocated: bool) -> Color32 {
    if allocated {
        match kind {
            NodeKind::Keystone => Color32::from_rgb(0xff, 0xb0, 0x66),
            NodeKind::Notable => Color32::from_rgb(0xf0, 0x78, 0x20),
            NodeKind::Mastery => Color32::from_rgb(0xd8, 0x5a, 0x12),
            NodeKind::Socket => Color32::from_rgb(0x9f, 0xd8, 0xf0),
            _ => Color32::from_rgb(0xd8, 0x5a, 0x12),
        }
    } else {
        // Unallocated art sits at 46% alpha in the design.
        match kind {
            NodeKind::Keystone | NodeKind::Notable => {
                Color32::from_rgba_unmultiplied(0x8a, 0x9a, 0xaf, 117)
            }
            NodeKind::Socket => Color32::from_rgba_unmultiplied(0x5b, 0x93, 0xe6, 117),
            _ => Color32::from_rgba_unmultiplied(0x5a, 0x6a, 0x7e, 117),
        }
    }
}

/// Draw an image centred on a point, preserving its aspect ratio.
///
/// `size` is the width to draw at; the height follows from the texture's own
/// proportions. Forcing a square would distort art that is not — the largest
/// group ring, PSGroupBackground3, is 284x144, so squaring it stretches the
/// ring into an ellipse.
fn draw_centred(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    centre: Pos2,
    size: f32,
    tint: Color32,
) {
    let dims = texture.size_vec2();
    let aspect = if dims.x > 0.0 { dims.y / dims.x } else { 1.0 };
    let rect = Rect::from_center_size(centre, vec2(size, size * aspect));
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        tint,
    );
}

/// Draw the circle inscribed in a square texture, `size` across.
///
/// PoB2's node icons are opaque squares, and at PoB2's own sizes a square
/// icon's corners reach 0.97 of the ring's half-width where the ring only
/// covers out to about 0.8. PoB2 draws the square anyway; on a desktop at its
/// zoom that is a pixel of near-black. Here, zoomed in on a tablet, the corners
/// read as a square sitting under a round ring. The inscribed circle ends at
/// 0.68, inside the ring's band, so nothing the ring would show is lost.
fn draw_round(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    centre: Pos2,
    size: f32,
    tint: Color32,
) {
    const SEGMENTS: u32 = 40;
    let r = size / 2.0;
    let mut mesh = egui::Mesh::with_texture(texture.id());
    mesh.vertices.push(egui::epaint::Vertex { pos: centre, uv: pos2(0.5, 0.5), color: tint });
    for i in 0..=SEGMENTS {
        let (sin, cos) = (i as f32 / SEGMENTS as f32 * std::f32::consts::TAU).sin_cos();
        mesh.vertices.push(egui::epaint::Vertex {
            pos: pos2(centre.x + cos * r, centre.y + sin * r),
            uv: pos2(0.5 + cos * 0.5, 0.5 + sin * 0.5),
            color: tint,
        });
    }
    for i in 1..=SEGMENTS {
        mesh.indices.extend([0, i, i + 1]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw a square sprite turned `angle` radians about `centre`, as PoB's
/// `DrawImageRotated` does for the jewel radius rings.
fn draw_rotated(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    centre: Pos2,
    size: f32,
    angle: f32,
    tint: Color32,
) {
    let h = size / 2.0;
    let (sin, cos) = angle.sin_cos();
    let corner = |x: f32, y: f32| pos2(centre.x + x * cos - y * sin, centre.y + x * sin + y * cos);
    let mut mesh = egui::Mesh::with_texture(texture.id());
    for (pos, uv) in [
        (corner(-h, -h), pos2(0.0, 0.0)),
        (corner(h, -h), pos2(1.0, 0.0)),
        (corner(h, h), pos2(1.0, 1.0)),
        (corner(-h, h), pos2(0.0, 1.0)),
    ] {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw a node: its icon inside its frame, or a coloured disc when the art is
/// too small to be worth drawing.
/// Draw a sprite-sheet rect centred on a point.
///
/// Coordinates from the engine are pixels on their own sheet (headless
/// `ImageSize()` reports 1x1, so PoB's own normalisation is a no-op), which
/// is why they are divided by the real texture size here.
// Every argument is a distinct piece of paint state; a struct would be
// built and taken apart per call for no reader's benefit.
#[allow(clippy::too_many_arguments)]
fn draw_sprite(
    painter: &egui::Painter,
    ctx: &egui::Context,
    art: &mut TreeArt,
    icon: &pob_engine::tree::Icon,
    version: &str,
    centre: Pos2,
    size: f32,
    tint: Color32,
) -> bool {
    // PoB2 form: the icon is a whole DDS slice keyed by its icon path, not a
    // sub-rect of a sheet. Decode-on-demand and draw the full texture.
    if !icon.key.is_empty() {
        // Decode is synchronous, so this returns the texture on first use or a
        // genuine miss (an icon the atlas does not carry, e.g. mastery
        // whole-images). No repaint request: there is no later state to wait
        // for, and asking every frame for the misses would spin the view.
        let Some(tex) = art.icon_texture(ctx, version, &icon.key) else {
            return false;
        };
        painter.image(
            tex.id(),
            Rect::from_center_size(centre, vec2(size, size)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
        return true;
    }

    // PoB1 form: a rect inside a named sprite sheet. Resolved through a cache
    // keyed by the engine's sheet name.
    let sheet = art.resolved_sheet(version, &icon.sheet);
    let Some(dims) = art.dimensions(&sheet) else {
        // Not loaded yet: load it, and pick it up next frame. Ask for that
        // frame: the host pauses the view when nothing requests one, and a
        // sprite first seen on a frame nobody follows never appears.
        art.get(ctx, &sheet);
        ctx.request_repaint();
        return false;
    };
    if dims.x <= 0.0 || dims.y <= 0.0 {
        return false;
    }
    let Some(tex) = art.get(ctx, &sheet) else {
        return false;
    };
    let (x0, y0, x1, y1) = icon.px;
    let uv = Rect::from_min_max(
        pos2(x0 / dims.x, y0 / dims.y),
        pos2(x1 / dims.x, y1 / dims.y),
    );
    painter.image(
        tex.id(),
        Rect::from_center_size(centre, vec2(size, size)),
        uv,
        tint,
    );
    true
}

// Every argument is a distinct piece of paint state; a struct would be
// built and taken apart per call for no reader's benefit.
#[allow(clippy::too_many_arguments)]
fn draw_node(
    painter: &egui::Painter,
    ctx: &egui::Context,
    art: &mut TreeArt,
    node: &Node,
    p: Pos2,
    zoom: f32,
    start_art: Option<&str>,
    tree_version: &str,
    radius_tint: Option<Color32>,
) {
    // The class start node draws its own art in place of a frame: the
    // coloured paths radiating out of where the build begins. PoB treats it
    // as a node overlay, so it belongs here in node draw order rather than
    // behind the group rings, which would bury it.
    if node.kind == pob_engine::tree::NodeKind::ClassStart {
        let name = if node.allocated {
            start_art
        } else {
            Some("PSStartNodeBackgroundInactive")
        };
        if let Some(name) = name
            && let Some(tex) = art.get(ctx, name)
        {
            let width = tex.size_vec2().x * 1.33 * 2.0 * zoom;
            if width >= 4.0 {
                draw_centred(painter, tex, p, width, Color32::WHITE);
            }
        }
        return;
    }

    // The picture in the middle of a group is drawn in the pass under the
    // connectors, with the glows. See draw_node_effect.
    if node.kind == NodeKind::OnlyImage {
        return;
    }

    let sizes = NodeArt::of(node.kind, node.ascendancy.is_some());
    let size = sizes.ring * zoom;
    let r = (size * 0.5).max(1.0);

    // Below about three pixels a frame is indistinguishable from a dot, and
    // drawing thousands of textured quads at that size is wasted work.
    if size < 3.5 {
        painter.circle_filled(p, r, node_fill(node.kind, node.allocated));
        if node.allocated && r > 2.5 {
            painter.circle_stroke(
                p,
                r,
                Stroke::new((r * 0.18).max(0.8), Color32::from_rgb(0xff, 0xb0, 0x66)),
            );
        }
        return;
    }

    // Compare mode multiplies the node's normal art by a flat colour rather
    // than swapping in different art — PoB's `SetDrawColor` before the sprite
    // draw. Green is what switching to the compared spec would gain, red what
    // it would lose, blue a mastery or jewel that differs.
    // A node inside a tapped socket's radius takes that radius's colour
    // instead, PoB's `SetDrawColor(data.col)` on hover.
    let tint = match (radius_tint, node.compare) {
        (Some(c), _) => c,
        (None, Some(diff)) => {
            let [r, g, b] = diff.tint();
            Color32::from_rgb(r, g, b)
        }
        (None, None) => Color32::WHITE,
    };

    // The icon, then the frame around it. Icons sit inside their frame
    // rather than filling it — except masteries, which have no frame at all,
    // so insetting one leaves a thumbnail marooned in the group ring.
    if let Some(icon) = &node.icon {
        // Sized from the sprite's own width, exactly as PoB's DrawAsset does:
        // `width * 1.33`, drawn at double that. Not a fraction of the frame.
        //
        // The inset this replaces (72% of node size) was invented, and it made
        // every icon about half the size PoB draws — a normal node's icon is
        // 69 tree units against the 53-unit frame, so it is meant to overflow
        // the frame slightly rather than sit inside it. The frame art carries
        // transparent margin around its ring, which is the room it overflows
        // into.
        let (inner, icon_tint) = if icon.key.is_empty() {
            let (x0, _, x1, _) = icon.px;
            ((x1 - x0) * 1.33 * 2.0 * zoom, tint)
        } else {
            // PoB2: the size is PoB2's own, and an icon that is not allocated
            // is drawn at half grey (`SetDrawColor(halfGray)` before the base
            // in PassiveTreeView.lua), which is what makes the taken nodes
            // stand out from the rest.
            let grey = if node.allocated { tint } else { HALF_GREY };
            (sizes.icon * zoom, grey)
        };
        if icon.key.is_empty() {
            draw_sprite(painter, ctx, art, icon, tree_version, p, inner, icon_tint);
        } else if let Some(tex) = art.icon_texture(ctx, tree_version, &icon.key) {
            draw_round(painter, tex, p, inner, icon_tint);
        }
    }

    // The frame is whatever the engine resolved: usually a file, but the
    // per-bloodline ascendancy frames are packed into a sheet instead.
    if let Some(frame) = &node.frame {
        match &frame.atlas {
            Some(rect) => {
                draw_sprite(painter, ctx, art, rect, tree_version, p, size, tint);
            }
            None => {
                // PoB1 frames are loose files; PoB2 frames are DDS-atlas
                // slices under the same nodeOverlay name. Try the file, then
                // the atlas. The frame is tinted too: PoB never resets the
                // colour between the base art and the overlay, so a diffed
                // node is coloured ring and all.
                if let Some(tex) = art.get(ctx, &frame.name).cloned() {
                    draw_centred(painter, &tex, p, size, tint);
                } else if let Some(tex) = art.icon_texture(ctx, tree_version, &frame.name) {
                    // PoB2 forces the ring's width and height to the same
                    // target size, so it is drawn square even where the
                    // texture is not (the notable ring is 160x164).
                    painter.image(
                        tex.id(),
                        Rect::from_center_size(p, vec2(size, size)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        tint,
                    );
                }
            }
        }
    }

    // A socketed jewel's art over the socket frame. PoB2 draws it as the
    // socket's overlay, at the overlay's size, which for a socket is the
    // frame's own.
    if let Some(name) = node.socket_jewel.as_ref().and_then(|j| j.overlay.as_deref())
        && let Some(tex) = art.named(ctx, tree_version, name)
    {
        painter.image(
            tex.id(),
            Rect::from_center_size(p, vec2(size, size)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }

}

/// The effect glow that marks a chosen mastery (or a tattoo). Drawn in its own
/// pass *before* the connectors, not with the node, because it is far larger
/// than the icon — a mastery icon is 86px where its glow is 297px, three and a
/// half times the size — and drawn in node order (after connectors) its opaque
/// centre painted over the orange lines passing under the mastery, so a
/// selected mastery erased the connectors around it. Painting it under the
/// connectors keeps the lines visible while the glow still spreads behind
/// them. It must still come after the base node art in the pass that follows,
/// but the base icon sits within the node and does not cross a connector.
fn draw_node_effect(
    painter: &egui::Painter,
    ctx: &egui::Context,
    art: &mut TreeArt,
    node: &Node,
    p: Pos2,
    zoom: f32,
    tree_version: &str,
) {
    let sizes = NodeArt::of(node.kind, node.ascendancy.is_some());

    // PoB2 draws the picture in the middle of a group on the same layer as the
    // glows, at 15% strength, so it sits behind the lines as a watermark.
    if node.kind == NodeKind::OnlyImage {
        if let Some(icon) = &node.icon {
            let w = sizes.icon * zoom;
            if w >= 4.0 {
                draw_sprite(painter, ctx, art, icon, tree_version, p, w, FAINT);
            }
        }
        return;
    }

    if let Some(effect) = &node.effect {
        if effect.key.is_empty() {
            let (x0, _, x1, _) = effect.px;
            let w = (x1 - x0) * 1.33 * 2.0 * zoom;
            draw_sprite(painter, ctx, art, effect, tree_version, p, w, Color32::WHITE);
        } else {
            // PoB2: full strength under an allocated node, 15% otherwise.
            let tint = if node.allocated { Color32::WHITE } else { FAINT };
            let w = sizes.glow * zoom;
            if w >= 4.0 {
                draw_sprite(painter, ctx, art, effect, tree_version, p, w, tint);
            }
        }
    }
}

/// PoB2's `halfGray` (`^x808080`), the tint on an icon that is not allocated.
const HALF_GREY: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);

/// The longest side the class portrait and its frame are kept at; the dimmed
/// plates at the rim get half of it. A memory budget, not one of PoB2's
/// numbers: see `TreeArt::backdrop_texture`. It leaves the portraits whole
/// and halves the 4008-pixel frame.
const BACKDROP_SIDE: u32 = 2048;

/// PoB2's `SetDrawColor(1, 1, 1, 0.15)`, for art that is there but not lit.
/// Premultiplied, as egui wants a tint: 15% of 255 in every channel.
const FAINT: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, 38);

pub struct TreeCanvas<'a> {
    pub tree: &'a Tree,
    pub view: &'a mut View,
    pub art: &'a mut TreeArt,
    /// Nodes matching the current search. Empty means no search is active,
    /// which is not the same as a search that matched nothing — that case
    /// dims everything, and is the honest answer to "no results".
    pub matches: &'a std::collections::HashSet<u64>,
    pub searching: bool,
    /// Node power, normalised to 0..1. Empty when the ranking is not shown.
    pub power: &'a std::collections::HashMap<u64, f32>,
    /// Set when an open tooltip is taller than the room it has, so the wheel
    /// belongs to it rather than to the camera.
    pub tooltip_scrollable: bool,
    /// A node whose tooltip is pinned open, on touch.
    ///
    /// A finger that has lifted hovers nothing, so a tooltip driven by hover
    /// alone can never be read on a tablet: it exists only while you are
    /// covering it. Tapping a node pins it, tapping again or tapping
    /// elsewhere dismisses it.
    pub pinned: Option<u64>,
}

/// What the canvas produced this frame.
pub struct CanvasResponse {
    pub response: egui::Response,
    pub hovered: Option<u64>,
    /// Node the user clicked, to allocate or deallocate.
    pub clicked: Option<u64>,
    /// Set when the user asked to fit the tree to the window.
    pub fit_requested: bool,
    /// Wheel delta the canvas declined to use because an open tooltip has
    /// more content than fits.
    pub tooltip_scroll: f32,
}

impl TreeCanvas<'_> {
    pub fn show(self, ui: &mut Ui) -> CanvasResponse {
        let viewport = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(viewport, Sense::click_and_drag());
        let painter = ui.painter_at(viewport);

        // Background: the radial gradient from the design, approximated as a
        // flat fill plus a lighter core.
        painter.rect_filled(viewport, 0.0, Color32::from_rgb(0x05, 0x08, 0x0c));

        // Drag to pan.
        if response.dragged() {
            self.view.pan_pixels(response.drag_delta());
        }

        // Wheel/pinch zooms at the cursor — unless a tooltip is open and has
        // more content than fits, in which case the wheel scrolls that
        // instead. Zooming the tree out from under a tooltip you are trying
        // to read is the wrong response to the same gesture.
        let mut fit_requested = false;
        let mut tooltip_scroll = 0.0;
        // Not gated on `hovered()`: a finger that has lifted hovers nothing,
        // so on a touch screen the tooltip would never receive its scroll.
        // An open scrollable tooltip owns the wheel regardless.
        if self.tooltip_scrollable {
            // The wheel belongs to the tooltip, so hand the delta over rather
            // than dropping it. Routing it through the canvas is deliberate:
            // the tooltip is anchored to the pointer, so reaching its
            // scrollbar would mean moving the pointer, which moves the
            // tooltip and changes which node is hovered.
            tooltip_scroll = ui.input(|i| i.smooth_scroll_delta.y);
        }
        if response.hovered() && !self.tooltip_scrollable {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01
                && let Some(pointer) = response.hover_pos()
            {
                self.view.zoom_at(viewport, pointer, (scroll * 0.004).exp());
            }
        }

        // Pinch, which arrives as a zoom factor rather than as scroll. A
        // trackpad pinch produces this too, but on a touch screen it is the
        // only way to zoom at all — there is no wheel, and the tree is far
        // too large to navigate without it.
        //
        // Suppressed while a scrollable tooltip is open. A two-finger scroll
        // is never perfectly parallel, so it reports a little zoom too — and
        // that zoom was winning, which made a tooltip impossible to scroll
        // on a touch screen.
        let pinch = ui.input(|i| i.zoom_delta());
        if (pinch - 1.0).abs() > 0.001 && !self.tooltip_scrollable {
            // Anchored at the viewport centre when there is no pointer: a
            // finger that has lifted is nowhere, and zooming toward a stale
            // position throws the view somewhere unexpected.
            let anchor = response.hover_pos().unwrap_or_else(|| viewport.center());
            self.view.zoom_at(viewport, anchor, pinch);
        }

        // Keyboard: arrows pan, +/- zoom, F fits.
        if response.has_focus() {
            ui.input(|i| {
                let step = 60.0;
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.view.pan_pixels(vec2(step, 0.0));
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.view.pan_pixels(vec2(-step, 0.0));
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.view.pan_pixels(vec2(0.0, step));
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.view.pan_pixels(vec2(0.0, -step));
                }
                if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                    self.view.zoom_at(viewport, viewport.center(), 1.2);
                }
                if i.key_pressed(egui::Key::Minus) {
                    self.view.zoom_at(viewport, viewport.center(), 1.0 / 1.2);
                }
                if i.key_pressed(egui::Key::F) {
                    fit_requested = true;
                }
            });
        }

        // Cull to what is actually on screen: at full zoom-out this is 3,000
        // nodes and 3,100 edges, and drawing off-screen geometry every frame
        // is wasted work.
        let margin = 200.0;
        let visible = viewport.expand(margin);
        let on_screen = |p: Pos2| visible.contains(p);

        let ctx = ui.ctx().clone();

        // Backdrops sit behind the rings: a tiled parchment across the whole
        // viewport, then the class art near that class's starting area. PoB
        // hardcodes the class positions, noting the tree JSON does not carry
        // them.
        // PoB2's Background2 is a near-black grain that its own view draws
        // screen-fixed and all but invisible; our dark radial-gradient canvas
        // already stands in for it. Loading and tiling the DDS in tree space
        // instead put a visibly wrong texture on the tree, so leave it to the
        // loose-file path (absent in PoB2), i.e. draw nothing here.
        if let Some(tex) = self.art.get(&ctx, "Background2") {
            // Tiled in *tree* units so it scrolls and scales with the tree
            // rather than sitting still behind it like a window blind. PoB
            // sizes one tile at width * 1.33 * 2.5.
            let dims = tex.size_vec2() * self.view.zoom * 1.33 * 2.5;
            if dims.x >= 4.0 && dims.y >= 4.0 {
                let cols = (viewport.width() / dims.x).ceil() as i32 + 1;
                let rows = (viewport.height() / dims.y).ceil() as i32 + 1;
                // Anchor the grid to tree origin so tiles don't crawl when panning.
                let origin = self.view.to_screen(viewport, 0.0, 0.0);
                let start_x = viewport.left() - (viewport.left() - origin.x).rem_euclid(dims.x);
                let start_y = viewport.top() - (viewport.top() - origin.y).rem_euclid(dims.y);
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                for row in 0..rows {
                    for col in 0..cols {
                        let min = pos2(
                            start_x + col as f32 * dims.x,
                            start_y + row as f32 * dims.y,
                        );
                        painter.image(
                            tex.id(),
                            Rect::from_min_size(min, dims),
                            uv,
                            // Full strength. The texture is near-black by
                            // design (max channel 26 of 255) — a faint grain
                            // over the void, not scenery. Dimming it further
                            // just deletes it.
                            Color32::WHITE,
                        );
                    }
                }
            }
        }

        // The class portrait with its ring and frame, then the ascendancy
        // plates, in the engine's order, which is PoB2's.
        for backdrop in &self.tree.backdrops {
            // Asked for whether or not it is in view. They arrive one a
            // frame, and asking only for what is on screen would put that
            // wait, and a re-read of the sheet, in the middle of every pan
            // round the rim.
            let side = if backdrop.dimmed { BACKDROP_SIDE / 2 } else { BACKDROP_SIDE };
            let tex = self
                .art
                .backdrop_texture(&ctx, &self.tree.version, &backdrop.key, side)
                .cloned();
            let Some(tex) = tex else { continue };
            let centre = self.view.to_screen(viewport, backdrop.x, backdrop.y);
            let size = backdrop.half * 2.0 * self.view.zoom;
            // A turned square reaches furthest at its corners.
            let reach = size * std::f32::consts::FRAC_1_SQRT_2;
            if size < 8.0 || !viewport.expand(reach).contains(centre) {
                continue;
            }
            let tint = if backdrop.dimmed { HALF_GREY } else { Color32::WHITE };
            draw_rotated(&painter, &tex, centre, size, backdrop.turn, tint);
        }
        self.art.backdrops_asked(&self.tree.version);


        // Group rings sit behind everything else, giving the tree its texture.
        for group in &self.tree.groups {
            let Some(bg) = group.background else { continue };
            let centre = self.view.to_screen(viewport, group.x, group.y);
            if !visible.contains(centre) {
                continue;
            }
            // PoB draws ring art at its own pixel size times 1.33, doubled
            // (it centres the image by offsetting a half-width and drawing at
            // width * 2). These are those widths in tree units.
            let size = match bg {
                1 => 138.0 * 1.33 * 2.0,
                2 => 179.0 * 1.33 * 2.0,
                _ => 284.0 * 1.33 * 2.0,
            } * self.view.zoom;
            if size < 8.0 {
                continue;
            }
            let name = group_background_name(bg, group.expansion);
            // PoB1 group rings are loose files; PoB2 packs them into a DDS
            // sheet under the same name, so fall back to the atlas.
            let tex = self
                .art
                .get(&ctx, name)
                .cloned()
                .or_else(|| self.art.icon_texture(&ctx, &self.tree.version, name).cloned());
            if let Some(tex) = tex {
                let tex = &tex;
                // Full white, as PoB draws it: the only place PoB dims tree
                // art at all is an unselected ascendancy plate, at 50%.
                let tint = Color32::WHITE;
                // The largest ring art is only the *top half* of the circle —
                // its CDN name is "...GroupBackgroundLargeHalf", and it is
                // 284x144 rather than square. PoB draws it twice, the second
                // copy mirrored vertically, to close the ring. Drawing it once
                // leaves a semicircle with a hard flat edge.
                if bg >= 3 {
                    let dims = tex.size_vec2();
                    let aspect = if dims.x > 0.0 { dims.y / dims.x } else { 0.5 };
                    let h = size * aspect;
                    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    // Flipping v0/v1 mirrors the image vertically, matching
                    // PoB's own (0, 1, 1, 0) UV order for the lower half.
                    let flipped = Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0));

                    let top = Rect::from_min_size(
                        pos2(centre.x - size * 0.5, centre.y - h),
                        vec2(size, h),
                    );
                    let bottom = Rect::from_min_size(
                        pos2(centre.x - size * 0.5, centre.y),
                        vec2(size, h),
                    );
                    painter.image(tex.id(), top, full, tint);
                    painter.image(tex.id(), bottom, flipped, tint);
                } else {
                    draw_centred(&painter, tex, centre, size, tint);
                }
            }
        }

        // Connectors under nodes.
        // Cull in *tree* space before transforming anything. There are ~3,100
        // connectors totalling ~26,500 path points, and at any useful zoom
        // most are off-screen; transforming them all first — and allocating a
        // Vec per connector to find out — is most of a frame's work spent
        // discovering there was nothing to draw. That cost shows up as
        // panning that feels heavy.
        let (vt_x0, vt_y0) = self.view.to_tree(viewport, visible.min);
        let (vt_x1, vt_y1) = self.view.to_tree(viewport, visible.max);

        // Mastery/tattoo effect glows go down *before* the connectors: they are
        // much larger than the node and would otherwise paint over the orange
        // lines running under an allocated mastery. See draw_node_effect.
        for node in &self.tree.nodes {
            if node.effect.is_none() && node.kind != NodeKind::OnlyImage {
                continue;
            }
            let p = self.view.to_screen(viewport, node.x, node.y);
            if on_screen(p) {
                draw_node_effect(&painter, &ctx, self.art, node, p, self.view.zoom, &self.tree.version);
            }
        }

        // Each connector is a ribbon of its art, or below the size the art
        // can be shown at, a line in the colour of the art's edge. Ribbons
        // are gathered by texture and drawn plain first, lit last, so the
        // build's path lies over whatever it crosses.
        let scale = ctx.pixels_per_point();
        let mut ribbons: Vec<(usize, egui::Mesh)> = Vec::new();
        let mut lines: Vec<(usize, egui::Shape)> = Vec::new();
        let mut points: Vec<Pos2> = Vec::with_capacity(64);
        for c in &self.tree.connectors {
            let (x0, y0, x1, y1) = c.bounds;
            if x1 < vt_x0 || x0 > vt_x1 || y1 < vt_y0 || y0 > vt_y1 {
                continue;
            }
            if c.path.len() < 2 {
                continue;
            }

            // One reused buffer rather than an allocation per connector.
            points.clear();
            // Straight edges need no intermediate points.
            let path = if c.is_arc {
                &c.path[..]
            } else {
                &c.path[..2]
            };
            points.extend(
                path.iter()
                    .map(|(x, y)| self.view.to_screen(viewport, *x, *y)),
            );

            // A connector whose active-ness differs between specs is forced
            // to its lit art and tinted, so a path the compared spec would
            // gain is visible rather than simply absent.
            let (state, tint) = match (c.compare, c.tint) {
                (Some(diff), _) => {
                    let [r, g, b] = diff.tint();
                    (ConnectorState::Active, Color32::from_rgb(r, g, b))
                }
                (None, Some([r, g, b])) => (c.state, Color32::from_rgb(r, g, b)),
                (None, None) => (c.state, Color32::WHITE),
            };
            let Some(art) = self
                .tree
                .line_art
                .get(&c.art)
                .and_then(|files| self.art.line_art(&ctx, &self.tree.version, &c.art, files))
                .map(|states| &states[state.art_index()])
            else {
                continue;
            };
            let order = state.art_index();
            match art.level(art.rows * self.view.zoom * scale) {
                Some(tex) => {
                    let id = tex.id();
                    let at = match ribbons.iter().position(|(o, m)| *o == order && m.texture_id == id) {
                        Some(at) => at,
                        None => {
                            ribbons.push((order, egui::Mesh::with_texture(id)));
                            ribbons.len() - 1
                        }
                    };
                    ribbon(&mut ribbons[at].1, &points, art.rows * self.view.zoom / 2.0, tint);
                }
                None => {
                    let width = (art.band * self.view.zoom).max(0.7);
                    let stroke = Stroke::new(width, tinted(art.edge, tint));
                    lines.push((order, egui::Shape::line(points.clone(), stroke)));
                }
            }
        }
        lines.sort_by_key(|(order, _)| *order);
        ribbons.sort_by_key(|(order, _)| *order);
        painter.extend(lines.into_iter().map(|(_, line)| line));
        painter.extend(ribbons.into_iter().map(|(_, mesh)| egui::Shape::mesh(mesh)));

        // Nodes, with the hovered one picked up as we go.
        // `hover_pos` is None on the frame a click completes — egui reports
        // the pointer as interacting rather than hovering — so the hit test
        // would find no node exactly when a click needs one. Fall back to the
        // interaction position, which is set precisely then.
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let mut hovered = None;
        let mut hovered_dist = f32::MAX;
        // A pinned node reads as hovered even with no pointer anywhere, which
        // is what makes the tooltip survive the finger lifting.
        let pinned = self.pinned;

        // A tapped socket shows its radii, PoB's hover behaviour: the nodes
        // inside each take that radius's colour, and rings mark the radii
        // themselves. Discs for an ordinary jewel, annuli for Thread of
        // Hope's kind. Some sockets have no radius.
        let radius_socket = pinned
            .and_then(|id| self.tree.nodes.iter().find(|n| n.id == id))
            .filter(|n| n.kind == pob_engine::tree::NodeKind::Socket && !n.no_radius);
        let annular = radius_socket.and_then(|n| n.socket_jewel.as_ref()).is_some_and(|j| j.variable_radius);
        let shown_radii: Vec<&pob_engine::tree::JewelRadius> = self
            .tree
            .jewel_radii
            .iter()
            .filter(|r| if annular { r.inner != 0.0 } else { r.inner == 0.0 })
            .collect();
        let radius_tint_for = |node: &Node| -> Option<Color32> {
            use pob_engine::tree::NodeKind::*;
            let socket = radius_socket?;
            if node.id == socket.id || matches!(node.kind, ClassStart | AscendancyStart | Mastery) {
                return None;
            }
            let d2 = (node.x - socket.x).powi(2) + (node.y - socket.y).powi(2);
            shown_radii
                .iter()
                .find(|r| d2 <= r.outer * r.outer && r.inner * r.inner <= d2)
                .map(|r| Color32::from_rgb(r.colour[0], r.colour[1], r.colour[2]))
        };

        for node in &self.tree.nodes {
            // Decoration, drawn with the glows: nothing to shade, search or
            // hover, and its 760-unit size would swallow every tap nearby.
            if node.kind == NodeKind::OnlyImage {
                continue;
            }
            let p = self.view.to_screen(viewport, node.x, node.y);
            if !on_screen(p) {
                continue;
            }
            let ring = NodeArt::of(node.kind, node.ascendancy.is_some()).ring;
            let r = (ring * 0.5 * self.view.zoom).max(1.0);

            // Power shading goes under the node: a red disc whose strength
            // tracks the score, as PoB's heat map draws it. Red, not the
            // orange of allocated paths and notables, or the best nodes
            // vanish into the build. A ring keeps it visible zoomed out.
            if let Some(score) = self.power.get(&node.id)
                && *score > 0.02
            {
                let alpha = (score * 220.0) as u8;
                painter.circle_filled(
                    p,
                    r * 2.0,
                    Color32::from_rgba_unmultiplied(0xff, 0x28, 0x28, alpha),
                );
                painter.circle_stroke(
                    p,
                    r * 2.0,
                    egui::Stroke::new(
                        (1.5 * score).max(0.5),
                        Color32::from_rgba_unmultiplied(0xff, 0x60, 0x60, alpha.max(60)),
                    ),
                );
            }

            draw_node(
                &painter,
                &ctx,
                self.art,
                node,
                p,
                self.view.zoom,
                self.tree.start_art.as_deref(),
                &self.tree.version,
                radius_tint_for(node),
            );

            // A search dims what did not match rather than hiding it, so the
            // tree keeps its shape and you can still see where hits sit
            // relative to everything else.
            if self.searching {
                if self.matches.contains(&node.id) {
                    painter.circle_stroke(
                        p,
                        r * 1.5,
                        Stroke::new(
                            (r * 0.3).max(1.5),
                            Color32::from_rgb(0xff, 0xb0, 0x66),
                        ),
                    );
                } else {
                    painter.circle_filled(
                        p,
                        r * 1.2,
                        Color32::from_rgba_unmultiplied(0x05, 0x08, 0x0c, 170),
                    );
                }
            }

            if let Some(ptr) = pointer {
                let d = p.distance(ptr);
                // Generous hit radius at low zoom, where nodes are tiny.
                if d <= r.max(6.0) && d < hovered_dist {
                    hovered_dist = d;
                    hovered = Some(node.id);
                }
            }
        }

        // Jewel radius rings, above the nodes as PoB draws them (its layer
        // 25). Each sprite turns at PoB's rate, angle × GetTime() × 0.00003
        // with GetTime in milliseconds, so ±0.7 is one turn every five
        // minutes: slow enough that 15 frames a second reads as smooth, and
        // the canvas asks for exactly that while a ring is on screen.
        let t = ctx.input(|i| i.time) as f32 * 1000.0 * 0.00003;
        let mut animating = false;
        for node in &self.tree.nodes {
            for rings in &node.jewel_rings {
                let centre = self.view.to_screen(viewport, node.x, node.y);
                let outer = rings.outer * self.view.zoom;
                if !viewport.expand(outer).contains(centre) {
                    continue;
                }
                animating = true;
                let [r, g, b, a] = rings.tint;
                let tint = Color32::from_rgba_premultiplied(r, g, b, a);
                for sprite in &rings.sprites {
                    // The shaded rings are files under Assets; a conqueror's
                    // circles are slices of a sheet.
                    if let Some(tex) = self.art.named(&ctx, &self.tree.version, &sprite.art) {
                        let size = sprite.radius * self.view.zoom * 2.0;
                        draw_rotated(&painter, &tex, centre, size, sprite.speed * t, tint);
                    }
                }
            }
        }
        if animating {
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }

        // The tapped socket's radii, PoB's `ring.png` in each radius's colour
        // at its outer edge, and at the inner edge too for an annulus.
        if let Some(socket) = radius_socket
            && let Some(tex) = self.art.get(&ctx, "ring")
        {
            let centre = self.view.to_screen(viewport, socket.x, socket.y);
            for radius in &shown_radii {
                let colour = Color32::from_rgb(radius.colour[0], radius.colour[1], radius.colour[2]);
                draw_centred(&painter, tex, centre, radius.outer * self.view.zoom * 2.0, colour);
                if radius.inner > 0.0 {
                    draw_centred(&painter, tex, centre, radius.inner * self.view.zoom * 2.0, colour);
                }
            }
        }

        // Focus ring, since the canvas is keyboard-operable.
        if response.has_focus() {
            painter.rect_stroke(
                viewport.shrink(1.0),
                egui::CornerRadius::same(2),
                Stroke::new(2.0, color::EMBER_600),
                egui::StrokeKind::Inside,
            );
        }

        CanvasResponse {
            // A click counts only if the pointer barely moved. Panning the
            // tree routinely ends with the pointer over a node, and treating
            // that as a click would allocate passives every time you moved
            // the view.
            //
            // Tested on distance rather than `dragged()`: on a
            // click_and_drag widget egui still reports `dragged()` on the
            // frame a plain click completes, so pairing the two conditions
            // rejects every click.
            clicked: (response.clicked()
                && ui.input(|i| i.pointer.press_origin())
                    .zip(response.interact_pointer_pos())
                    .is_none_or(|(a, b)| a.distance(b) < 4.0))
            .then_some(hovered)
            .flatten(),
            response,
            // A pinned node reads as hovered even with no pointer anywhere,
            // which is what lets its tooltip survive the finger lifting.
            hovered: hovered.or(pinned),
            fit_requested,
            tooltip_scroll,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))
    }

    #[test]
    fn screen_and_tree_coordinates_round_trip() {
        let view = View {
            center: (1234.0, -567.0),
            zoom: 0.05,
        };
        let vp = viewport();
        let screen = view.to_screen(vp, 2000.0, -1000.0);
        let (x, y) = view.to_tree(vp, screen);
        assert!((x - 2000.0).abs() < 0.01, "x round trip: {x}");
        assert!((y + 1000.0).abs() < 0.01, "y round trip: {y}");
    }

    #[test]
    fn zooming_keeps_the_anchor_point_still() {
        // Zoom-at-cursor is what makes a tree this large navigable; without
        // it the view drifts away from whatever you were looking at.
        let mut view = View {
            center: (0.0, 0.0),
            zoom: 0.02,
        };
        let vp = viewport();
        let anchor = pos2(750.0, 200.0);
        let before = view.to_tree(vp, anchor);

        view.zoom_at(vp, anchor, 2.0);

        let after = view.to_tree(vp, anchor);
        assert!((before.0 - after.0).abs() < 0.01, "drifted in x");
        assert!((before.1 - after.1).abs() < 0.01, "drifted in y");
    }

    #[test]
    fn zoom_stays_within_its_limits() {
        let mut view = View::default();
        let vp = viewport();
        for _ in 0..100 {
            view.zoom_at(vp, vp.center(), 2.0);
        }
        assert!(view.zoom <= MAX_ZOOM);
        for _ in 0..200 {
            view.zoom_at(vp, vp.center(), 0.5);
        }
        assert!(view.zoom >= MIN_ZOOM);
    }

    #[test]
    fn a_restored_camera_is_checked_before_use() {
        // Persisted state can be stale or corrupt. A zero zoom divides by
        // zero in the transforms, and a centre far from the tree restores to
        // empty space with no obvious way back — so both are rejected in
        // favour of a fresh fit.
        let tree = Tree { bounds: (-10000.0, -10000.0, 10000.0, 10000.0), ..Tree::default() };

        let good = View { center: (0.0, 0.0), zoom: 0.05 };
        assert!(good.is_sane(&tree));

        let zero_zoom = View { center: (0.0, 0.0), zoom: 0.0 };
        assert!(!zero_zoom.is_sane(&tree));

        let nan = View { center: (f32::NAN, 0.0), zoom: 0.05 };
        assert!(!nan.is_sane(&tree));

        let lost = View { center: (500_000.0, 0.0), zoom: 0.05 };
        assert!(!lost.is_sane(&tree), "a camera far off the tree is not sane");
    }

    #[test]
    fn fitting_an_empty_tree_does_not_divide_by_zero() {
        let view = View::fit(&Tree::default(), viewport());
        assert!(view.zoom.is_finite() && view.zoom > 0.0);
    }

    #[test]
    fn panning_moves_the_view_against_the_drag() {
        // Dragging right should bring content on the left into view, i.e.
        // the camera centre moves left.
        let mut view = View {
            center: (0.0, 0.0),
            zoom: 0.1,
        };
        view.pan_pixels(vec2(100.0, 0.0));
        assert!(view.center.0 < 0.0, "centre: {:?}", view.center);
    }
}

#[cfg(test)]
mod start_art_tests {
    use super::*;

    #[test]
    fn the_class_start_node_is_inside_the_fitted_view() {
        // The start art sits ~3,180 units above tree centre, further out than
        // most nodes. If `fit` ever stops accounting for it — by using a
        // bounds rectangle that excludes it, say — the art silently vanishes
        // off the top edge and looks like a failed asset load rather than a
        // camera bug. This pins which of the two it would be.
        let mut tree = Tree::default();
        tree.nodes.push(pob_engine::tree::Node {
            id: 1,
            kind: pob_engine::tree::NodeKind::ClassStart,
            name: "Witch".into(),
            x: 0.0,
            y: -3179.89,
            allocated: true,
            reachable: false,
            compare: None,
            ascendancy: None,
            stats: vec![],
            group_x: 0.0,
            group_y: -3179.89,
            angle: 0.0,
            orbit: 0,
            group: 0,
            icon: None,
            frame: None,
            effect: None,
            mastery_choice: None,
            mastery_effect_count: 0,
            is_ascendant_mirror: false,
            jewel_rings: Vec::new(),
            socket_jewel: None,
            no_radius: false,
            granted_only: false,
        });
        // A second node opposite it, so the fit has a real extent to work with.
        let mut far = tree.nodes[0].clone();
        far.id = 2;
        far.y = 3000.0;
        tree.nodes.push(far);

        let viewport = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1200.0, 800.0));
        let view = View::fit(&tree, viewport);
        let p = view.to_screen(viewport, 0.0, -3179.89);
        assert!(
            viewport.expand(200.0).contains(p),
            "start node lands at {p:?}, outside the fitted viewport"
        );
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    /// An empty match set means two different things depending on whether a
    /// search is running, and the canvas has to tell them apart.
    #[test]
    fn no_search_and_a_search_with_no_hits_are_different_states() {
        // With no search active, nothing is dimmed — the tree looks normal.
        // With a search that matched nothing, *everything* is dimmed, which
        // is the honest rendering of "no results". Collapsing the two into
        // "is the set empty?" would silently show a normal tree for a failed
        // search, which reads as "your search matched everything".
        let matches: std::collections::HashSet<u64> = std::collections::HashSet::new();

        // The distinction is carried by `searching`, not by the set.
        let idle = TreeCanvasState {
            searching: false,
            matches: &matches,
        };
        let no_hits = TreeCanvasState {
            searching: true,
            matches: &matches,
        };
        assert!(!idle.dims(1), "an idle canvas must not dim anything");
        assert!(no_hits.dims(1), "a failed search must dim everything");
    }

    #[test]
    fn a_matching_node_is_highlighted_and_the_rest_are_dimmed() {
        let matches: std::collections::HashSet<u64> = [7u64].into_iter().collect();
        let state = TreeCanvasState {
            searching: true,
            matches: &matches,
        };
        assert!(state.highlights(7));
        assert!(!state.dims(7));
        assert!(state.dims(8));
        assert!(!state.highlights(8));
    }
}

/// The search half of the canvas state, split out so the rule that decides
/// highlight-versus-dim is testable without a live egui context.
#[cfg(test)]
struct TreeCanvasState<'a> {
    searching: bool,
    matches: &'a std::collections::HashSet<u64>,
}

#[cfg(test)]
impl TreeCanvasState<'_> {
    fn highlights(&self, id: u64) -> bool {
        self.searching && self.matches.contains(&id)
    }
    fn dims(&self, id: u64) -> bool {
        self.searching && !self.matches.contains(&id)
    }
}

#[cfg(test)]
mod ribbon_tests {
    use super::*;

    #[test]
    fn a_straight_ribbon_is_its_width_across() {
        let mut mesh = egui::Mesh::default();
        ribbon(&mut mesh, &[pos2(0.0, 0.0), pos2(10.0, 0.0)], 3.0, Color32::WHITE);
        let at: Vec<_> = mesh.vertices.iter().map(|v| (v.pos.x, v.pos.y, v.uv.y)).collect();
        // The band's top row down one side, its bottom row down the other.
        assert_eq!(at, [(0.0, 3.0, 0.0), (0.0, -3.0, 1.0), (10.0, 3.0, 0.0), (10.0, -3.0, 1.0)]);
        assert_eq!(mesh.indices, [0, 1, 2, 2, 1, 3]);
    }

    #[test]
    fn a_bend_is_mitred_not_gapped() {
        // A right angle. The corner's two points sit on the diagonal, far
        // enough out that both arms keep their full width up to it.
        let mut mesh = egui::Mesh::default();
        ribbon(&mut mesh, &[pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0)], 3.0, Color32::WHITE);
        let corner = mesh.vertices[2].pos;
        assert!((corner.x - 7.0).abs() < 0.001 && (corner.y - 3.0).abs() < 0.001, "{corner:?}");
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.indices.len(), 12);
    }

    #[test]
    fn a_second_ribbon_in_the_same_mesh_does_not_join_the_first() {
        let mut mesh = egui::Mesh::default();
        ribbon(&mut mesh, &[pos2(0.0, 0.0), pos2(10.0, 0.0)], 1.0, Color32::WHITE);
        ribbon(&mut mesh, &[pos2(50.0, 0.0), pos2(60.0, 0.0)], 1.0, Color32::WHITE);
        assert_eq!(mesh.indices, [0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 7]);
    }
}

#[cfg(test)]
mod sprite_size_tests {
    use pob_engine::tree::{NodeArt, NodeKind};

    #[test]
    fn sizes_are_pob2s_own_doubled() {
        // GetNodeTargetSize in PassiveTree.lua, icon then ring, which
        // DrawAsset draws at twice the number. Guessing these is how icons
        // came to poke out past their rings.
        for (kind, in_ascendancy, icon, ring) in [
            (NodeKind::Normal, false, 37.0, 54.0),
            (NodeKind::Notable, false, 54.0, 80.0),
            (NodeKind::Keystone, false, 82.0, 120.0),
            (NodeKind::Socket, false, 76.0, 76.0),
            (NodeKind::Normal, true, 37.0, 80.0),
            (NodeKind::Notable, true, 54.0, 100.0),
        ] {
            let art = NodeArt::of(kind, in_ascendancy);
            assert_eq!(art.icon, icon * 2.0, "{kind:?} icon");
            assert_eq!(art.ring, ring * 2.0, "{kind:?} ring");
        }
    }

    #[test]
    fn size_is_the_main_tree_ring() {
        // `size()` is what hit-testing and the far-zoom dot use, so it has to
        // agree with the ring that is drawn.
        for kind in [NodeKind::Normal, NodeKind::Notable, NodeKind::Keystone, NodeKind::Socket] {
            assert_eq!(kind.size(), NodeArt::of(kind, false).ring, "{kind:?}");
        }
    }

    #[test]
    fn a_glow_is_larger_than_its_ring() {
        // The glow is meant to spread past the node it sits under.
        let art = NodeArt::of(NodeKind::Notable, false);
        assert!(art.glow > art.ring * 2.0, "glow {} against ring {}", art.glow, art.ring);
    }
}
