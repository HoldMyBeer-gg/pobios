//! Loading the passive tree's art from PoB's own downloaded assets.
//!
//! PoB caches every image it needs under `TreeData/`, so nothing is fetched
//! here — we read what is already on disk beside the tree data.

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Art needed to draw the tree, loaded once and kept as GPU textures.
#[derive(Default)]
pub struct TreeArt {
    textures: HashMap<String, TextureHandle>,
    /// Names that failed to load, so a missing file is reported once rather
    /// than retried every frame.
    missing: HashMap<String, ()>,
    /// Engine sheet name -> path under `TreeData`. See `resolved_sheet`.
    sheet_paths: HashMap<String, String>,
    root: Option<PathBuf>,
    /// PoB's `Assets/` directory. Tree art lives under `TreeData`, but the
    /// tooltip header slices and other UI chrome live here instead, so a
    /// single root cannot reach both.
    assets_root: Option<PathBuf>,
    /// PoB2 node-icon resolvers, one per tree version. Built lazily from
    /// `TreeData/<version>/tree.lua`; each maps an icon key to a DDS sheet
    /// slice and decodes it.
    atlases: HashMap<String, tree_assets::Atlas>,
    /// Backdrops read since their sheets were last let go.
    backdrops_read: Vec<String>,
    /// The frame a backdrop was last decoded in, and whether one has been
    /// put off since. See `backdrop_texture`.
    backdrop_frame: Option<u64>,
    backdrops_waiting: bool,
    /// Line art by family, each as plain, in-between, lit. `None` for a
    /// family whose files could not be read.
    lines: HashMap<String, Option<[LineArt; 3]>>,
}

/// The art one kind of connector is drawn with.
///
/// PoB2's line art is a band of colour across the line that does not change
/// along it, so the band is all that is kept: one column of the picture. It
/// is kept at three sizes because a band of 29 rows squeezed into a few
/// pixels flickers between whichever rows happen to be sampled. PoB2 loads
/// the same files with mipmaps for the same reason.
#[derive(Clone)]
pub struct LineArt {
    levels: [TextureHandle; 3],
    /// Rows across the whole picture, glow included: the width PoB2 draws
    /// the line at, in tree units.
    pub rows: f32,
    /// Rows across the solid part of the line, without the glow.
    pub band: f32,
    /// The colour along the band's edge, for a line too thin on screen to
    /// show anything else.
    pub edge: egui::Color32,
}

impl LineArt {
    /// The band to draw a line `px` pixels wide with, or `None` when even
    /// the smallest would be squeezed.
    pub fn level(&self, px: f32) -> Option<&TextureHandle> {
        self.levels.iter().find(|tex| px >= tex.size()[1] as f32 * LineArt::SQUEEZE)
    }

    /// How far a band may be squeezed before the next size down is used.
    /// Each size is half the last, so this keeps every one within a factor
    /// of 1.4 of its own rows.
    const SQUEEZE: f32 = 0.7;
}

/// Frames, by node kind and allocation state.
///
/// `PSSkillFrameActive` is 57px against the unallocated frame's 40px: an
/// allocated node's frame is genuinely larger art, not the same image tinted,
/// so the two must be looked up separately.
/// Resolve a sprite sheet named by the engine to a path under `TreeData`.
///
/// The engine reports the sheet each node's icon was packed into, because
/// there is no single atlas: `skills-3.jpg` and `skills-disabled-3.jpg` hold
/// the allocated and unallocated icons in *different layouts*, and masteries
/// and tattoos live on their own images again. Using one sheet's coordinates
/// against another lands the icon on whatever else occupies that rectangle,
/// which shows up as icons sliced in half or two overlapping fragments.
///
/// Sheets are stored per tree version, so the version prefixes the name.
pub fn icon_sheet_path(version: &str, sheet: &str) -> String {
    // The engine reports e.g. "skills-3.jpg"; the loader tries extensions
    // itself, so hand it the stem.
    let stem = sheet.rsplit('/').next().unwrap_or(sheet);
    let stem = stem.split('.').next().unwrap_or(stem);
    format!("{version}/{stem}")
}

