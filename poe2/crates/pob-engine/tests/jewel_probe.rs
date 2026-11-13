//! Jewels on the tree: the gem laid over a filled socket, and the rings
//! round a jewel that has a radius.
//!
//! The fixture's own jewels have no radius, so for the rings three of them
//! are swapped for jewels that do. Expected radii are PoB2's `jewelRadii`
//! table times its `PassiveTreeJewelDistanceMultiplier` of 1.2.

use pob_engine::Engine;
use pob_engine::tree::{NodeKind, Tree};
use std::path::PathBuf;

const CODE: &str = include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt");

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

fn atlas(tree: &Tree) -> tree_assets::Atlas {
    let dir = pob2_src().join("TreeData").join(&tree.version);
    let lua = std::fs::read_to_string(dir.join("tree.lua")).unwrap();
    tree_assets::Atlas::new(&dir, &lua)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// Swap the text of item `id` for `text`, leaving its place in the build.
fn with_item(xml: &str, id: u32, text: &str) -> String {
    let open = format!("<Item id=\"{id}\">");
    let start = xml.find(&open).expect("item") + open.len();
    let end = start + xml[start..].find("</Item>").expect("item end");
    format!("{}\n{}\n\t\t{}", &xml[..start], text.trim(), &xml[end..])
}

#[test]
#[serial_test::serial]
fn a_filled_socket_shows_its_gem() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    engine.load_build_xml(&pob_import::decode(CODE.trim()).unwrap()).expect("load");
    let tree = engine.tree().expect("tree");
    let atlas = atlas(&tree);

    let mut gems = Vec::new();
    for n in tree.nodes.iter().filter(|n| n.kind == NodeKind::Socket && n.allocated) {
        let Some(jewel) = &n.socket_jewel else { continue };
        let art = jewel.overlay.as_deref().unwrap_or_else(|| panic!("{} has no art", jewel.name));
        assert!(atlas.resolve(art).is_some(), "{art} is not in ddsCoords");
        println!("{:28} -> {art}", jewel.name);
        gems.push((jewel.name.clone(), art.to_owned()));
    }
    assert_eq!(gems.len(), 15);
    let art_of = |name: &str| gems.iter().find(|g| g.0 == name).map(|g| g.1.as_str());
    // A rare is drawn as its base.
    assert_eq!(art_of("Foe Ember, Ruby"), Some("Ruby"));
    assert_eq!(art_of("Blight Scar, Sapphire"), Some("Sapphire"));
    // A unique with art of its own is drawn as itself.
    assert_eq!(art_of("The Adorned, Diamond"), Some("The Adorned"));
    assert_eq!(art_of("Voices, Sapphire"), Some("Voices"));
    // None of these has a radius, so none has rings.
    assert!(tree.nodes.iter().all(|n| n.jewel_rings.is_empty()));
    // The sockets Voices grants show no radius when tapped.
    let sinister: Vec<_> = tree.nodes.iter().filter(|n| n.name == "Sinister Jewel Socket").collect();
    assert!(!sinister.is_empty());
    assert!(sinister.iter().all(|n| n.no_radius));
    assert!(tree.nodes.iter().filter(|n| n.name == "Jewel Socket").all(|n| !n.no_radius));
}

