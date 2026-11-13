//! Probes of the decoded art, for inspecting by eye and by number.
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../PathOfBuilding-PoE2/src/TreeData/0_5")
}

/// Every name the node rings use must resolve. `PSSkillFrame` sits in the
/// last slice of its sheet, so reading ddsCoords as 0-based loses it.
#[test]
fn ring_names_resolve_and_are_distinct() {
    let d = dir();
    let lua = std::fs::read_to_string(d.join("tree.lua")).unwrap();
    let mut atlas = tree_assets::Atlas::new(&d, &lua);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../decoded-icons");
    std::fs::create_dir_all(&out).unwrap();
    let mut seen = Vec::new();
    for n in [
        "PSSkillFrame",
        "PSSkillFrameHighlighted",
        "PSSkillFrameActive",
        "NotableFrameUnallocated",
        "NotableFrameCanAllocate",
        "NotableFrameAllocated",
        "KeystoneFrameUnallocated",
        "KeystoneFrameAllocated",
        "JewelFrameUnallocated",
        "JewelFrameAllocated",
    ] {
        let img = atlas.icon_rgba(n).unwrap_or_else(|| panic!("{n} did not resolve"));
        image::save_buffer(
            out.join(format!("frame-{n}.png")),
            &img.pixels,
            img.width,
            img.height,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        assert!(!seen.contains(&img.pixels), "{n} decoded to the same image as another ring");
        seen.push(img.pixels);
    }
}

#[test]
fn shrinking_does_not_bleed_hidden_colour() {
    // One solid red pixel beside three clear ones that hold white, as a soft
    // edge does. The result is red at a quarter strength, not pink.
    let img = tree_assets::Rgba8 {
        width: 2,
        height: 2,
        pixels: vec![255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0],
    };
    let small = img.shrunk(2);
    assert_eq!((small.width, small.height), (1, 1));
    assert_eq!(small.pixels, [255, 0, 0, 64]);
    // Wholly clear stays clear and holds nothing.
    let clear = tree_assets::Rgba8 { width: 2, height: 2, pixels: [9, 9, 9, 0].repeat(4) };
    assert_eq!(clear.shrunk(2).pixels, [0, 0, 0, 0]);
}

#[test]
fn a_released_sheet_is_read_again_when_wanted() {
    let d = dir();
    let lua = std::fs::read_to_string(d.join("tree.lua")).unwrap();
    let mut atlas = tree_assets::Atlas::new(&d, &lua);
    let first = atlas.icon_rgba("PSSkillFrame").unwrap().pixels;
    assert_eq!(atlas.held(), 1);
    atlas.release("PSSkillFrame");
    assert_eq!(atlas.held(), 0);
    assert_eq!(atlas.icon_rgba("PSSkillFrame").unwrap().pixels, first);
}

/// The centre of the tree: what it costs to decode, and what it looks like.
#[test]
fn centre_art_decodes() {
    let d = dir();
    let lua = std::fs::read_to_string(d.join("tree.lua")).unwrap();
    let mut atlas = tree_assets::Atlas::new(&d, &lua);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../decoded-icons");
    std::fs::create_dir_all(&out).unwrap();
    for n in ["BGTree", "BGTreeActive", "ClassesMercenary", "ClassesGemling Legionnaire", "ClassesTitan"] {
        let t = std::time::Instant::now();
        let img = atlas.icon_rgba(n).unwrap_or_else(|| panic!("{n} did not resolve"));
        let took = t.elapsed();
        let opaque = img.pixels.chunks(4).filter(|p| p[3] > 8).count();
        println!(
            "{n}: {}x{} in {took:?}, {}% covered",
            img.width,
            img.height,
            opaque * 100 / (img.width * img.height) as usize
        );
        let full = image::RgbaImage::from_raw(img.width, img.height, img.pixels).unwrap();
        let small = image::imageops::resize(&full, 800, 800, image::imageops::FilterType::Triangle);
        small.save(out.join(format!("centre-{n}.png"))).unwrap();
    }
}

/// What an icon's corners hold, since a square icon under a round ring shows
/// its corners unless they are transparent.
#[test]
fn icon_corners() {
    let d = dir();
    let lua = std::fs::read_to_string(d.join("tree.lua")).unwrap();
    let mut atlas = tree_assets::Atlas::new(&d, &lua);
    for key in [
        "Art/2DArt/SkillIcons/passives/plusstrength.dds",
        "Art/2DArt/SkillIcons/passives/LightningDamagenode.dds",
    ] {
        let (file, slice) = atlas.resolve(key).map(|(f, s)| (f.to_string(), s)).unwrap();
        let img = atlas.icon_rgba(key).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2], img.pixels[i + 3]]
        };
        let m = img.width - 1;
        println!("{key}\n  sheet {file} slice {slice} {}x{}", img.width, img.height);
        println!("  corners {:?} {:?} {:?} {:?}", at(0, 0), at(m, 0), at(0, m), at(m, m));
        println!("  centre  {:?}", at(img.width / 2, img.height / 2));
        let opaque = img.pixels.chunks(4).filter(|p| p[3] > 0).count();
        println!("  opaque pixels {opaque} of {}", img.width * img.height);
    }
}