/// The tooltip header art for a node, as three slices: left cap, tiled
/// middle, right cap, with the widths PoB2's `Tooltip.lua` draws them at.
///
/// PoB2 gives each kind of passive its own header, a notable's ornate where
/// a small passive's is plain, and the header is what makes a tooltip read
/// as belonging to the game rather than to a form. Every slice is 71x88 in
/// the file and drawn 38 high; the caps are 32 wide, a notable's left cap
/// 38, and the middle tiles in 32s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipHeader {
    pub left: &'static str,
    pub middle: &'static str,
    pub right: &'static str,
    pub height: f32,
    pub side: f32,
    pub tile: f32,
    /// How far down PoB2 nudges the title inside the band.
    pub text_offset: f32,
}

impl TooltipHeader {
    fn of(stem: &'static str, side: f32) -> Self {
        let (left, middle, right) = match stem {
            "notable" => ("notablepassiveheaderleft", "notablepassiveheadermiddle", "notablepassiveheaderright"),
            "keystone" => ("keystonepassiveheaderleft", "keystonepassiveheadermiddle", "keystonepassiveheaderright"),
            "jewel" => ("jewelpassiveheaderleft", "jewelpassiveheadermiddle", "jewelpassiveheaderright"),
            "ascendancy" => ("ascendancypassiveheaderleft", "ascendancypassiveheadermiddle", "ascendancypassiveheaderright"),
            _ => ("normalpassiveheaderleft", "normalpassiveheadermiddle", "normalpassiveheaderright"),
        };
        Self { left, middle, right, height: 38.0, side, tile: 32.0, text_offset: 6.0 }
    }
}

/// The header for a node, as PoB2's `AddNodeTooltip` picks it: an
/// ascendancy's passives share one header, everything else goes by kind.
pub fn tooltip_header(kind: pob_engine::tree::NodeKind, in_ascendancy: bool) -> TooltipHeader {
    use pob_engine::tree::NodeKind as K;
    match (kind, in_ascendancy) {
        (K::Normal | K::Notable, true) => TooltipHeader::of("ascendancy", 32.0),
        (K::Notable, _) => TooltipHeader::of("notable", 38.0),
        (K::Keystone, _) => TooltipHeader::of("keystone", 32.0),
        (K::Socket, _) => TooltipHeader::of("jewel", 32.0),
        _ => TooltipHeader::of("normal", 32.0),
    }
}

/// Group background art, by the ring size PoB assigned.
///
/// Cluster jewel groups use the expansion art. The small size has no
/// expansion image on disk, and PoB draws nothing for it rather than falling
/// back to the tree's ring.
pub fn group_background_name(size: u8, expansion: bool) -> &'static str {
    match (size, expansion) {
        (1, false) => "PSGroupBackground1",
        (2, false) => "PSGroupBackground2",
        (_, false) => "PSGroupBackground3",
        (1, true) => "GroupBackgroundSmallAlt",
        (2, true) => "GroupBackgroundMediumAlt",
        (_, true) => "GroupBackgroundLargeHalfAlt",
    }
}

impl TreeArt {
    /// Point at PoB's `TreeData` directory.
    pub fn new(pob_src: &Path) -> Self {
        Self {
            sheet_paths: HashMap::new(),
            root: Some(pob_src.join("TreeData")),
            assets_root: Some(pob_src.join("Assets")),
            ..Default::default()
        }
    }

    /// Fetch a texture, loading it from disk on first use.
    ///
    /// Loading lazily matters: `TreeData` holds 182 images and the tree needs
    /// a couple of dozen, so loading eagerly would cost startup time and GPU
    /// memory for art that is never drawn.
    pub fn get(&mut self, ctx: &Context, name: &str) -> Option<&TextureHandle> {
        if self.missing.contains_key(name) {
            return None;
        }
        if !self.textures.contains_key(name) {
            match self.load(ctx, name) {
                Some(tex) => {
                    self.textures.insert(name.to_owned(), tex);
                }
                None => {
                    self.missing.insert(name.to_owned(), ());
                    return None;
                }
            }
        }
        self.textures.get(name)
    }

