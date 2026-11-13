//! Jewel radius rings: an allocated socket holding a jewel with a radius
//! carries the ring art PoB would draw around it, sized from PoB's own
//! radius table for the tree version.
use pob_engine::timeless::DesiredNode;
use pob_engine::Engine;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

const WINTER_ORB: &str = include_str!("fixtures/winter-orb-85.xml");

#[test]
#[serial_test::serial]
fn a_socketed_timeless_jewel_brings_its_rings_and_an_empty_socket_none() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // The fixture allocates socket 61419 and leaves it empty.
    let before = engine.tree().expect("tree");
    let socket = before.nodes.iter().find(|n| n.id == 61419).expect("socket node");
    assert!(socket.allocated);
    assert!(socket.jewel_rings.is_empty(), "an empty socket has no rings");
    // The radius table is PoB's for this tree: five discs, then annuli.
    let discs: Vec<f32> = before.jewel_radii.iter().filter(|r| r.inner == 0.0).map(|r| r.outer).collect();
    assert_eq!(discs, [960.0, 1440.0, 1800.0, 2400.0, 2880.0]);
    assert_eq!(before.jewel_radii[0].colour, [0xbb, 0x66, 0x00]);
    assert!(before.jewel_radii.iter().any(|r| r.label == "Variable" && r.inner > 0.0));

    // Put a Lethal Pride in it, via PoB's own finder and slot code.
    let mut state = engine.timeless_state().expect("state");
    state.jewel_type = 2;
    state.socket = 61419;
    state.desired = vec![DesiredNode::parse("karui_notable_add_double_damage, 1, 0, 0").unwrap()];
    engine.set_timeless_state(&state).expect("set");
    engine.timeless_search(&[]).expect("search");
    assert!(engine.timeless_add_to_build(0).expect("add").is_some());
    let jewel = engine
        .item_list()
        .expect("items")
        .into_iter()
        .find(|i| i.name.contains("Lethal Pride"))
        .expect("the added jewel");
    engine.equip_item("Jewel 61419", jewel.id).expect("socket it");

    let after = engine.tree().expect("tree");
    let socket = after.nodes.iter().find(|n| n.id == 61419).expect("socket node");
    assert_eq!(socket.jewel_rings.len(), 1);
    let rings = &socket.jewel_rings[0];
    // Neutral: white at 0.7, premultiplied.
    assert_eq!(rings.tint, [179, 179, 179, 179]);
    let art: Vec<&str> = rings.sprites.iter().map(|s| s.art.as_str()).collect();
    assert_eq!(art, ["PassiveSkillScreenKaruiJewelCircle1", "PassiveSkillScreenKaruiJewelCircle2"]);
    // Large radius on a 3.16+ tree, the pair turning opposite ways.
    assert_eq!(rings.outer, 1800.0);
    assert!(rings.sprites.iter().all(|s| s.radius == 1800.0));
    assert_eq!(rings.sprites.iter().map(|s| s.speed).collect::<Vec<_>>(), [-0.7, 0.7]);
    // Every ring asset PoB names is on disk, in TreeData or Assets.
    for name in &art {
        let p = pob_src().join("TreeData").join(format!("{name}.png"));
        assert!(p.is_file(), "{} missing", p.display());
    }
    for name in ["ShadedOuterRing", "ShadedOuterRingFlipped", "ShadedInnerRing", "ShadedInnerRingFlipped"] {
        assert!(pob_src().join("Assets").join(format!("{name}.png")).is_file(), "{name} missing");
    }
    // No other node grew rings.
    assert_eq!(after.nodes.iter().filter(|n| !n.jewel_rings.is_empty()).count(), 1);

    // The socket now shows its jewel: the Legion overlay, and the item's
    // name and lines in place of "Jewel Socket".
    let j = socket.socket_jewel.as_ref().expect("socketed jewel");
    assert_eq!(j.overlay.as_deref(), Some("JewelSocketActiveLegion"));
    assert!(j.name.starts_with("Lethal Pride"), "{}", j.name);
    assert!(j.name.chars().any(|c| c.is_ascii_digit()), "the seed is in the name: {}", j.name);
    assert_eq!(j.rarity, "UNIQUE");
    assert_eq!(j.name_colour(), [0xaf, 0x60, 0x25]);
    assert!(!j.variable_radius);
    assert!(j.active, "socket 61419 is allocated in the fixture");
    // Lethal Pride adds a line to every notable in radius rather than
    // renaming it, and the socket's tooltip lists each one.
    assert!(!j.changes.is_empty(), "a timeless jewel changes the passives around it");
    // Keystones are replaced (Strength of Blood); notables and small
    // passives gain a line, never a blank one.
    assert!(j.changes.iter().any(|c| c.replaced && c.to == "Strength of Blood"), "{:?}", j.changes);
    assert!(j.changes.iter().any(|c| !c.replaced && !c.small && c.to.contains('%')), "{:?}", j.changes);
    assert!(j.changes.iter().all(|c| !c.to.trim().is_empty() && !c.to.starts_with(';')), "{:?}", j.changes);
    assert!(j.changes.iter().any(|c| c.small && c.to.contains("Strength")));
    assert!(j.changes.iter().any(|c| c.allocated) && j.changes.iter().any(|c| !c.allocated));
    assert!(j.lines.iter().any(|l| l.contains("Conquered by the Karui")), "{:?}", j.lines);
    assert!(pob_src().join("TreeData/JewelSocketActiveLegion.png").is_file());
    assert!(before.nodes.iter().all(|n| n.socket_jewel.is_none()));

    // PoB rewrites the passives in radius in place (ReplaceNode and the
    // additions), so the tree already reports what the jewel turns them
    // into: some node's stats changed.
    let changed = after
        .nodes
        .iter()
        .zip(before.nodes.iter())
        .filter(|(a, b)| a.id == b.id && a.stats != b.stats)
        .count();
    assert!(changed > 0, "no passive in radius was rewritten by the jewel");
}

