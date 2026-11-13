//! Cluster jewels on the tree.
//!
//! PoB generates a subgraph per socketed cluster jewel — nodes, connectors
//! and a group — and draws it exactly like the rest of the tree. None of it
//! is in the static tree data; it only exists on the live spec, so a reader
//! that walks `tree.nodes` never sees a cluster jewel at all.

use pob_engine::tree::{NodeKind, ORBIT_RADII};
use pob_engine::Engine;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

/// One large cluster with two medium clusters nested in it, plus more
/// around the tree. PoB gives subgraph nodes ids above 0x10000.
const GOAL_BUILD: &str = include_str!("fixtures/goal-build.xml");
const CLUSTER_ID_BASE: u64 = 0x10000;

#[test]
#[serial_test::serial]
fn socketed_cluster_jewels_appear_on_the_tree() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");
    let tree = engine.tree().expect("tree");

    let cluster: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.id >= CLUSTER_ID_BASE)
        .collect();
    assert!(
        cluster.len() >= 30,
        "expected the fixture's cluster nodes, got {}",
        cluster.len()
    );

    // Every kind a cluster can hold, positioned and with art, so it draws
    // like any other node rather than as a blank disc.
    for kind in [NodeKind::Normal, NodeKind::Notable, NodeKind::Mastery] {
        let n = cluster
            .iter()
            .find(|n| n.kind == kind)
            .unwrap_or_else(|| panic!("no cluster {kind:?}"));
        assert!(n.x.is_finite() && n.y.is_finite() && (n.x != 0.0 || n.y != 0.0));
        assert!(n.icon.is_some(), "cluster {kind:?} {} has no icon", n.name);
    }
    assert!(
        cluster.iter().any(|n| n.allocated),
        "the fixture allocates cluster notables"
    );

    // The connectors PoB built for the subgraph: arcs around the cluster's
    // own orbit, and one straight line back to the parent socket.
    let edges: Vec<_> = tree
        .connectors
        .iter()
        .filter(|c| c.from >= CLUSTER_ID_BASE || c.to >= CLUSTER_ID_BASE)
        .collect();
    assert!(edges.iter().any(|c| c.is_arc), "no arcs between cluster nodes");
    assert!(
        edges.iter().any(|c| !c.is_arc && c.path.len() == 2),
        "no line from a cluster back to its socket"
    );
    // An arc's points sit on the cluster's orbit, not on a chord across it.
    let arc = edges.iter().find(|c| c.is_arc).unwrap();
    let a = tree.node(arc.from).unwrap();
    let radius = ORBIT_RADII[a.orbit as usize];
    for (x, y) in &arc.path {
        let r = ((x - a.group_x).powi(2) + (y - a.group_y).powi(2)).sqrt();
        assert!((r - radius).abs() < 1.0, "arc point off the orbit: {r} vs {radius}");
    }
}

#[test]
#[serial_test::serial]
fn cluster_groups_use_the_expansion_ring_art() {
    // PoB draws a subgraph's group with GroupBackground*Alt, the tree's own
    // groups with PSGroupBackground*. Sharing one flag for both puts the
    // wrong ring under every cluster.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");
    let tree = engine.tree().expect("tree");

    let expansion: Vec<_> = tree.groups.iter().filter(|g| g.expansion).collect();
    assert!(!expansion.is_empty(), "no cluster groups were reported");
    assert!(
        expansion.iter().all(|g| g.background.is_some()),
        "a cluster group always has a ring size: its orbit is set when built"
    );
    // The large cluster's ring is the large size; its medium children are
    // medium. Both sizes must survive, or every cluster draws the same ring.
    assert!(expansion.iter().any(|g| g.background == Some(3)));
    assert!(expansion.iter().any(|g| g.background == Some(2)));
    assert!(
        tree.groups.iter().filter(|g| !g.expansion).count() > 500,
        "the tree's own groups must keep the plain art"
    );
}

#[test]
#[serial_test::serial]
fn masteries_draw_without_a_frame() {
    // PoB's nodeOverlay has a Mastery entry, and PassiveTreeView never reads
    // it: a mastery is its sprite and nothing else. Resolving that entry put
    // an ascendancy ring around all 300-odd masteries, and around the
    // "Nothingness" mastery at the centre of every cluster.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");
    let tree = engine.tree().expect("tree");

    let masteries: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Mastery)
        .collect();
    assert!(masteries.len() > 100);
    for m in masteries {
        assert!(m.frame.is_none(), "mastery {} ({}) has frame {:?}", m.name, m.id, m.frame);
    }
    // Other kinds still do, or the frontier of the build goes invisible.
    assert!(tree.nodes.iter().filter(|n| n.kind == NodeKind::Notable).all(|n| n.frame.is_some()));
}

#[test]
#[serial_test::serial]
fn unsocketing_a_cluster_jewel_removes_its_subgraph() {
    // The tree is re-read after a socket changes; this checks the engine
    // side of that, PoB tearing the subgraph down when the jewel goes.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");
    let before = engine
        .tree()
        .expect("tree")
        .nodes
        .iter()
        .filter(|n| n.id >= CLUSTER_ID_BASE)
        .count();

    // Find a socket holding a cluster jewel and empty it.
    let equipment = engine.equipment().expect("equipment");
    let slot = equipment
        .iter()
        .find(|s| {
            s.name.starts_with("Jewel ")
                && s.item.as_ref().is_some_and(|i| {
                    i.base_name.contains("Cluster Jewel") || i.name.contains("Cluster Jewel")
                })
        })
        .map(|s| s.name.clone())
        .expect("a socketed cluster jewel");
    engine.equip_item(&slot, 0).expect("unsocket");

    let after = engine
        .tree()
        .expect("tree")
        .nodes
        .iter()
        .filter(|n| n.id >= CLUSTER_ID_BASE)
        .count();
    assert!(after < before, "unsocketing left {after} cluster nodes (was {before})");
}