    fn load(&self, ctx: &Context, name: &str) -> Option<TextureHandle> {
        // PoB stores most art as .png; a few sheets are .jpg or .webp.
        // Searched across both roots so callers name an asset without having
        // to know which directory it happens to live in.
        let roots = [self.root.as_ref(), self.assets_root.as_ref()];
        let path = roots
            .into_iter()
            .flatten()
            .flat_map(|root| {
                ["png", "jpg", "webp"]
                    .iter()
                    .map(move |ext| root.join(format!("{name}.{ext}")))
            })
            .find(|p| p.is_file())?;

        let bytes = std::fs::read(&path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?;
        let rgba = image.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

        Some(ctx.load_texture(name, pixels, TextureOptions::LINEAR))
    }

    /// A PoB2 node icon as a texture, resolved through `ddsCoords` and decoded
    /// from its DDS sheet slice on first use, then cached like any other.
    ///
    /// `key` is the node's icon path (`Icon::key`). Returns `None` for icons
    /// the atlas does not carry (e.g. mastery `isOnlyImage` nodes, which use
    /// the whole-image asset path) or if the sheet cannot be read.
    pub fn icon_texture(
        &mut self,
        ctx: &Context,
        version: &str,
        key: &str,
    ) -> Option<&TextureHandle> {
        // Namespace the cache so an icon key can never collide with a sheet
        // or asset name.
        let cache_key = format!("icon\0{version}\0{key}");
        if self.missing.contains_key(&cache_key) {
            return None;
        }
        if !self.textures.contains_key(&cache_key) {
            match self.decode_icon(ctx, version, key, &cache_key) {
                Some(tex) => {
                    self.textures.insert(cache_key.clone(), tex);
                }
                None => {
                    self.missing.insert(cache_key, ());
                    return None;
                }
            }
        }
        self.textures.get(&cache_key)
    }

    fn decode_icon(
        &mut self,
        ctx: &Context,
        version: &str,
        key: &str,
        cache_key: &str,
    ) -> Option<TextureHandle> {
        let root = self.root.as_ref()?;
        if !self.atlases.contains_key(version) {
            let dir = root.join(version);
            let tree_lua = std::fs::read_to_string(dir.join("tree.lua")).ok()?;
            self.atlases
                .insert(version.to_owned(), tree_assets::Atlas::new(&dir, &tree_lua));
        }
        let img = self.atlases.get_mut(version)?.icon_rgba(key)?;
        let size = [img.width as usize, img.height as usize];
        let pixels = ColorImage::from_rgba_unmultiplied(size, &img.pixels);
        Some(ctx.load_texture(cache_key, pixels, TextureOptions::LINEAR))
    }

    /// A family's line art, read from the files the tree names for it.
    pub fn line_art(&mut self, ctx: &Context, version: &str, family: &str, files: &[String; 3]) -> Option<&[LineArt; 3]> {
        if !self.lines.contains_key(family) {
            let art = self.load_lines(ctx, version, family, files);
            self.lines.insert(family.to_owned(), art);
        }
        self.lines.get(family)?.as_ref()
    }

    fn load_lines(&self, ctx: &Context, version: &str, family: &str, files: &[String; 3]) -> Option<[LineArt; 3]> {
        let dir = self.root.as_ref()?.join(version);
        let mut bands = Vec::new();
        for file in files {
            let image = image::open(dir.join(file)).ok()?.to_rgba8();
            let x = image.width() / 2;
            let band: Vec<[u8; 4]> = (0..image.height()).map(|y| image.get_pixel(x, y).0).collect();
            bands.push(band);
        }
        // The edge is the brightest solid row of the plain line, and the
        // same row of the others: all three are one line lit differently.
        let edge = (0..bands[0].len())
            .filter(|&y| bands[0][y][3] == 255)
            .max_by_key(|&y| bands[0][y][..3].iter().map(|&c| c as u32).sum::<u32>())?;

        let mut out = Vec::new();
        for (state, band) in bands.iter().enumerate() {
            let rows = band.len() as u32;
            // Four columns wide, so halving twice still leaves one.
            let whole = tree_assets::Rgba8 {
                width: 4,
                height: rows,
                pixels: band.iter().flat_map(|p| p.repeat(4)).collect(),
            };
            let levels = [1, 2, 4].map(|factor| {
                let img = whole.shrunk(factor);
                let size = [img.width as usize, img.height as usize];
                let pixels = ColorImage::from_rgba_unmultiplied(size, &img.pixels);
                ctx.load_texture(format!("line\0{version}\0{family}\0{state}\0{factor}"), pixels, TextureOptions::LINEAR)
            });
            let [r, g, b, _] = band[edge.min(band.len() - 1)];
            out.push(LineArt {
                levels,
                rows: rows as f32,
                band: band.iter().filter(|p| p[3] == 255).count() as f32,
                edge: egui::Color32::from_rgb(r, g, b),
            });
        }
        out.try_into().ok()
    }

    /// Art by the name PoB2 gives it, wherever PoB2 keeps it: a loose file
    /// beside the tree data or under `Assets`, or a slice of a DDS sheet.
    pub fn named(&mut self, ctx: &Context, version: &str, name: &str) -> Option<TextureHandle> {
        if let Some(tex) = self.get(ctx, name) {
            return Some(tex.clone());
        }
        self.icon_texture(ctx, version, name).cloned()
    }

    /// One of the large pictures under the tree, no more than `side` pixels
    /// along its longest edge.
    ///
    /// These are 1500 and 4008 pixels square, and PoB2 draws two dozen of
    /// them. It keeps them block-compressed on the GPU at a byte a pixel;
    /// Apple GPUs cannot, so here they cost four. Kept whole they come to
    /// some 330 MB, most of it on plates that are drawn dimmed at the rim.
    ///
    /// Only one is decoded per frame, the largest taking a quarter of a
    /// second. `None` means not yet as well as not at all: a repaint has been
    /// asked for and the picture will be there within a few frames.
    pub fn backdrop_texture(
        &mut self,
        ctx: &Context,
        version: &str,
        key: &str,
        side: u32,
    ) -> Option<&TextureHandle> {
        let cache_key = format!("backdrop\0{version}\0{side}\0{key}");
        if self.missing.contains_key(&cache_key) {
            return None;
        }
        if !self.textures.contains_key(&cache_key) {
            let frame = ctx.cumulative_frame_nr();
            if self.backdrop_frame == Some(frame) {
                self.backdrops_waiting = true;
                ctx.request_repaint();
                return None;
            }
            self.backdrop_frame = Some(frame);
            match self.decode_backdrop(ctx, version, key, side, &cache_key) {
                Some(tex) => {
                    self.textures.insert(cache_key.clone(), tex);
                    self.backdrops_read.push(key.to_owned());
                }
                None => {
                    self.missing.insert(cache_key, ());
                    return None;
                }
            }
        }
        self.textures.get(&cache_key)
    }

    fn decode_backdrop(
        &mut self,
        ctx: &Context,
        version: &str,
        key: &str,
        side: u32,
        cache_key: &str,
    ) -> Option<TextureHandle> {
        let root = self.root.as_ref()?;
        if !self.atlases.contains_key(version) {
            let dir = root.join(version);
            let tree_lua = std::fs::read_to_string(dir.join("tree.lua")).ok()?;
            self.atlases
                .insert(version.to_owned(), tree_assets::Atlas::new(&dir, &tree_lua));
        }
        let img = self.atlases.get_mut(version)?.icon_rgba(key)?;
        let img = img.shrunk(img.width.max(img.height).div_ceil(side.max(1)));
        let size = [img.width as usize, img.height as usize];
        let pixels = ColorImage::from_rgba_unmultiplied(size, &img.pixels);
        Some(ctx.load_texture(cache_key, pixels, TextureOptions::LINEAR))
    }

    /// Say that every backdrop has been asked for this frame.
    ///
    /// Once none was put off, the sheets they came from are let go: 110 MB of
    /// blocks that nothing will read again.
    pub fn backdrops_asked(&mut self, version: &str) {
        if std::mem::take(&mut self.backdrops_waiting) {
            return;
        }
        if let Some(atlas) = self.atlases.get_mut(version) {
            for key in self.backdrops_read.drain(..) {
                atlas.release(&key);
            }
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.textures.len()
    }

    /// Pixel dimensions of a loaded texture, for mapping atlas rects to UVs.
    /// Sheet name from the engine mapped to its path under `TreeData`.
    ///
    /// Cached because the mapping is pure and the caller is a per-node,
    /// per-frame loop: recomputing it allocates a String thousands of times a
    /// second to arrive at the same handful of answers.
    pub fn resolved_sheet(&mut self, version: &str, sheet: &str) -> String {
        if let Some(path) = self.sheet_paths.get(sheet) {
            return path.clone();
        }
        let path = icon_sheet_path(version, sheet);
        self.sheet_paths.insert(sheet.to_owned(), path.clone());
        path
    }

    pub fn dimensions(&self, name: &str) -> Option<egui::Vec2> {
        self.textures.get(name).map(|t| t.size_vec2())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_engine::tree::NodeKind as K;

    #[test]
    fn the_backdrop_texture_is_dark_by_design_not_by_accident() {
        // Background2 is near-black: max channel 26 of 255, average 8/12/17.
        // Over the #05080c canvas it reads as a faint grain, which is what
        // PoB shows. This is worth pinning because it looks exactly like a
        // texture that failed to load, and the reflex on seeing "nothing
        // there" is to brighten or dim it — both wrong. If a future asset
        // update makes it bright, this fails and says so.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../PathOfBuilding-PoE2/src/TreeData/Background2.png");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let image = image::load_from_memory(&bytes).expect("decodes").to_rgba8();
        let brightest = image
            .pixels()
            .flat_map(|p| p.0[..3].iter().copied())
            .max()
            .unwrap_or(0);
        assert!(
            brightest < 64,
            "Background2 is meant to be a dark grain; brightest channel {brightest}"
        );
    }


    #[test]
    fn every_tooltip_header_slice_exists_on_disk() {
        // A missing header falls back to a plain title rather than crashing,
        // so a typo here would show up as "the headers silently stopped"
        // rather than as an error. These live under Assets/, not TreeData/.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../PathOfBuilding-PoE2/src/Assets");
        if !root.is_dir() {
            return;
        }
        for kind in [K::Normal, K::Notable, K::Keystone, K::Socket] {
            for asc in [false, true] {
                let h = tooltip_header(kind, asc);
                for name in [h.left, h.middle, h.right] {
                    assert!(root.join(format!("{name}.png")).is_file(), "{name}.png is missing from Assets");
                }
            }
        }
    }

    #[test]
    fn an_ascendancy_passive_shares_one_header_and_a_notable_has_a_wider_cap() {
        assert_eq!(tooltip_header(K::Normal, true), tooltip_header(K::Notable, true));
        assert_ne!(tooltip_header(K::Normal, false), tooltip_header(K::Notable, false));
        assert_eq!(tooltip_header(K::Notable, false).side, 38.0);
        assert_eq!(tooltip_header(K::Keystone, false).side, 32.0);
    }

    #[test]
    fn group_backgrounds_cover_every_ring_size() {
        for size in 1..=3u8 {
            assert!(group_background_name(size, false).starts_with("PSGroupBackground"));
            assert!(group_background_name(size, true).ends_with("Alt"));
        }
        // An unexpected size falls back to the largest rather than panicking.
        assert_eq!(group_background_name(9, false), "PSGroupBackground3");
        assert_eq!(group_background_name(9, true), "GroupBackgroundLargeHalfAlt");
    }
}
