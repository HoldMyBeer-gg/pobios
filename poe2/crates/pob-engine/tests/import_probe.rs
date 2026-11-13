//! Importing a real PoE2 character from the account API into a fresh build.
//!
//! Regression for the root-element bug: PoB2 requires a <PathOfBuilding2>
//! root, and the app's empty-build template used PoE1's <PathOfBuilding>, so
//! the from-home import loaded no build and then silently did nothing. The
//! fixture is a real level-4 Mercenary character's API JSON.

use pob_engine::Engine;
use std::path::PathBuf;

fn src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .unwrap()
}

// The empty build the app seeds a from-account import into: PoB2 root, latest
// tree version. A PoE1 <PathOfBuilding> root is rejected outright.
const EMPTY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding2>
	<Build level="1" targetVersion="0_1" bandit="None" className="Warrior" ascendClassName="None" mainSocketGroup="1" viewMode="TREE"/>
	<Skills activeSkillSet="1"><SkillSet id="1"/></Skills>
	<Items activeItemSet="1"><ItemSet id="1"/></Items>
	<Tree activeSpec="1"><Spec classId="6" ascendClassId="0" treeVersion="0_5" nodes=""/></Tree>
	<Config activeConfigSet="1"><ConfigSet id="1"/></Config>
	<Notes></Notes>
</PathOfBuilding2>"#;

#[test]
#[serial_test::serial]
fn imports_a_poe2_character() {
    let engine = Engine::new(&src()).unwrap();
    let json = include_str!("fixtures/poe2-character.json");

    engine.load_build_xml(EMPTY).expect("empty PoB2 build loads");
    engine.import_character_tree(json, false).expect("import tree");
    engine.import_character_items(json, true, true, false).expect("import items");

    let info = engine.info().unwrap();
    // The empty build was a Warrior; the import must switch it to the
    // character's class and pull in its allocated passives.
    assert_eq!(info.class, "Mercenary", "class not taken from the character");
    assert!(info.allocated_nodes >= 1, "no passives imported");
    println!("imported: {} nodes={} level={}", info.class, info.allocated_nodes, info.level);
}
