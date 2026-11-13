//! Undo must not lose what was never changed.
use pob_engine::Engine;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

/// Eighteen jewels in sockets, saved from the iPad.
const GOAL_BUILD: &str = include_str!("fixtures/goal-build.xml");

fn socketed(engine: &Engine) -> usize {
    engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter(|s| s.name.starts_with("Jewel ") && s.item.is_some())
        .count()
}

/// Socket entries with an item, whichever order PoB's writer put the
/// attributes in: it walks a hash table, so that varies per process.
fn saved_sockets(engine: &Engine) -> usize {
    let xml = engine.build_xml().expect("xml");
    xml.split("<Socket ")
        .skip(1)
        .map(|rest| &rest[..rest.find("/>").unwrap_or(rest.len())])
        .filter(|tag| tag.contains("itemId=\"") && !tag.contains("itemId=\"0\""))
        .count()
}

/// PoB's ItemsTab records its first undo state before the tree has filled
/// the jewel sockets, so undoing any item change back to it emptied every
/// socket and saved the build that way. The engine re-baselines after load.
#[test]
#[serial_test::serial]
fn undoing_an_item_change_keeps_the_socketed_jewels() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");
    assert_eq!(socketed(&engine), 18);
    assert_eq!(saved_sockets(&engine), 18);

    // Add an unrelated item, undo it.
    engine
        .add_item_from_text("Rarity: RARE\nSpare Jewel\nCobalt Jewel\n5% increased Cast Speed")
        .expect("add")
        .expect("id");
    assert!(engine.undo_state("items").expect("state").0, "something to undo");
    engine.undo_redo("items", false).expect("undo");

    assert_eq!(socketed(&engine), 18, "undo emptied the jewel sockets");
    assert_eq!(saved_sockets(&engine), 18, "the save lost the sockets");
}
