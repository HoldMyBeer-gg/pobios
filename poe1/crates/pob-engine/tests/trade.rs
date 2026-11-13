//! The weighted trade search (PoB's TradeQueryGenerator, driven headless).
use pob_engine::Engine;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .unwrap()
}

const WINTER_ORB: &str = include_str!("fixtures/winter-orb-85.xml");

/// A unique-equipped slot must still produce a weighted query.
///
/// PoB's generator only special-cases two uniques (Megalomaniac, Watcher's
/// Eye); for any other unique it searches for an item literally named after
/// the slot, which leaves the item category nil so no mod is ever weighted
/// and the search comes back "found no mods". We route around that by
/// searching for rare upgrades of the same base. The winter-orb fixture wears
/// a unique Unset Ring in Ring 1 and a unique Pagan Wand in Weapon 1, both of
/// which regressed to zero before the fix.
#[test]
#[serial_test::serial]
fn weighted_search_handles_unique_equipped_slots() {
    let e = Engine::new(&pob_src()).unwrap();
    e.load_build_xml(WINTER_ORB).unwrap();

    let weights = [("FullDPS".to_owned(), 1.0), ("TotalEHP".to_owned(), 0.5)];
    for slot in ["Ring 1", "Weapon 1"] {
        match e.trade_query(slot, &weights, 0) {
            Ok(Ok(q)) => {
                let filters = q.matches("\"id\"").count();
                assert!(
                    filters > 0,
                    "{slot} holds a unique and produced no stat filters: the unique-slot dead-end regressed"
                );
            }
            Ok(Err(m)) => panic!("{slot}: {m}"),
            Err(err) => panic!("{slot}: engine error {err}"),
        }
    }
}

/// When PoB finds nothing worth weighting it still builds the query table
/// but sets errMsg, and its own handler shows the message and stops. The
/// error must win here too, not a link to an empty search.
#[test]
#[serial_test::serial]
fn no_mods_to_search_for_surfaces_pobs_message() {
    let e = Engine::new(&pob_src()).unwrap();
    e.load_build_xml(WINTER_ORB).unwrap();
    match e.trade_query("Ring 1", &[], 0) {
        Ok(Err(m)) => assert!(m.contains("found no mods"), "{m}"),
        Ok(Ok(q)) => panic!("expected PoB's no-mods message, got a query: {q}"),
        Err(err) => panic!("engine error {err}"),
    }
}

/// The "Find best" link is PoB's own: its buildUrl (realm, encoded league)
/// plus the bare query gzipped into the path, as TradeQuery.lua builds it
/// when there is no auth token.
#[test]
#[serial_test::serial]
fn the_search_link_is_built_the_way_pob_builds_it() {
    let e = Engine::new(&pob_src()).unwrap();
    e.load_build_xml(WINTER_ORB).unwrap();
    let weights = [("FullDPS".to_owned(), 1.0), ("TotalEHP".to_owned(), 0.5)];
    let q = e.trade_query("Ring 1", &weights, 0).unwrap().unwrap();
    // The bare query PoB gzips, not the API body wrapped around it.
    assert!(q.starts_with('{') && q.contains("\"stats\"") && !q.contains("\"sort\""), "{q}");

    let url = e.trade_search_url(&q, "Hardcore Settlers", 0).unwrap();
    assert!(url.starts_with("https://www.pathofexile.com/trade/search/Hardcore%20Settlers/H4sI"), "{url}");
    let decoded: String = e
        .eval(&format!(
            r#"return require("Classes.TradeHelpers").B64GzipDecode("{}")"#,
            url.rsplit('/').next().unwrap()
        ))
        .unwrap();
    assert_eq!(decoded, q);

    let xbox = e.trade_search_url(&q, "Standard", 2).unwrap();
    assert!(xbox.starts_with("https://www.pathofexile.com/trade/search/xbox/Standard/H4sI"), "{xbox}");
}
