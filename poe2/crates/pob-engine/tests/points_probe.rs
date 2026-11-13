use pob_engine::Engine;
use std::path::PathBuf;
fn src()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../PathOfBuilding-PoE2/src").canonicalize().unwrap()}
#[test]
#[serial_test::serial]
fn points_probe(){
    let e=Engine::new(&src()).unwrap();
    let xml=pob_import::decode(include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt").trim()).unwrap();
    e.load_build_xml(&xml).unwrap();
    let i=e.info().unwrap();
    println!("level {} class {} / {}", i.level, i.class, i.ascendancy);
    println!("passives: {} / {}", i.points_used, i.points_max);
    println!("ascendancy: {} / {}", i.ascendancy_points_used, i.ascendancy_points_max);
    // The PoE1 hardcode was max = 99 + 23 = 122 and used = raw PointsUsed
    // (which counts weapon-set points), reading ~149 / 122. The PoB2 formula
    // gives max = 99 + questPoints and subtracts shared weapon-set points, so
    // used is weapon-set-adjusted and max reflects PoE2's larger budget. Guard
    // against a regression to the PoE1 numbers (builds may still be a little
    // over, which PoB2 itself flags, so allow a small overage).
    assert!(i.points_max >= 123, "points_max looks like the PoE1 cap: {}", i.points_max);
    assert!(i.points_used <= i.points_max + 5, "used not weapon-set-adjusted: {} vs {}", i.points_used, i.points_max);
}
