//! Drive the real `TreeCanvas` through a headless egui frame and confirm it
//! uploads node-icon textures — i.e. the PoB2 icon draw path actually runs
//! end to end, from `engine.tree()` through `ddsCoords` decode to an egui
//! texture, without a GPU or the app shell.

use pob_core::tree_art::TreeArt;
use pob_core::tree_view::{TreeCanvas, View};
use pob_engine::Engine;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn tree_canvas_uploads_node_icons() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    let tree = engine.tree().expect("extract tree");
    let mut art = TreeArt::new(&pob2_src());

    // One headless egui pass driving the real canvas. Node icons decode
    // synchronously on first draw, so a single frame uploads them all.
    egui::__run_test_ui(|ui| {
        let viewport = ui.available_rect_before_wrap();
        let mut view = View::fit(&tree, viewport);
        let matches = HashSet::new();
        let power = HashMap::new();
        TreeCanvas {
            tree: &tree,
            view: &mut view,
            art: &mut art,
            matches: &matches,
            searching: false,
            power: &power,
            tooltip_scrollable: false,
            pinned: None,
        }
        .show(ui);
    });

    let uploaded = art.loaded_count();
    println!("textures uploaded while drawing the whole PoE2 tree: {uploaded}");
    assert!(
        uploaded > 100,
        "expected many node-icon textures to upload; got {uploaded}"
    );
}