#[test]
#[serial_test::serial]
fn a_jewel_with_a_radius_is_ringed() {
    let xml = pob_import::decode(CODE.trim()).unwrap();
    // Socket 32763: a timeless jewel, which has circles of its conqueror's.
    let xml = with_item(
        &xml,
        11,
        "Rarity: UNIQUE\nHeroic Tragedy\nTimeless Jewel\nLimited to: 1 Historic\nRadius: Very Large\n\
         Implicits: 0\nRemembrancing 1000 songworthy deeds by the line of Vorana\n\
         Passives in radius are Conquered by the Kalguur\nHistoric",
    );
    // Socket 54127: a plain radius.
    let xml = with_item(
        &xml,
        13,
        "Rarity: RARE\nViper Sliver\nTime-Lost Ruby\nRadius: Medium\nImplicits: 0\n\
         Notable Passive Skills in Radius also grant 10% increased Fire Damage",
    );
    // Socket 26196: a ring, not a disc.
    let xml = with_item(
        &xml,
        14,
        "Rarity: UNIQUE\nControlled Metamorphosis\nDiamond\nLimited to: 1\nRadius: Variable\n\
         Implicits: 0\nOnly affects Passives in Medium Ring\n\
         Passives in Radius can be Allocated without being connected to your tree\n\
         -10% to all Elemental Resistances",
    );

    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    engine.load_build_xml(&xml).expect("load");
    let tree = engine.tree().expect("tree");
    let atlas = atlas(&tree);

    for n in tree.nodes.iter().filter(|n| !n.jewel_rings.is_empty()) {
        println!("{} {:?} {:?}", n.id, n.name, n.socket_jewel.as_ref().map(|j| &j.name));
        for set in &n.jewel_rings {
            for s in &set.sprites {
                println!("    {:>7.1} {:>5.1} {}", s.radius, s.speed, s.art);
            }
        }
    }
    let rings = |id: u64| {
        let node = tree.node(id).expect("socket");
        assert_eq!(node.jewel_rings.len(), 1, "socket {id}");
        node.jewel_rings[0].clone()
    };

    let timeless = rings(32763);
    let circles: Vec<_> = timeless.sprites.iter().map(|s| (s.art.as_str(), s.speed)).collect();
    let ingame = "art/textures/interface/2d/2dart/uiimages/ingame/";
    assert_eq!(
        circles,
        [
            (format!("{ingame}passiveskillscreenkalguuranjewelcircle1.dds").as_str(), -0.7),
            (format!("{ingame}passiveskillscreenkalguuranjewelcircle2.dds").as_str(), 0.7),
        ]
    );
    assert!(timeless.sprites.iter().all(|s| near(s.radius, 1500.0 * 1.2)));
    for s in &timeless.sprites {
        assert!(atlas.resolve(&s.art).is_some(), "{} is not in ddsCoords", s.art);
    }

    let plain = rings(54127);
    let shaded: Vec<_> = plain.sprites.iter().map(|s| (s.art.as_str(), s.speed)).collect();
    assert_eq!(shaded, [("ShadedOuterRing", -0.7), ("ShadedOuterRingFlipped", 0.7)]);
    assert!(plain.sprites.iter().all(|s| near(s.radius, 1150.0 * 1.2)));

    // Medium Ring is the fourth of the jewel's eight, 1100 to 1400 in the
    // table. The inner ring is drawn 6% wide of its radius.
    let ring = rings(26196);
    let annulus: Vec<_> = ring.sprites.iter().map(|s| s.art.as_str()).collect();
    assert_eq!(
        annulus,
        ["ShadedOuterRing", "ShadedOuterRingFlipped", "ShadedInnerRing", "ShadedInnerRingFlipped"]
    );
    assert!(near(ring.sprites[0].radius, 1400.0 * 1.2), "outer at {}", ring.sprites[0].radius);
    assert!(near(ring.sprites[2].radius, 1100.0 * 1.2 * 1.06), "inner at {}", ring.sprites[2].radius);
    assert!(tree.node(26196).unwrap().socket_jewel.as_ref().unwrap().variable_radius);

    // The shaded rings are files PoB2 ships, not slices of a sheet.
    for name in ["ShadedOuterRing", "ShadedOuterRingFlipped", "ShadedInnerRing", "ShadedInnerRingFlipped", "ring"] {
        assert!(pob2_src().join("Assets").join(format!("{name}.png")).is_file(), "{name}.png");
    }

    // What a tapped socket shows is measured the same way.
    assert!(near(tree.jewel_radii[0].outer, 1000.0 * 1.2));
    assert!(near(tree.jewel_radii[4].inner, 650.0 * 1.2));
}

/// The five Sinister sockets are granted by the Voices jewel, the first so
/// many of them, and cannot be bought with a point. The fixture's Voices
/// grants three.
#[test]
#[serial_test::serial]
fn a_sinister_socket_is_granted_not_bought() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    engine.load_build_xml(&pob_import::decode(CODE.trim()).unwrap()).expect("load");
    let tree = engine.tree().expect("tree");
    let sinister: Vec<_> = tree.nodes.iter().filter(|n| n.name == "Sinister Jewel Socket").collect();
    assert_eq!(sinister.iter().filter(|n| n.allocated).count(), 3);
    assert!(sinister.iter().all(|n| n.granted_only));
    assert!(tree.nodes.iter().filter(|n| n.name == "Jewel Socket").all(|n| !n.granted_only));

    // Spending a point on one that was not granted does nothing.
    let spare = sinister.iter().find(|n| !n.allocated).expect("one not granted").id;
    let before = engine.info().unwrap().points_used;
    let _ = engine.toggle_node(spare);
    let tree = engine.tree().expect("tree");
    println!("after toggling {spare}: allocated {}", tree.node(spare).unwrap().allocated);
    assert!(!tree.node(spare).unwrap().allocated);
    assert_eq!(engine.info().unwrap().points_used, before);
}
