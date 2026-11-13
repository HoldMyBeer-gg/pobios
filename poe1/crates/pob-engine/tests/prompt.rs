//! PoB reports its own errors through a prompt the engine reads back. One
//! error must not be reported again by every later call.
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
fn one_bad_load_does_not_poison_every_later_call() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // Malformed XML: PoB swallows the parse error into its prompt.
    let bad = engine.load_build_xml("<PathOfBuilding><Build></Buil></PathOfBuilding>");
    assert!(bad.is_err(), "a broken file must be reported");

    // The next real load and the next edit must both succeed; before the
    // fix every one of them returned the stale "trying to close <Build>".
    engine.load_build_xml(WINTER_ORB).expect("a good load after a bad one");
    let level = engine.info().unwrap().level;
    engine.set_level(level + 1).expect("an edit after a bad load");
    assert_eq!(engine.info().unwrap().level, level + 1);
}
