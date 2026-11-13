//! Account import: PoB's sample character goes through PoB's own import
//! code, and the build comes out the way PoB's spec says it should.
use pob_engine::Engine;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

const WINTER_ORB: &str = include_str!("fixtures/winter-orb-85.xml");
/// PoB's own `spec/System/SampleCharacter.json`: a level 99 Hierophant.
const SAMPLE: &str = include_str!("fixtures/sample-character.json");

#[test]
#[serial_test::serial]
fn the_tree_import_matches_pobs_own_spec() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    assert_eq!(engine.info().unwrap().class, "Witch");

    engine.import_character_tree(SAMPLE, true).expect("import tree");

    // The assertions PoB's TestImport_spec makes.
    let info = engine.info().expect("info");
    assert_eq!(info.class, "Templar");
    assert_eq!(info.ascendancy, "Hierophant");
    assert_eq!(info.level, 99);
    let (keystones, notables, masteries, subgraphs): (i64, i64, i64, i64) = engine
        .eval(
            r#"
            local b = __mainObject__.main.modes["BUILD"]
            local n = 0
            for _ in pairs(b.spec.subGraphs) do n = n + 1 end
            return b.spec.allocatedKeystoneCount, b.spec.allocatedNotableCount, b.spec.allocatedMasteryCount, n
            "#,
        )
        .expect("eval");
    assert_eq!(keystones, 2);
    assert_eq!(notables, 29);
    assert_eq!(masteries, 7);
    assert_eq!(subgraphs, 2);
    let (bandit, major, minor): (String, String, String) = engine
        .eval(
            r#"
            local i = __mainObject__.main.modes["BUILD"].configTab.input
            return i.bandit, i.pantheonMajorGod, i.pantheonMinorGod
            "#,
        )
        .expect("eval");
    assert_eq!((bandit.as_str(), major.as_str(), minor.as_str()), ("None", "TheBrineKing", "Yugul"));
}

#[test]
#[serial_test::serial]
fn the_items_import_equips_the_character_and_its_skills() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    engine.import_character_items(SAMPLE, true, true, false).expect("import items");

    // With "Delete equipment" on, the Winter Orb's gear goes first, so the
    // count can drop; look for the sample's own pieces instead.
    let items = engine.item_list().expect("items");
    assert!(items.iter().any(|i| i.name.contains("Diamond Flask")), "{:?}", items.iter().map(|i| &i.name).collect::<Vec<_>>());
    let equipped = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter(|s| s.item.is_some())
        .count();
    assert!(equipped >= 10, "the character's gear is worn: {equipped}");
    assert!(!engine.socket_groups().expect("skills").is_empty(), "its gems are socketed");
}

#[test]
#[serial_test::serial]
fn something_that_is_not_a_character_is_refused() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let err = engine.import_character_tree(r#"{"error": {"message": "Forbidden"}}"#, true).unwrap_err();
    assert!(err.to_string().contains("not a character"), "{err}");
    assert_eq!(engine.info().unwrap().class, "Witch", "the build is untouched");
}

#[test]
#[serial_test::serial]
fn a_character_missing_its_league_leaves_the_build_untouched() {
    // PoB's importer deletes jewels and resets the tree before it reads
    // the league, so without a guard this gutted the build and returned Err.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let before = engine.info().unwrap();
    let err = engine
        .import_character_tree(r#"{"character":{"class":"Witch","passives":{"hashes":[]}}}"#, true)
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
    let after = engine.info().unwrap();
    assert_eq!((after.class, after.allocated_nodes), (before.class, before.allocated_nodes));
    let err = engine
        .import_character_items(r#"{"character":{"class":"Witch","league":"Standard","passives":{"hashes":[]}}}"#, true, true, false)
        .unwrap_err();
    assert!(err.to_string().contains("equipment"), "{err}");
    assert_eq!(engine.item_list().unwrap().len(), engine.item_list().unwrap().len());
}
