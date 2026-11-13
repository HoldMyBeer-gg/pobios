//! Find Timeless Jewel: the seed tables load, and PoB's own search runs.

use pob_engine::timeless::{DesiredNode, TimelessState};
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
fn inflate_is_real_zlib_not_the_headless_stub() {
    // The headless definition file stubs Inflate to return "". Every seed
    // table is a zlib stream read through it, so with the stub the whole
    // jewel finder silently searched nothing.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    let round_trip: String = engine
        .eval(r#"return Inflate(Deflate("timeless jewels"))"#)
        .expect("eval");
    assert_eq!(round_trip, "timeless jewels");

    let (ok, err): (Option<String>, Option<String>) = engine
        .eval(r#"return Inflate("not a zlib stream")"#)
        .expect("eval");
    assert!(ok.is_none() && err.is_some(), "garbage must report an error");
}

#[test]
#[serial_test::serial]
fn seed_tables_load_and_answer_lookups() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // Lethal Pride, a notable inside the fixture's jewel socket's radius.
    let (len, first): (usize, i64) = engine
        .eval(
            r#"
            local b = __mainObject__.main.modes["BUILD"]
            local socket = b.spec.tree.nodes[61419]
            local notable
            for id in pairs(socket.nodesInRadius[3]) do
                local n = b.spec.tree.nodes[id]
                if n and n.isNotable then notable = id break end
            end
            local t = data.readLUT(12345, notable, 2)
            return #t, t[1] or -1
            "#,
        )
        .expect("eval");
    assert!(len >= 1, "readLUT returned nothing: the LUT did not load");
    assert!(first >= 0);
}

#[test]
#[serial_test::serial]
fn the_dialogs_options_are_pobs_own() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let state = engine.timeless_state().expect("state");
    assert_eq!(state.jewel_type, 1, "a fresh build defaults to Glorious Vanity");

    let opts = engine.timeless_options().expect("options");
    assert_eq!(opts.jewel_types.len(), 11, "PoB lists eleven jewel kinds");
    assert_eq!(opts.jewel_types[1].label, "Lethal Pride");
    assert_eq!(opts.conquerors.len(), 4, "three conquerors plus Any");
    assert_eq!(opts.conquerors[0].label, "Any");
    // 2.67+ adds "Mana Cost Efficiency" beside the legacy "Mana Cost".
    assert_eq!(opts.devotion_variants.len(), 17);
    assert!(opts.devotion_variants.iter().any(|d| d.label == "Mana Cost Efficiency"));
    assert!(opts.fallback_modes.iter().any(|m| m.contains("DPS")));

    // Sockets: the multi-socket entry first, then every jewel socket, the
    // allocated ones flagged rather than prefixed.
    assert_eq!(opts.sockets[0].id, -1);
    assert!(opts.sockets.len() > 20);
    assert!(opts.sockets.iter().any(|s| s.allocated));
    assert!(opts.sockets.iter().all(|s| !s.label.starts_with("# ")));

    // Glorious Vanity offers its notables and small passives with stats.
    assert!(opts.nodes.len() > 50);
    assert!(opts.nodes.iter().any(|n| n.notable && !n.stats.is_empty()));
    assert!(opts.nodes.iter().all(|n| n.id.starts_with("vaal_")));

    // Another type, another node list and conqueror list.
    engine
        .set_timeless_state(&TimelessState { jewel_type: 2, socket: 61419, ..state.clone() })
        .expect("set");
    let opts = engine.timeless_options().expect("options");
    assert!(opts.nodes.iter().all(|n| n.id.starts_with("karui_") || n.total));
    assert!(opts.nodes[0].total, "Lethal Pride leads with Total Strength");
    assert_eq!(opts.conquerors[1].label, "Kaom (Strength of Blood)");
}

