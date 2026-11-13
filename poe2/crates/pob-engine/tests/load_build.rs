//! Load a real PoB2 build share code and read stats back out.
//!
//! This is the last premise question: not just "does PoB2 boot" but "does
//! our engine decode a real PoE2 build, apply it, and compute numbers." The
//! share-code transport (base64url + zlib of build XML) is game-agnostic, so
//! `pob_import::decode` works unchanged; the interesting part is whether
//! PoB2's calc path produces a live stat block.
//!
//! Fixture: a level-100 Mercenary / Gemling Legionnaire Flameblast build
//! (a public test code, not anyone's character). Against a PoB2 checkout
//! matching the one it was exported from, our numbers reproduce PoB2's own
//! recorded `<PlayerStat>` values (85/106 exact at time of writing; the
//! remainder trace to PoB2's own version-bound Blasphemy/Heightened-Curse
//! spirit-reservation bug, not our engine — see README.md).

use pob_engine::Engine;
use std::path::PathBuf;

const CODE: &str = include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt");

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn loads_a_real_pob2_build() {
    let xml = pob_import::decode(CODE.trim()).expect("decode PoB2 share code");
    println!("decoded {} bytes of build XML", xml.len());
    assert!(xml.contains("<PathOfBuilding"), "decoded payload is not a PoB build");

    let engine = Engine::new(&pob2_src()).expect("PoB2 engine boot");
    engine.load_build_xml(&xml).expect("load PoB2 build");

    let candidates = [
        "TotalDPS", "CombinedDPS", "AverageDamage", "AverageHit", "Speed",
        "Life", "EnergyShield", "Mana", "TotalEHP", "Str", "Dex", "Int",
    ];
    let mut found = 0;
    for name in candidates {
        if let Ok(Some(v)) = engine.stat(name) {
            println!("  {name:<14} = {v}");
            found += 1;
        }
    }
    assert!(found > 0, "engine produced no stats for the build");
    println!("read {found} live stats from the PoB2 build");
}