#[test]
#[serial_test::serial]
fn an_impossible_escape_rings_the_keystone_it_names_not_its_socket() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let id = engine
        .add_item_from_text(
            "Rarity: UNIQUE\nImpossible Escape\nViridian Jewel\nRadius: Small\nLimited to: 1\n\
             Implicits: 0\nPassive Skills in Radius of Mind Over Matter can be Allocated\n\
             without being connected to your tree",
        )
        .expect("add")
        .expect("an item id");
    engine.equip_item("Jewel 61419", id).expect("socket it");

    let tree = engine.tree().expect("tree");
    let socket = tree.nodes.iter().find(|n| n.id == 61419).unwrap();
    assert!(socket.jewel_rings.is_empty(), "the socket itself draws no ring");
    assert!(socket.socket_jewel.as_ref().is_some_and(|j| j.name.starts_with("Impossible Escape")));
    let keystone = tree
        .nodes
        .iter()
        .find(|n| n.name == "Mind Over Matter")
        .expect("the keystone");
    let rings = keystone.jewel_rings.first().expect("rings on the keystone");
    // PoB: the shaded outer pair at the jewel's radius (Small, 960), the
    // inner pair at a fixed 150, at its own four speeds.
    let got: Vec<(&str, f32, f32)> = rings.sprites.iter().map(|s| (s.art.as_str(), s.radius, s.speed)).collect();
    assert_eq!(
        got,
        [
            ("ShadedOuterRing", 960.0, -0.8),
            ("ShadedOuterRingFlipped", 960.0, 1.0),
            ("ShadedInnerRing", 150.0, -1.2),
            ("ShadedInnerRingFlipped", 150.0, 1.0),
        ]
    );
    assert_eq!(rings.outer, 960.0);
}