#[test]
#[serial_test::serial]
fn pobs_own_search_runs_headless_and_its_columns_are_read_back() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let mut state = engine.timeless_state().expect("state");
    state.jewel_type = 2;
    state.socket = 61419;
    state.desired = vec![
        DesiredNode::parse("karui_notable_add_double_damage, 1, 0, 0").unwrap(),
        DesiredNode::parse("karui_notable_add_strength, 0.5, 0, 0").unwrap(),
    ];
    engine.set_timeless_state(&state).expect("set");
    // What was set is what is read back, through PoB's own text form.
    assert_eq!(engine.timeless_state().expect("state").desired, state.desired);

    let start = std::time::Instant::now();
    let search = engine.timeless_search(&[]).expect("search");
    println!("{} seeds in {:?}", search.results.len(), start.elapsed());
    assert!(search.results.len() > 100, "Lethal Pride has thousands of seeds");
    assert_eq!(search.desired.len(), 2);
    // Columns are headed by the legion node's name, not PoB's stat text.
    assert!(search.desired.iter().all(|n| !n.contains('%')), "{:?}", search.desired);

    // Best first, and the per-node columns add up to PoB's total.
    let top = &search.results[0];
    assert!(search.results.windows(2).all(|w| w[0].total >= w[1].total));
    assert_eq!(top.matches.len(), 2);
    let sum: f64 = top.matches.iter().map(|m| m.weight).sum();
    assert!((sum - top.total).abs() < 0.02, "columns {sum} vs total {}", top.total);
    assert!(
        top.matches.iter().any(|m| !m.targets.is_empty()),
        "a match names the passives it replaced"
    );
    assert!(top.socket_id.is_none(), "single-socket search carries no socket");

    // The trade URL is PoB's query for the top seeds, against the league
    // the user named.
    let url = engine
        .timeless_trade_url(0, 1, "Standard", 0, 0)
        .expect("url")
        .expect("a url");
    // PoB 2.67+ gzips the query into the path (`H4sI` is gzip in base64).
    assert!(url.starts_with("https://www.pathofexile.com/trade/search/Standard/H4sI"), "{url}");
    let query = trade_query(&engine, &url);
    assert!(query.contains(&format!("{}", top.seed)), "top seed {} missing from {query}", top.seed);
    // PoB's buildUrl encodes the league itself; encoding it first as well
    // would turn the space into %2520 and name a league that does not exist.
    let hc = engine.timeless_trade_url(0, 1, "Hardcore Settlers", 0, 0).expect("url").unwrap();
    assert!(hc.starts_with("https://www.pathofexile.com/trade/search/Hardcore%20Settlers/H4sI"), "{hc}");
    let xbox = engine.timeless_trade_url(0, 2, "Standard", 2, 4).expect("url").unwrap();
    assert!(xbox.contains("/trade/search/xbox/Standard/"), "{xbox}");
    // One seed: the query names it and no other.
    let one = engine.timeless_trade_url(1, 0, "Standard", 0, 0).expect("url").unwrap();
    let second = search.results[1].seed.to_string();
    let one = trade_query(&engine, &one);
    assert!(one.contains(&second), "{one}");
    assert!(!one.contains(&top.seed.to_string()), "a single-seed query must not carry the row above: {one}");

    // Adding a result puts a Lethal Pride with that seed in the build.
    let items_before = engine.item_list().expect("items").len();
    assert!(engine.timeless_add_to_build(0).expect("add").is_some());
    let items = engine.item_list().expect("items");
    assert_eq!(items.len(), items_before + 1);
    assert!(items.iter().any(|i| i.name.contains("Lethal Pride") && i.name.contains(&top.seed.to_string())));
    // And it went straight into the socket the search was for.
    let jewel = items.iter().find(|i| i.name.contains("Lethal Pride")).unwrap();
    assert!(jewel.equipped_in.iter().any(|s| s == "Jewel 61419"), "{:?}", jewel.equipped_in);
    assert!(engine.tree().unwrap().nodes.iter().any(|n| n.id == 61419 && !n.jewel_rings.is_empty()));
    // Re-reading the same results shows it as added; a new search would
    // not, and PoB's list behaves the same way.
    assert!(engine.timeless_results().expect("results").results[0].added);
    assert!(!engine.timeless_search(&[]).expect("search").results[0].added);
}

#[test]
#[serial_test::serial]
fn a_multi_socket_search_labels_each_result_with_its_socket() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let mut state = engine.timeless_state().expect("state");
    state.jewel_type = 2;
    state.socket = -1;
    state.desired = vec![DesiredNode::parse("karui_notable_add_double_damage, 1, 0, 0").unwrap()];
    engine.set_timeless_state(&state).expect("set");

    let search = engine.timeless_search(&[]).expect("search");
    assert!(!search.results.is_empty());
    assert!(search.results.iter().all(|r| r.socket_id.is_some() && !r.socket_label.is_empty()));
}

/// The JSON query inside a PoB trade link, decoded by PoB's own helper.
fn trade_query(engine: &Engine, url: &str) -> String {
    let encoded = url.rsplit('/').next().unwrap();
    engine
        .eval::<String>(&format!(
            r#"return require("Classes.TradeHelpers").B64GzipDecode("{encoded}")"#
        ))
        .expect("PoB decodes its own trade link")
}

/// Militant Faith's "protect allocated nodes" list (PoB #10276: one checkbox
/// per allocated notable in radius) is offered where PoB shows it, and a
/// protected notable is never one a result replaces.
#[test]
#[serial_test::serial]
fn protected_notables_are_never_replaced() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let mut state = engine.timeless_state().expect("state");
    state.jewel_type = 2; // Lethal Pride: PoB shows no protect list.
    state.socket = 61419;
    state.socket_filter = true;
    engine.set_timeless_state(&state).expect("set");
    assert!(engine.timeless_options().unwrap().protect_candidates.is_empty(), "Lethal Pride has no protect list in PoB");

    state.jewel_type = 4; // Militant Faith
    state.desired = vec![DesiredNode::parse("templar_notable_devotion, 1, 0, 0").unwrap()];
    engine.set_timeless_state(&state).expect("set");
    let candidates = engine.timeless_options().unwrap().protect_candidates;
    assert!(!candidates.is_empty(), "Militant Faith with Filter Nodes offers the allocated notables");

    // "Replaced" as PoB's search defines it: the seed's lookup-table entry
    // for that node is a replacement (>= data.timelessJewelAdditions). A
    // protected notable may still be a match target, since Militant Faith
    // adds devotion to it; what protection forbids is replacing it.
    let replaces = |seed: i64, name: &str| -> bool {
        engine
            .eval::<bool>(&format!(
                r#"local spec = __mainObject__.main.modes["BUILD"].spec
                for id, node in pairs(spec.nodes) do
                    if node.dn == {name:?} then
                        return data.readLUT({seed}, id, 4)[1] >= data.timelessJewelAdditions
                    end
                end
                error("no node named " .. {name:?})"#
            ))
            .expect("readLUT")
    };
    let open = engine.timeless_search(&[]).expect("search");
    let victim = candidates
        .iter()
        .find(|c| open.results.iter().any(|r| replaces(r.seed, c)))
        .unwrap_or_else(|| panic!("no candidate is ever replaced: {candidates:?}"))
        .clone();

    let guarded = engine.timeless_search(std::slice::from_ref(&victim)).expect("search");
    assert!(!guarded.results.is_empty());
    assert!(guarded.results.len() < open.results.len(), "protecting {victim} dropped no seeds");
    for r in &guarded.results {
        assert!(!replaces(r.seed, &victim), "seed {} replaces protected {victim}", r.seed);
    }
}
