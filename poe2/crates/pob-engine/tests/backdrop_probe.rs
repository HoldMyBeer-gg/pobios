//! The pictures under the tree: the class portrait and its frame at the
//! centre, and the ascendancy plates round the rim.
//!
//! PoB2 places these from the tree data and draws them in a fixed order. The
//! fixture is a Mercenary who has ascended to Gemling Legionnaire.

use pob_engine::Engine;
use pob_engine::tree::NodeKind;
use std::path::PathBuf;

const CODE: &str = include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt");

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn the_centre_is_portrait_then_ring_then_frame() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    engine.load_build_xml(&pob_import::decode(CODE.trim()).unwrap()).expect("load");
    let tree = engine.tree().expect("tree");

    for b in &tree.backdrops {
        println!(
            "{:32} at ({:9.1}, {:9.1}) half {:6.0} turn {:5.2} {}",
            b.key,
            b.x,
            b.y,
            b.half,
            b.turn,
            if b.dimmed { "dimmed" } else { "" }
        );
    }

    let keys: Vec<&str> = tree.backdrops.iter().take(3).map(|b| b.key.as_str()).collect();
    // Once ascended, the portrait at the centre is the ascendancy's.
    assert_eq!(keys, ["ClassesGemling Legionnaire", "BGTreeActive", "BGTree"]);
    let [portrait, ring, frame] = &tree.backdrops[..3] else { unreachable!() };
    assert_eq!((portrait.x, portrait.y, portrait.half), (0.0, 0.0, 1500.0));
    assert_eq!((ring.half, frame.half), (2000.0, 2000.0));
    assert_eq!((portrait.turn, frame.turn), (0.0, 0.0));

    // The ring's marker is drawn at the top of its picture, and is turned to
    // face the class's start node.
    let start = tree
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::ClassStart && n.allocated)
        .expect("an allocated class start");
    let (sin, cos) = ring.turn.sin_cos();
    // Straight up, turned: (0, -1) goes to (sin, -cos).
    let facing = (sin, -cos);
    let away = ((start.x - ring.x).powi(2) + (start.y - ring.y).powi(2)).sqrt();
    let toward = ((start.x - ring.x) / away, (start.y - ring.y) / away);
    assert!(
        (facing.0 - toward.0).abs() < 0.001 && (facing.1 - toward.1).abs() < 0.001,
        "ring faces {facing:?}, start is toward {toward:?}"
    );

    // Every plate is drawn, and only the build's own at full brightness.
    let plates = &tree.backdrops[3..];
    assert!(plates.len() > 10, "only {} ascendancy plates", plates.len());
    let lit: Vec<&str> = plates.iter().filter(|b| !b.dimmed).map(|b| b.key.as_str()).collect();
    assert_eq!(lit, ["ClassesGemling Legionnaire"]);

    // And all of it is art the atlas can find.
    let dir = pob2_src().join("TreeData").join(&tree.version);
    let lua = std::fs::read_to_string(dir.join("tree.lua")).unwrap();
    let atlas = tree_assets::Atlas::new(&dir, &lua);
    for b in &tree.backdrops {
        assert!(atlas.resolve(&b.key).is_some(), "{} is not in ddsCoords", b.key);
    }
}
