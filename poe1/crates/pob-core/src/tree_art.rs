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
/// middle, right cap.
///
/// PoB gives each node type its own header — a notable's is ornate where a
/// small passive's is plain — and the header is what makes a tooltip read as
/// belonging to the game rather than to a form. All three are 88px tall; the
/// caps are 79px wide and the middle tiles to fill whatever is left.
pub fn tooltip_header(kind: pob_engine::tree::NodeKind, allocated: bool) -> [&'static str; 3] {
    use pob_engine::tree::NodeKind as K;
    let stem = match kind {
        K::Notable => "notablepassiveheader",
        K::Keystone => "keystonepassiveheader",
        K::Socket => "jewelpassiveheader",
        K::Mastery => {
            if allocated {
                "masteryheaderallocated"
            } else {
                "masteryheaderunallocated"
            }
        }
        _ => "normalpassiveheader",
    };
    match stem {
        "notablepassiveheader" => [
            "notablepassiveheaderleft",
            "notablepassiveheadermiddle",
            "notablepassiveheaderright",
        ],
        "keystonepassiveheader" => [
            "keystonepassiveheaderleft",
            "keystonepassiveheadermiddle",
            "keystonepassiveheaderright",
        ],
        "jewelpassiveheader" => [
            "jewelpassiveheaderleft",
            "jewelpassiveheadermiddle",
            "jewelpassiveheaderright",
        ],
        "masteryheaderallocated" => [
            "masteryheaderallocatedleft",
            "masteryheaderallocatedmiddle",
            "masteryheaderallocatedright",
        ],
        "masteryheaderunallocated" => [
            "masteryheaderunallocatedleft",
            "masteryheaderunallocatedmiddle",
            "masteryheaderunallocatedright",
        ],
        _ => [
            "normalpassiveheaderleft",
            "normalpassiveheadermiddle",
            "normalpassiveheaderright",
        ],
    }
}

/// Ascendancy nodes get their own header regardless of kind.
pub fn ascendancy_tooltip_header() -> [&'static str; 3] {
    [
        "ascendancypassiveheaderleft",
        "ascendancypassiveheadermiddle",
        "ascendancypassiveheaderright",
    ]
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
    use pob_engine::tree::NodeKind;

    #[test]
    fn the_backdrop_texture_is_dark_by_design_not_by_accident() {
        // Background2 is near-black: max channel 26 of 255, average 8/12/17.
        // Over the #05080c canvas it reads as a faint grain, which is what
        // PoB shows. This is worth pinning because it looks exactly like a
        // texture that failed to load, and the reflex on seeing "nothing
        // there" is to brighten or dim it — both wrong. If a future asset
        // update makes it bright, this fails and says so.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../PathOfBuilding/src/TreeData/Background2.png");
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
        // A missing header falls back to a plain label rather than crashing,
        // which means a typo here would show up as "the fancy tooltips
        // silently stopped working" rather than as an error. These live under
        // Assets/, not TreeData/, which is its own easy thing to get wrong.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../PathOfBuilding/src/Assets");
        if !root.is_dir() {
            return; // No checkout; nothing to check against.
        }
        let mut sets = vec![ascendancy_tooltip_header()];
        for kind in [
            NodeKind::Normal,
            NodeKind::Notable,
            NodeKind::Keystone,
            NodeKind::Socket,
            NodeKind::Mastery,
        ] {
            for allocated in [true, false] {
                sets.push(tooltip_header(kind, allocated));
            }
        }
        for set in sets {
            for name in set {
                assert!(
                    root.join(format!("{name}.png")).is_file(),
                    "{name}.png is missing from Assets"
                );
            }
        }
    }

    #[test]
    fn masteries_get_a_different_header_once_chosen()  {
        // The allocated and unallocated mastery headers are different art,
        // which is how PoB shows at a glance that an effect has been picked.
        assert_ne!(
            tooltip_header(NodeKind::Mastery, true),
            tooltip_header(NodeKind::Mastery, false)
        );
        // Everything else ignores allocation for its header.
        assert_eq!(
            tooltip_header(NodeKind::Notable, true),
            tooltip_header(NodeKind::Notable, false)
        );
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
