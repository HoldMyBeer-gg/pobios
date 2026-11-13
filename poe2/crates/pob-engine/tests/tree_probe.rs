//! Does poe1's `engine.tree()` extraction survive PoB2's tree schema?
//!
//! `engine.tree()` was written against PoB1's sprite model (each node icon is
//! a pixel rect inside a named sheet). PoB2 keys art through `ddsCoords`
//! (sheet file -> sprite -> array slice index) instead. This probe reports
//! what actually comes out so we can see how much of the extraction holds
//! and where the DDS rewrite has to bite.

use pob_engine::Engine;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn probe_pob2_tree_extraction() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    let tree = match engine.tree() {
        Ok(t) => t,
        Err(e) => {
            println!("engine.tree() FAILED on PoB2: {e:?}");
            panic!("extraction failed outright");
        }
    };

    println!("version   : {}", tree.version);
    println!("nodes     : {}", tree.nodes.len());
    println!("groups    : {}", tree.groups.len());
    println!("wheels    : {}", tree.wheels.len());
    println!("bounds    : {:?}", tree.bounds);
    println!("class_art : {}", tree.class_art.is_some());

    // What sheets do node icons reference, and how many nodes have a
    // non-empty icon sheet vs an empty one (the PoB1 field absent in PoB2)?
    let mut with_sheet = 0;
    let mut empty = 0;
    let mut sheets: BTreeMap<String, usize> = BTreeMap::new();
    let mut sample = Vec::new();
    for n in &tree.nodes {
        match &n.icon {
            Some(icon) if !icon.key.is_empty() => {
                with_sheet += 1;
                // Bucket by the icon path's parent folder, to see the shape.
                let folder = icon.key.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string();
                *sheets.entry(folder).or_default() += 1;
                if sample.len() < 6 {
                    sample.push(format!("{:?} key={:?}", n.name, icon.key));
                }
            }
            _ => empty += 1,
        }
    }
    println!("\nnodes with icon key: {with_sheet}, without: {empty}");
    println!("distinct icon folders: {}", sheets.len());
    for (s, c) in sheets.iter().take(12) {
        println!("  {c:>5}  {s}");
    }
    println!("\nsample nodes:");
    for s in &sample {
        println!("  {s}");
    }

    // The other half of the pipeline: do those keys resolve through the DDS
    // atlas, and do they decode? This is the real end-to-end coverage.
    let ver_dir = pob2_src().join("TreeData").join(&tree.version);
    let tree_lua = std::fs::read_to_string(ver_dir.join("tree.lua")).expect("tree.lua");
    let mut atlas = tree_assets::Atlas::new(&ver_dir, &tree_lua);
    println!("\natlas knows {} icon paths", atlas.len());

    let mut resolved = 0;
    let mut unresolved = Vec::new();
    for n in &tree.nodes {
        if let Some(icon) = &n.icon {
            if !icon.key.is_empty() {
                if atlas.resolve(&icon.key).is_some() {
                    resolved += 1;
                } else if unresolved.len() < 6 {
                    unresolved.push(icon.key.clone());
                }
            }
        }
    }
    println!("node icon keys that resolve to a sheet+slice: {resolved}/{with_sheet}");

    // Lock in the milestone: geometry extracts, and the atlas covers the vast
    // majority of node icons. The shortfall is mastery isOnlyImage nodes,
    // which use the whole-image asset path (a separate follow-up).
    assert!(tree.nodes.len() > 4000, "tree geometry did not extract");
    assert!(with_sheet > 4000, "node icon keys did not extract");
    assert!(
        resolved * 100 / with_sheet >= 90,
        "icon atlas coverage regressed: {resolved}/{with_sheet}"
    );
    if !unresolved.is_empty() {
        println!("unresolved sample:");
        for u in &unresolved {
            println!("  {u}");
        }
    }

    // Decode a couple to prove the slice comes out as a real image, and dump
    // PNGs for eyeballing.
    let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../decoded-icons");
    let _ = std::fs::create_dir_all(&out);
    let mut dumped = 0;
    for n in &tree.nodes {
        if dumped >= 4 {
            break;
        }
        let Some(icon) = &n.icon else { continue };
        if icon.key.is_empty() {
            continue;
        }
        if let Some(img) = atlas.icon_rgba(&icon.key) {
            let name = format!("{}.png", n.name.replace(['/', ' '], "_"));
            let _ = image::save_buffer(
                out.join(&name),
                &img.pixels,
                img.width,
                img.height,
                image::ExtendedColorType::Rgba8,
            );
            println!("decoded {:?} -> {}x{} -> {name}", n.name, img.width, img.height);
            dumped += 1;
        }
    }
}
