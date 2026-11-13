//! The load-bearing test for the whole architecture.
//!
//! The fixture is a real PoB build code exported by real PoB, so its
//! `<PlayerStat>` values are PoB's own answers. If our embedded engine
//! reproduces them, we have 1:1 numbers. If a future re-vendor breaks that,
//! this test tells us before users do.

use pob_engine::Engine;
use std::path::PathBuf;

/// Upstream checkout. Vendored locally rather than committed; see README.
fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

/// Stats recorded in the fixture by PoB itself, to compare against.
fn expected(xml: &str, stat: &str) -> f64 {
    let needle = format!(r#"<PlayerStat stat="{stat}" value=""#);
    let rest = &xml[xml.find(&needle).expect("stat not in fixture") + needle.len()..];
    rest[..rest.find('"').unwrap()].parse().unwrap()
}

#[test]
#[serial_test::serial]
fn reproduces_pob_stats_for_occultist_fixture() {
    let xml = include_str!("fixtures/occultist-100.xml");

    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    // Spot-check across offence, dot, and speed rather than a single number,
    // so a regression in one calc path cannot hide behind another.
    for stat in ["TotalDPS", "AverageHit", "Speed", "WithPoisonDPS"] {
        let want = expected(xml, stat);
        let got = engine
            .stat(stat)
            .expect("read stat")
            .unwrap_or_else(|| panic!("{stat} missing from output"));

        // PoB's own XML rounds to 6 decimals, so compare relatively.
        let rel = (got - want).abs() / want.abs().max(1.0);
        assert!(
            rel < 1e-6,
            "{stat}: got {got}, PoB recorded {want} (rel err {rel:.2e})"
        );
        println!("{stat}: {got} (PoB: {want})");
    }
}
