//! EHPUnluckyWorstOf is a numeric-valued list. Stored as a string it makes
//! PoB's CalcDefence compare "2" > 1 and throw, which corrupted the build.
//! The setter must store PoB's native type for the chosen list entry.
use pob_engine::Engine;
use pob_engine::config::Value;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../PathOfBuilding/src").canonicalize().unwrap()
}
const WINTER_ORB: &str = include_str!("fixtures/winter-orb-85.xml");

#[test]
#[serial_test::serial]
fn numeric_list_config_stays_numeric_and_calcs() {
    let e = Engine::new(&pob_src()).unwrap();
    e.load_build_xml(WINTER_ORB).unwrap();

    // Setting the list choice must store a number, not the string "2".
    e.set_config("EHPUnluckyWorstOf", Value::Text("2".into())).unwrap();
    let ty: String = e
        .eval(r#"return type(__mainObject__.main.modes["BUILD"].configTab.input["EHPUnluckyWorstOf"])"#)
        .unwrap();
    assert_eq!(ty, "number", "a numeric list value must be stored as a number");

    // The defence calc must run rather than throw, and an unrelated toggle
    // afterwards must not be poisoned.
    assert!(e.stat("TotalEHP").unwrap().is_some());
    e.set_config("conditionFullLife", Value::Bool(true)).unwrap();
    assert!(e.stat("TotalEHP").unwrap().is_some());
}
