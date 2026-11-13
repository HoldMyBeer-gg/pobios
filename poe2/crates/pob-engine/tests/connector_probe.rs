//! A connector has to reach the two nodes it joins.
//!
//! PoE2 curves a connection in two ways. Nodes sharing an orbit of one group
//! are joined round that group's centre, as in PoE1. A connection can also
//! name an orbit of its own, and then the curve has that orbit's radius about
//! a centre worked out from the two endpoints. Sweeping the second kind round
//! the group leaves it ending in mid-air.

use pob_engine::Engine;
use std::collections::HashMap;
use std::path::PathBuf;

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn every_connector_ends_on_its_nodes() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    let tree = engine.tree().expect("tree");
    let at: HashMap<u64, (f32, f32)> = tree.nodes.iter().map(|n| (n.id, (n.x, n.y))).collect();

    let gap = |p: (f32, f32), q: (f32, f32)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    let (mut arcs, mut adrift) = (0, Vec::new());
    for c in &tree.connectors {
        let (Some(&a), Some(&b)) = (at.get(&c.from), at.get(&c.to)) else { continue };
        let (first, last) = (c.path[0], c.path[c.path.len() - 1]);
        // Either way round: an arc is swept from whichever end comes first.
        let off = (gap(first, a) + gap(last, b)).min(gap(first, b) + gap(last, a));
        if c.is_arc {
            arcs += 1;
            // Drawn as a chord, an arc still ends on its nodes, so that has
            // to be caught separately.
            if c.path.len() == 2 {
                adrift.push((c.from, c.to, -1.0));
                continue;
            }
        }
        if off > 2.0 {
            adrift.push((c.from, c.to, off));
            continue;
        }
        // The ends are pinned to the nodes, so a curve about the wrong centre
        // would still arrive. It would arrive with a jump in its last step.
        let steps: Vec<f32> = c.path.windows(2).map(|w| gap(w[0], w[1])).collect();
        let (short, long) = steps.iter().fold((f32::MAX, 0.0f32), |(s, l), &d| (s.min(d), l.max(d)));
        if long - short > 1.0 {
            adrift.push((c.from, c.to, long - short));
        }
    }
    println!("connectors {} arcs {arcs} adrift {}", tree.connectors.len(), adrift.len());
    for (from, to, off) in adrift.iter().take(10) {
        println!("  {from} -> {to}: off by {off:.1}");
    }
    assert!(adrift.is_empty(), "{} connectors miss their nodes", adrift.len());
}

/// What a connector is drawn with. The fixture is a Gemling Legionnaire.
#[test]
#[serial_test::serial]
fn a_line_is_lit_only_between_two_taken_nodes() {
    use pob_engine::tree::ConnectorState;
    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    let code = include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt");
    engine.load_build_xml(&pob_import::decode(code.trim()).unwrap()).expect("load");
    let tree = engine.tree().expect("tree");
    let taken: HashMap<u64, bool> = tree.nodes.iter().map(|n| (n.id, n.allocated)).collect();
    let owner: HashMap<u64, Option<&str>> =
        tree.nodes.iter().map(|n| (n.id, n.ascendancy.as_deref())).collect();

    let (mut lit, mut sets) = (0, 0);
    for c in &tree.connectors {
        let both = taken[&c.from] && taken[&c.to];
        // Two taken nodes are not always joined: those taken for different
        // weapon sets are not. But nothing else is ever lit.
        if c.state == ConnectorState::Active {
            assert!(both, "{} -> {} is lit with an end not taken", c.from, c.to);
            lit += 1;
        }
        // With nothing hovered there is no in-between.
        assert_ne!(c.state, ConnectorState::Intermediate);

        // Every connector names art the tree has a file for.
        let files = tree.line_art.get(&c.art).unwrap_or_else(|| panic!("no art for {:?}", c.art));
        let file = pob2_src().join("TreeData").join(&tree.version).join(&files[c.state.art_index()]);
        assert!(file.is_file(), "{}", file.display());

        // Another ascendancy's lines are greyed and the build's own are
        // not. On the tree itself a lit line taken for a weapon set has
        // that set's colour, PoB2's red for the first and green for the
        // second.
        match (owner[&c.from], c.tint) {
            (Some("Gemling Legionnaire"), tint) => assert_eq!(tint, None),
            (Some(_), tint) => assert_eq!(tint, Some([0xbf, 0xbf, 0xbf])),
            (None, None) => {}
            (None, Some(set)) => {
                assert_eq!(c.state, ConnectorState::Active);
                assert!(set == [0xdd, 0x00, 0x22] || set == [0x33, 0xff, 0x77], "{set:?}");
                sets += 1;
            }
        }
    }
    println!("lit for a weapon set: {sets}");
    assert!(lit > 100, "only {lit} lit connectors");
    let mut families: Vec<_> = tree.line_art.keys().map(String::as_str).collect();
    families.sort();
    println!("families {families:?}, lit {lit}");
    assert!(families.contains(&"Character") && families.contains(&"CharacterAscendancy"));
}
