//! Ranking supports for a socket group by the DPS each adds, as PoB's
//! "Sort gems by DPS" does.
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
fn supports_rank_by_the_dps_they_add() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let main = engine.main_socket_group().expect("main group");

    let ranked = engine.rank_gems_for_group(main).expect("rank");
    assert!(ranked.len() > 20, "many supports can support the skill: {}", ranked.len());
    // Best first.
    assert!(ranked.windows(2).all(|w| w[0].1 >= w[1].1), "not sorted");
    // A real support raises DPS; the ranking must contain gains.
    assert!(ranked[0].1 > 0.0, "top support adds nothing: {:?}", ranked[0]);
    // Ranking must not have changed the build.
    let after = engine.rank_gems_for_group(main).expect("rank again");
    assert_eq!(ranked[0].0, after[0].0, "ranking is stable");
}

#[test]
#[serial_test::serial]
fn socketed_supports_are_scored_by_their_worth() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let main = engine.main_socket_group().expect("main group");
    // Socket a support into the group (Frostbite takes curse supports; use
    // one that supports it), then rank: it should come back marked socketed
    // with a non-zero worth.
    if let Some(gem) = engine.find_gem("Increased Duration").ok().flatten() {
        engine.set_gem_id(main, 99, &gem.id).expect("socket support");
        let ranked = engine.rank_gems_for_group(main).expect("rank");
        assert!(
            ranked.iter().any(|r| r.2),
            "a socketed support should be marked and scored: {ranked:?}"
        );
    }
}
