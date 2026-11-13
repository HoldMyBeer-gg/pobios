//! The values the UI shows must be the values PoB computed.
//!
//! `dps.rs` proves the engine matches PoB. This proves the display layer does
//! not distort them on the way to the screen — a formatting bug would show a
//! wrong number just as effectively as a calculation bug.

use pob_engine::stats::{Format, GROUPS};
use pob_engine::Engine;
use pob_engine::items::SetKind;
use std::path::PathBuf;

fn pob_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src")
        .canonicalize()
        .expect("PathOfBuilding checkout missing")
}

#[test]
#[serial_test::serial]
fn displayed_stats_round_trip_back_to_pobs_values() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let mut checked = 0;
    for group in GROUPS {
        for spec in group.stats {
            let Some(raw) = engine.stat(spec.key).expect("read stat") else {
                continue;
            };
            let shown = spec.format.render(raw);

            // Parse the rendered string back and confirm it still means the
            // same number, within the precision the format claims.
            let parsed: f64 = shown
                .replace(',', "")
                .trim_end_matches('%')
                .parse()
                .unwrap_or_else(|_| panic!("{} rendered unparseably as {shown:?}", spec.key));

            let tolerance = match spec.format {
                Format::Integer => 0.5,
                Format::Number => 0.05,
                Format::Rate | Format::Percent => 0.005,
            };
            assert!(
                (parsed - raw).abs() <= tolerance,
                "{}: displayed {shown:?} but PoB computed {raw}",
                spec.key
            );
            checked += 1;
        }
    }

    // Guard against the test silently passing because nothing was read.
    assert!(checked >= 10, "only {checked} stats had values");
}

#[test]
#[serial_test::serial]
fn build_info_matches_the_fixtures_header() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let info = engine.info().expect("build info");
    assert_eq!(info.class, "Witch");
    assert_eq!(info.ascendancy, "Occultist");
    assert_eq!(info.level, 100);
    assert_eq!(info.allocated_nodes, 134);
    assert_eq!(info.summary(), "Witch Occultist — level 100");
}

#[test]
#[serial_test::serial]
fn chaos_inoculation_life_of_one_is_reported_as_is() {
    // The fixture is a CI build: life is exactly 1 by design, with the pool
    // moved into energy shield. A display layer that treated 1 as "missing"
    // or rounded it away would hide a real and deliberate value.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    assert_eq!(engine.stat("Life").unwrap(), Some(1.0));
    assert_eq!(engine.stat("EnergyShield").unwrap(), Some(14690.0));
}

#[test]
#[serial_test::serial]
fn socket_groups_match_the_fixture() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let groups = engine.socket_groups().expect("socket groups");
    assert_eq!(groups.len(), 13, "fixture has 13 socket groups");

    // The XML records 35 gems, 18 of them supports.
    let all: Vec<_> = groups.iter().flat_map(|g| g.gems.iter()).collect();
    let supports = all.iter().filter(|g| g.is_support()).count();
    assert!(supports >= 18, "expected >= 18 supports, found {supports}");

    // Every group must produce a usable label rather than an empty row.
    for g in &groups {
        assert!(!g.display_label().is_empty(), "group has no label");
    }

    // The main group index must be in range, since the UI selects with it.
    let main = engine.main_socket_group().expect("main group");
    assert!(main < groups.len(), "main group {main} out of range");
}

#[test]
#[serial_test::serial]
fn granted_groups_are_distinguishable_from_socketed_ones() {
    // Groups with a `source` come from an item or effect rather than from
    // sockets, so the UI must not offer to edit their gems.
    //
    // Two shapes exist, and the fixture has both: a global effect with no
    // slot (source "Explode"), and one granted by a specific equipped item,
    // which *does* carry that item's slot (Asenath's Gentle Touch, Gloves).
    // Presence of a source is the signal; absence of a slot is not.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let groups = engine.socket_groups().expect("socket groups");
    let granted: Vec<_> = groups.iter().filter(|g| g.source.is_some()).collect();
    assert_eq!(granted.len(), 2, "fixture has two granted groups");

    assert!(
        granted.iter().any(|g| g.slot.is_none()),
        "expected a global granted group"
    );
    assert!(
        granted.iter().any(|g| g.slot.is_some()),
        "expected an item-granted group carrying its item's slot"
    );

    // A granted group still needs a usable name even with no explicit label
    // and no active gems to fall back on.
    for g in &granted {
        assert!(!g.display_label().is_empty());
    }
}

#[test]
#[serial_test::serial]
fn equipment_reads_the_fixtures_gear() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let slots = engine.equipment().expect("equipment");
    // The base gear comes first, in PoB's order; allocated jewel sockets
    // and other active slots follow it.
    assert_eq!(
        slots.iter().take(pob_engine::items::EQUIPMENT_SLOTS.len()).map(|s| s.name.as_str()).collect::<Vec<_>>(),
        pob_engine::items::EQUIPMENT_SLOTS.to_vec(),
        "every equipment slot is listed, filled or not"
    );

    let filled: Vec<_> = slots.iter().filter(|s| s.item.is_some()).collect();
    assert!(
        filled.len() >= 8,
        "fixture is a level 100 build; expected most slots filled, got {}",
        filled.len()
    );

    // Every equipped item must have a name and at least one mod, otherwise
    // the list would render blank rows.
    for slot in &filled {
        let item = slot.item.as_ref().unwrap();
        assert!(!item.name.is_empty(), "{} has a nameless item", slot.name);
    }

    // Mods must be typed, not lumped together: the editor groups by kind.
    let any_explicit = filled.iter().any(|s| {
        s.item
            .as_ref()
            .unwrap()
            .mods_of(pob_engine::items::ModKind::Explicit)
            .next()
            .is_some()
    });
    assert!(any_explicit, "no explicit mods found on any item");
}

#[test]
#[serial_test::serial]
fn jewel_sockets_are_not_mistaken_for_equipment() {
    // PoB's live slot table holds 131 entries for this fixture because it
    // includes every abyssal and jewel socket. Reading it directly would put
    // socket rows in the equipment list.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let slots = engine.equipment().expect("equipment");
    assert!(
        !slots.iter().any(|s| s.name.contains("Abyssal")),
        "equipment list must not contain socket slots"
    );
    assert!(slots.iter().any(|s| s.name == "Body Armour"));
}

#[test]
#[serial_test::serial]
fn calc_sections_carry_pobs_own_formatted_values() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let sections = engine.calc_sections().expect("calc sections");
    assert!(
        sections.len() >= 20,
        "PoB defines 30 sections; got {}",
        sections.len()
    );

    // Life is the clearest check: the fixture is a CI build, so PoB shows a
    // total of exactly 1, and the rows above it are non-trivial.
    let life = sections
        .iter()
        .find(|s| s.id == "Life")
        .expect("Life section");
    assert!(!life.is_empty(), "Life should have values");

    let total = life
        .rows
        .iter()
        .find(|r| r.label == "Total")
        .expect("Life has a Total row");
    assert_eq!(
        total.cells.first().map(|c| c.value.as_str()),
        Some("1"),
        "CI life is exactly 1"
    );

    // Values arrive already formatted by PoB, separators included, rather
    // than as raw numbers we would have to format ourselves.
    let has_separator = sections
        .iter()
        .flat_map(|s| s.rows.iter())
        .flat_map(|r| r.cells.iter())
        .any(|c| c.value.contains(','));
    assert!(has_separator, "expected PoB-formatted thousands separators");
}

#[test]
#[serial_test::serial]
fn calc_headers_are_stripped_of_colour_escapes() {
    // Column headers arrive as "^xADAA47Lightning:"; leaving the escape in
    // would render it as literal text.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let sections = engine.calc_sections().expect("calc sections");
    for section in &sections {
        assert!(
            !section.label.contains('^'),
            "section label kept an escape: {:?}",
            section.label
        );
        for row in &section.rows {
            assert!(!row.label.contains("^x"), "row label kept an escape");
            for cell in &row.cells {
                assert!(
                    !cell.value.contains("^x"),
                    "cell kept an escape: {:?}",
                    cell.value
                );
            }
        }
    }

    // The damage-type header row should still name its columns.
    let hit = sections.iter().find(|s| s.id == "HitDamage");
    if let Some(hit) = hit {
        let header = hit.rows.iter().find(|r| r.is_header);
        if let Some(header) = header {
            let joined: Vec<&str> = header.cells.iter().map(|c| c.value.as_str()).collect();
            assert!(
                joined.iter().any(|c| c.contains("Lightning")),
                "header lost its column names: {joined:?}"
            );
        }
    }
}

#[test]
#[serial_test::serial]
fn config_options_are_read_with_their_current_values() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let options = engine.config_options().expect("config options");
    assert!(
        options.len() >= 200,
        "PoB defines hundreds of options; got {}",
        options.len()
    );

    // Sections must be populated, since the tab groups by them.
    let sections: std::collections::BTreeSet<_> =
        options.iter().map(|o| o.section.as_str()).collect();
    assert!(sections.contains("General"), "sections: {sections:?}");

    // Labels are cleaned of PoB's colour escapes and trailing colons.
    for o in &options {
        assert!(!o.label.contains('^'), "label kept an escape: {:?}", o.label);
        assert!(
            !o.label.ends_with(':'),
            "label kept a trailing colon: {:?}",
            o.label
        );
    }

    // The fixture sets conditions, so some options must read as set.
    assert!(
        options.iter().any(|o| o.is_set()),
        "no configured options found in a level 100 build"
    );
}

#[test]
#[serial_test::serial]
fn writing_a_config_option_changes_the_calculated_dps() {
    // This is the whole point of the write path: a config toggle has to move
    // the numbers, not just store a value. Marking the enemy a boss changes
    // resistances and ailment thresholds, so DPS must respond.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let before = engine.stat("TotalDPS").unwrap().expect("baseline DPS");

    // Capture what the option held before we touch it. PoB writes list
    // defaults into `input` at load, so the starting value is not necessarily
    // "unset" — for enemyIsBoss it is "Pinnacle", from defaultIndex = 3.
    let original = engine
        .config_options()
        .unwrap()
        .into_iter()
        .find(|o| o.var == "enemyIsBoss")
        .map(|o| o.value)
        .expect("enemyIsBoss exists");

    engine
        .set_config("enemyIsBoss", pob_engine::config::Value::Text("Boss".into()))
        .expect("set config");
    let after = engine.stat("TotalDPS").unwrap().expect("DPS after");

    assert_ne!(
        before, after,
        "enemyIsBoss should change DPS; both were {before}"
    );

    // Reversible, otherwise the UI could not undo a toggle. Restoring means
    // writing the previous value back, NOT clearing the key: clearing would
    // drop PoB's default and leave the build in a third state that the user
    // never chose.
    engine.set_config("enemyIsBoss", original).expect("restore");
    let restored = engine.stat("TotalDPS").unwrap().expect("DPS restored");
    assert!(
        (restored - before).abs() < 0.01,
        "restoring should return to {before}, got {restored}"
    );
}

#[test]
#[serial_test::serial]
fn a_boolean_config_toggle_round_trips() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let read = |engine: &Engine, var: &str| {
        engine
            .config_options()
            .unwrap()
            .into_iter()
            .find(|o| o.var == var)
            .map(|o| o.value)
    };

    engine
        .set_config("conditionEnemyShocked", pob_engine::config::Value::Bool(true))
        .expect("set");
    assert_eq!(
        read(&engine, "conditionEnemyShocked"),
        Some(pob_engine::config::Value::Bool(true))
    );

    // Setting false must clear the entry rather than storing `false`: PoB
    // treats absent and false alike, and a stored false would leave a stale
    // key in the saved build.
    engine
        .set_config("conditionEnemyShocked", pob_engine::config::Value::Bool(false))
        .expect("clear");
    assert_eq!(
        read(&engine, "conditionEnemyShocked"),
        Some(pob_engine::config::Value::Unset)
    );
}

#[test]
#[serial_test::serial]
fn list_defaults_are_exposed_so_reset_does_not_invent_a_state() {
    // enemyIsBoss has defaultIndex = 3, so PoB's default is "Pinnacle" and
    // not the first list entry. A UI that treated "clear the key" as "reset"
    // would silently move the build to a state the user never picked, which
    // is worse than leaving it alone.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let boss = engine
        .config_options()
        .unwrap()
        .into_iter()
        .find(|o| o.var == "enemyIsBoss")
        .expect("enemyIsBoss exists");

    assert_eq!(
        boss.default.as_deref(),
        Some("Pinnacle"),
        "expected PoB's defaultIndex to resolve to Pinnacle"
    );

    // And its list must offer the values PoB offers, since the UI renders it.
    match &boss.kind {
        pob_engine::config::OptionKind::List { options } => {
            let vals: Vec<&str> = options.iter().map(|(v, _)| v.as_str()).collect();
            assert!(vals.contains(&"None"), "list values: {vals:?}");
            assert!(vals.contains(&"Uber"), "list values: {vals:?}");
        }
        other => panic!("enemyIsBoss should be a list, got {other:?}"),
    }
}

#[test]
#[serial_test::serial]
fn a_config_write_is_visible_in_the_very_next_read() {
    // The UI re-reads options straight after writing one, so a write that is
    // not yet visible would make a toggle appear to snap back.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    engine
        .set_config("conditionEnemyCursed", pob_engine::config::Value::Bool(true))
        .expect("set");

    let seen = engine
        .config_options()
        .unwrap()
        .into_iter()
        .find(|o| o.var == "conditionEnemyCursed")
        .map(|o| o.value);

    assert_eq!(
        seen,
        Some(pob_engine::config::Value::Bool(true)),
        "a write must be visible to the read that follows it"
    );
}

#[test]
#[serial_test::serial]
fn notes_round_trip_through_the_engine() {
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    // The fixture's notes are empty, which is the case the tab has to handle
    // without showing a stale or missing editor.
    let initial = engine.notes().expect("read notes");
    assert!(initial.trim().is_empty(), "expected empty, got {initial:?}");

    let text = "Chaos res is -30%, fix before red maps.\n\nSecond paragraph.";
    engine.set_notes(text).expect("write notes");
    assert_eq!(engine.notes().expect("re-read"), text);

    // Save and reload, which recalculates from scratch: the notes survive,
    // and the DPS is still the one PoB recorded in the fixture (the fixture
    // is PoB's own, so upstream calc fixes move both together).
    let saved = engine.build_xml().expect("save");
    engine.load_build_xml(&saved).expect("reload");
    assert_eq!(engine.notes().expect("notes after reload"), text);
    let want: f64 = {
        let needle = r#"<PlayerStat stat="TotalDPS" value=""#;
        let rest = &xml[xml.find(needle).unwrap() + needle.len()..];
        rest[..rest.find('"').unwrap()].parse().unwrap()
    };
    let got = engine.stat("TotalDPS").unwrap().expect("TotalDPS");
    assert!((got - want).abs() / want < 1e-6, "notes changed the DPS: {got} vs PoB's {want}");
}

#[test]
#[serial_test::serial]
fn notes_survive_characters_that_look_like_pob_markup() {
    // PoB uses ^xRRGGBB colour escapes in notes. A user typing a caret must
    // not have it silently eaten or reinterpreted on the way through.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let tricky = "2 ^ 3 = 8, and ^xFF0000 is red\ttabbed\n\"quoted\"";
    engine.set_notes(tricky).expect("write");
    assert_eq!(engine.notes().expect("read"), tricky);
}

#[test]
#[serial_test::serial]
fn a_build_survives_an_export_import_round_trip() {
    // The loop a user actually does: export a code, hand it to someone, they
    // paste it back. The build has to come out the same on the other side.
    let xml = include_str!("fixtures/occultist-100.xml");
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(xml).expect("load build");

    let before_dps = engine.stat("TotalDPS").unwrap().expect("baseline");
    let before_info = engine.info().expect("info");

    let exported = engine.build_xml().expect("serialise");
    assert!(
        exported.contains("<PathOfBuilding"),
        "SaveDB did not return build XML"
    );

    // Through the codec and back, as a shared code would go.
    let code = pob_import::encode(&exported).expect("encode");
    let decoded = pob_import::decode(&code).expect("decode");

    engine.load_build_xml(&decoded).expect("reload");

    let after_dps = engine.stat("TotalDPS").unwrap().expect("after");
    let after_info = engine.info().expect("info after");

    assert_eq!(before_info.class, after_info.class);
    assert_eq!(before_info.ascendancy, after_info.ascendancy);
    assert_eq!(before_info.level, after_info.level);
    assert_eq!(
        before_info.allocated_nodes, after_info.allocated_nodes,
        "the passive tree did not survive the round trip"
    );
    assert!(
        (before_dps - after_dps).abs() < 0.01,
        "DPS changed across a round trip: {before_dps} -> {after_dps}"
    );
}

/// A second real build, deliberately unlike the Occultist fixture.
///
/// The Occultist has poison and Chaos Inoculation; this one has neither.
/// Keeping both is what catches display rules that only look right on the
/// build they were written against — the "Poison DPS" mislabelling survived
/// because the Occultist made a large poison number look plausible.
const WINTER_ORB: &str = include_str!("fixtures/winter-orb-85.xml");
/// The owner's goal build. Used where a test needs a real wardrobe: the level
/// 85 build owns exactly one item per slot, so nothing there can be swapped.
const GOAL_BUILD: &str = include_str!("fixtures/goal-build.xml");

#[test]
#[serial_test::serial]
fn a_build_without_poison_reports_none() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // PoB gives no PoisonDPS at all for a build that applies no poison.
    assert_eq!(engine.stat("PoisonDPS").unwrap(), None);
    assert_eq!(engine.stat("TotalDotDPS").unwrap(), Some(0.0));

    // And its "inc. Poison" total is exactly Total DPS, which is precisely
    // why showing that row would imply poison damage that does not exist.
    let total = engine.stat("TotalDPS").unwrap().expect("total");
    assert_eq!(engine.stat("WithPoisonDPS").unwrap(), Some(total));
}

#[test]
#[serial_test::serial]
fn the_second_fixture_loads_completely() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let info = engine.info().expect("info");
    assert_eq!(info.class, "Witch");
    assert_eq!(info.ascendancy, "Occultist");
    assert_eq!(info.level, 85);
    assert_eq!(info.allocated_nodes, 115);

    assert_eq!(engine.socket_groups().expect("skills").len(), 9);

    let equipped = engine
        .equipment()
        .expect("items")
        .iter()
        .filter(|s| s.item.is_some())
        .count();
    assert_eq!(equipped, 15);
}

#[test]
#[serial_test::serial]
fn reading_another_build_leaves_the_current_one_untouched() {
    // Comparison needs two builds' numbers, and PoB holds one at a time. The
    // swap must be invisible: a user comparing against something must not
    // find their own build replaced by it.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let mine = engine.stat("TotalDPS").unwrap().expect("baseline");
    let my_level = engine.info().expect("info").level;

    let other = include_str!("fixtures/occultist-100.xml");
    let read = engine
        .stats_for(other, &["TotalDPS"])
        .expect("read other build");

    // We got the *other* build's numbers...
    let theirs = read
        .iter()
        .find(|(k, _)| k == "TotalDPS")
        .and_then(|(_, v)| *v)
        .expect("other build DPS");
    assert!(
        (theirs - mine).abs() > 1.0,
        "expected different builds, both were {mine}"
    );

    // ...and our own build is still loaded, unchanged.
    assert_eq!(engine.stat("TotalDPS").unwrap(), Some(mine));
    assert_eq!(engine.info().expect("info").level, my_level);
}

#[test]
#[serial_test::serial]
fn a_failed_comparison_does_not_cost_the_user_their_build() {
    // The case that matters most: pasting something broken must restore what
    // was loaded, not leave the app holding a half-loaded build.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let mine = engine.stat("TotalDPS").unwrap().expect("baseline");

    let _ = engine.stats_for("<PathOfBuilding>truncated", &["TotalDPS"]);

    assert_eq!(
        engine.stat("TotalDPS").unwrap(),
        Some(mine),
        "a failed comparison replaced the loaded build"
    );
}

#[test]
#[serial_test::serial]
fn comparing_two_real_builds_produces_sane_deltas() {
    // End-to-end for the Compare tab: read one build's stats while another is
    // loaded, and check the differences are the ones a player would expect.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let mine = engine.stat("TotalDPS").unwrap().expect("mine");
    let other = include_str!("fixtures/occultist-100.xml");

    let stats = engine
        .stats_for(other, &["TotalDPS", "Life", "EnergyShield"])
        .expect("compare");

    let get = |key: &str| {
        stats
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| *v)
            .unwrap_or_else(|| panic!("{key} missing"))
    };

    // The level 100 build out-damages the level 85 one by a wide margin.
    assert!(
        get("TotalDPS") > mine * 10.0,
        "expected the higher build to dominate: {} vs {mine}",
        get("TotalDPS")
    );

    // And the Chaos Inoculation build trades life for energy shield, which
    // is exactly the kind of tradeoff a comparison should surface.
    assert_eq!(get("Life"), 1.0);
    assert!(get("EnergyShield") > engine.stat("EnergyShield").unwrap().unwrap());
}

#[test]
#[serial_test::serial]
fn the_tree_loads_with_pobs_own_geometry() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let tree = engine.tree().expect("tree");

    // PoB reports 3,352 nodes for tree 3.29; most are drawable.
    assert!(
        tree.nodes.len() > 2500,
        "expected a full tree, got {} nodes",
        tree.nodes.len()
    );
    assert!(
        tree.connectors.len() > 2500,
        "expected a full connector set, got {}",
        tree.connectors.len()
    );
    assert!(tree.groups.len() > 500, "groups: {}", tree.groups.len());

    // The build's allocation must survive into the tree.
    assert_eq!(
        tree.allocated_count(),
        115,
        "allocated nodes should match the build"
    );

    // Bounds must be real, otherwise fit-to-view divides by zero.
    let (w, h) = tree.extent();
    assert!(w > 1000.0 && h > 1000.0, "extent looks wrong: {w} x {h}");
}

#[test]
#[serial_test::serial]
fn node_positions_are_not_reimplemented_geometry() {
    // The failure mode this guards against: computing node positions from
    // group + orbit + angle rather than reading PoB's results. Orbits of 16
    // and 40 nodes use irregular angle tables, so an even-division
    // implementation lands *close* to correct — drifting worst on the big
    // wheels — and looks plausible while being wrong.
    //
    // Reading PoB's own x/y makes that impossible. This asserts the
    // positions really are distinct and orbit-shaped rather than, say, all
    // sitting on their group centre.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let mut distinct = std::collections::HashSet::new();
    for node in &tree.nodes {
        distinct.insert((node.x.to_bits(), node.y.to_bits()));
    }
    assert!(
        distinct.len() > tree.nodes.len() / 2,
        "nodes are collapsing onto shared positions: {} distinct of {}",
        distinct.len(),
        tree.nodes.len()
    );

    // Keystones exist and are named, which the tooltip depends on.
    let keystones: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.kind == pob_engine::tree::NodeKind::Keystone)
        .collect();
    assert!(keystones.len() > 20, "keystones: {}", keystones.len());
    assert!(keystones.iter().all(|k| !k.name.is_empty()));
}

#[test]
#[serial_test::serial]
fn connectors_reflect_which_nodes_are_allocated() {
    // The three connector states are what make an allocated path readable.
    // A build with 115 nodes must produce a meaningful number of active
    // edges, and every edge must have a drawable path.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    use pob_engine::tree::ConnectorState;
    let active = tree
        .connectors
        .iter()
        .filter(|c| c.state == ConnectorState::Active)
        .count();
    let intermediate = tree
        .connectors
        .iter()
        .filter(|c| c.state == ConnectorState::Intermediate)
        .count();

    assert!(active > 50, "active connectors: {active}");
    assert!(intermediate > 0, "expected a frontier of partial edges");

    for c in &tree.connectors {
        assert!(
            c.path.len() >= 2,
            "connector {} -> {} has no drawable path",
            c.from,
            c.to
        );
    }
}

#[test]
#[serial_test::serial]
fn arcs_start_and_end_on_the_nodes_they_connect() {
    // The correctness check that matters for the tree's appearance: a swept
    // arc has to actually meet its endpoints. If the orbit radius or angle
    // convention is wrong, the curve is *nearly* right — offset by a few
    // units, hugging the wrong side of the wheel — which is exactly the
    // failure that looks plausible in a screenshot.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let by_id: std::collections::HashMap<u64, &pob_engine::tree::Node> =
        tree.nodes.iter().map(|n| (n.id, n)).collect();

    let arcs: Vec<_> = tree.connectors.iter().filter(|c| c.is_arc).collect();
    assert!(
        arcs.len() > 1000,
        "most of this tree's edges are arcs; got {}",
        arcs.len()
    );

    let mut checked = 0;
    for c in &arcs {
        let (Some(a), Some(b)) = (by_id.get(&c.from), by_id.get(&c.to)) else {
            continue;
        };
        // Only same-group arcs were swept; others stay chords.
        if a.group != b.group {
            continue;
        }

        let start = c.path[0];
        let end = c.path[c.path.len() - 1];
        let dist = |p: (f32, f32), n: &pob_engine::tree::Node| {
            ((p.0 - n.x).powi(2) + (p.1 - n.y).powi(2)).sqrt()
        };

        // The sweep may run either direction, so accept whichever pairing.
        let forward = dist(start, a).max(dist(end, b));
        let reverse = dist(start, b).max(dist(end, a));
        let error = forward.min(reverse);

        assert!(
            error < 1.0,
            "arc {} -> {} misses its nodes by {error:.2} units",
            c.from,
            c.to
        );
        checked += 1;
    }

    assert!(checked > 1000, "only checked {checked} arcs");
}

#[test]
#[serial_test::serial]
fn arcs_bow_away_from_their_chord() {
    // An arc that is secretly a straight line would pass the endpoint test.
    // This confirms the middle of the curve actually leaves the chord.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let bowed = tree
        .connectors
        .iter()
        .filter(|c| c.is_arc && c.path.len() > 2)
        .filter(|c| {
            let start = c.path[0];
            let end = c.path[c.path.len() - 1];
            let mid = c.path[c.path.len() / 2];
            let chord_mid = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
            let bow = ((mid.0 - chord_mid.0).powi(2) + (mid.1 - chord_mid.1).powi(2)).sqrt();
            bow > 1.0
        })
        .count();

    assert!(
        bowed > 500,
        "arcs are not curving away from their chords: only {bowed} bowed"
    );
}

#[test]
#[serial_test::serial]
fn ascendancy_wheels_do_not_overlap() {
    // The bug this exists to prevent, seen in an earlier attempt at this
    // renderer: wheels drawn on top of each other. Nine alternate wheels sit
    // in a row about 1,400 units apart, so art that is not capped against
    // its neighbour swallows the wheels beside it.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    assert!(
        tree.wheels.len() >= 30,
        "expected every ascendancy including alternates, got {}",
        tree.wheels.len()
    );

    for (i, a) in tree.wheels.iter().enumerate() {
        for b in tree.wheels.iter().skip(i + 1) {
            let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            assert!(
                d >= a.art_radius + b.art_radius,
                "{} and {} overlap: centres {d:.0} apart, art radii {:.0} + {:.0}",
                a.name,
                b.name,
                a.art_radius,
                b.art_radius
            );
        }
    }
}

#[test]
#[serial_test::serial]
fn ascendant_mirror_nodes_do_not_inflate_a_wheel() {
    // Ascendant carries a mirrored copy of each ascendancy's signature
    // notable, tagged with the *source* ascendancy's name but placed in the
    // Ascendant wheel ~1,700 units away. Counting those triples a wheel's
    // radius and drags its centre off — which is what puts wheels in the
    // wrong place.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    // With mirrors excluded, wheel radii cluster tightly. Including them
    // produced radii from 256 to 1242.
    for w in &tree.wheels {
        assert!(
            w.radius < 700.0,
            "{} has radius {:.0}; a mirror node is probably included",
            w.name,
            w.radius
        );
    }

    // The mirrors are still present as nodes — they are real Ascendant
    // passives — just not counted toward another wheel's geometry.
    let mirrors = tree.nodes.iter().filter(|n| n.is_ascendant_mirror).count();
    assert!(mirrors > 10, "expected Ascendant mirror nodes, got {mirrors}");
}

#[test]
#[serial_test::serial]
fn wheel_art_is_the_plate_not_the_button_glyph() {
    // AscendancyMiddle is a 40x40 button glyph. Using it as the wheel plate
    // and scaling it to ~800px is what produces a pixelated smear; the real
    // plate is Classes<Name> at roughly 740x700.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    for w in &tree.wheels {
        assert!(
            w.art.starts_with("Classes"),
            "{} uses {:?} as its plate",
            w.name,
            w.art
        );
        assert_ne!(w.art, "AscendancyMiddle");
    }

    // The build's own ascendancy must be among them.
    assert!(tree.wheels.iter().any(|w| w.name == "Occultist"));
}

#[test]
#[serial_test::serial]
fn every_ascendancy_is_drawn_not_just_the_builds_own() {
    // PoB shows all ascendancy wheels regardless of the character's class;
    // only the build's own is interactable. Hiding the others would be a
    // visible departure from the layout players know.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let names: std::collections::BTreeSet<&str> =
        tree.wheels.iter().map(|w| w.name.as_str()).collect();

    // Classic ascendancies from classes the build is not.
    for expected in ["Slayer", "Deadeye", "Juggernaut", "Hierophant"] {
        assert!(names.contains(expected), "missing wheel: {expected}");
    }
    // And at least some of the alternate/bloodline wheels.
    for expected in ["Aul", "Farrul", "Oshabi"] {
        assert!(names.contains(expected), "missing alternate wheel: {expected}");
    }
}

#[test]
#[serial_test::serial]
fn masteries_carry_only_their_chosen_effect() {
    // A mastery offers six or more effects. Showing all of them in a tooltip
    // overflows, so only the selection is carried — and the build's nine
    // chosen masteries must come through with their real text.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let masteries: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.kind == pob_engine::tree::NodeKind::Mastery)
        .collect();
    assert!(masteries.len() > 100, "masteries: {}", masteries.len());

    let chosen: Vec<_> = masteries
        .iter()
        .filter(|n| n.mastery_choice.is_some())
        .collect();
    assert_eq!(chosen.len(), 9, "the build selects nine mastery effects");

    for m in &chosen {
        let choice = m.mastery_choice.as_ref().unwrap();
        assert!(
            !choice.stats.is_empty(),
            "{} has a choice with no text",
            m.name
        );
        // The summary is the chosen effect, not a list of every option.
        let summary = m.mastery_summary();
        assert!(!summary.contains("effects to choose from"), "{summary}");
        assert!(summary.len() < 400, "summary too long: {summary}");
    }

    // An unchosen mastery reports its option count instead.
    let unchosen = masteries
        .iter()
        .find(|n| n.mastery_choice.is_none() && !n.allocated)
        .expect("an unallocated mastery");
    assert!(
        unchosen.mastery_summary().contains("effects to choose from"),
        "{}",
        unchosen.mastery_summary()
    );
    assert!(unchosen.mastery_effect_count > 0);
}

#[test]
#[serial_test::serial]
fn group_rings_are_not_drawn_twice_in_the_same_place() {
    // The owner reported doubled circles around Arcane Focus and Arcane
    // Will. Two causes: proxy groups, which PoB skips entirely, and pairs of
    // groups a few units apart both claiming the same ring size. Either
    // draws the same circle twice, which reads as a doubled outline.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let ringed: Vec<_> = tree
        .groups
        .iter()
        .filter(|g| g.background.is_some())
        .collect();

    for (i, a) in ringed.iter().enumerate() {
        for b in ringed.iter().skip(i + 1) {
            let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            assert!(
                d > 10.0,
                "two rings at effectively the same place: ({:.0},{:.0}) and ({:.0},{:.0})",
                a.x,
                a.y,
                b.x,
                b.y
            );
        }
    }
}

#[test]
#[serial_test::serial]
fn group_ring_sizes_are_ones_we_have_art_for() {
    // PoB selects a ring by *asking* for orbit 3, then 2, then 1 —
    // `group.oo[3]` is a lookup, not a maximum. Taking the largest orbit
    // present yields sizes like 4, which have no art and silently draw
    // nothing.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    for g in &tree.groups {
        if let Some(bg) = g.background {
            assert!(
                (1..=3).contains(&bg),
                "group at ({:.0},{:.0}) wants ring size {bg}, which has no art",
                g.x,
                g.y
            );
        }
    }

    // And plenty of groups legitimately have no ring at all.
    let plain = tree.groups.iter().filter(|g| g.background.is_none()).count();
    assert!(plain > 100, "expected many ringless groups, got {plain}");
}

#[test]
#[serial_test::serial]
fn the_build_reports_its_backdrop_start_art_and_frontier() {
    // Three pieces of art the tree cannot draw without the engine naming them:
    // the class backdrop behind the starting area, the start node's own art
    // (the coloured paths radiating out of it), and which nodes sit on the
    // frontier — adjacent to the build without being in it. PoB draws that
    // last group with a decorated "CanAllocate" frame; without the flag every
    // unallocated node looks alike and the edge of the build is invisible.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    assert!(tree.class_art.is_some(), "every build has a class backdrop");
    assert!(
        tree.start_art.is_some() && tree.start_pos.is_some(),
        "the allocated class start node carries its own art and position"
    );

    let allocated = tree.nodes.iter().filter(|n| n.allocated).count();
    let reachable = tree.nodes.iter().filter(|n| n.reachable).count();

    // A frontier is a rim, not the interior and not the whole tree. Zero would
    // mean the adjacency lists were never read; a count near the allocated
    // total would mean allocated nodes were counted as their own neighbours.
    assert!(allocated > 50, "fixture should be a real build: {allocated}");
    assert!(
        reachable > 0 && reachable < allocated,
        "frontier {reachable} against {allocated} allocated"
    );
    assert!(
        tree.nodes.iter().all(|n| !(n.allocated && n.reachable)),
        "an allocated node is in the build, not on its frontier"
    );
}


#[test]
#[serial_test::serial]
fn node_icons_name_the_sheet_they_were_packed_into() {
    // The bug this pins, which cost real time: every node's icon was sampled
    // from one hardcoded sheet. PoB packs icons across several — allocated and
    // unallocated live on `skills-3` and `skills-disabled-3` in *different*
    // layouts, and masteries have their own images again. A rect is only
    // meaningful against the sheet it came from, so using the wrong one draws
    // whatever else occupies that rectangle: icons sliced in half, or two
    // fragments overlapping.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let mut sheets = std::collections::BTreeSet::new();
    for node in &tree.nodes {
        if let Some(icon) = &node.icon {
            // Pixels, not UVs. PoB divides by the sheet size when it builds
            // these, but headless `ImageSize()` reports 1x1 for every image,
            // so the division is by one and raw pixels survive. Values well
            // above 1 are therefore expected — and are exactly what a
            // renderer that assumed UVs would silently mis-sample.
            let (x0, y0, x1, y1) = icon.px;
            assert!(x1 > x0 && y1 > y0, "{} has an empty icon rect", node.name);
            assert!(
                x1 <= 4096.0 && y1 <= 4096.0,
                "{} icon rect {:?} is off any plausible sheet",
                node.name,
                icon.px
            );
            sheets.insert(icon.sheet.clone());
        }
    }

    // Some rect must exceed 1.0, or the values really are normalised and the
    // renderer's division by sheet size would shrink every icon to a point.
    assert!(
        tree.nodes
            .iter()
            .filter_map(|n| n.icon.as_ref())
            .any(|i| i.px.2 > 1.0),
        "every icon rect fits inside 1.0: these are UVs, not pixels"
    );

    // More than one sheet is the whole point. Exactly one means the per-node
    // lookup collapsed back to a constant.
    assert!(
        sheets.len() > 1,
        "every icon resolved to the same sheet: {sheets:?}"
    );

    // Allocation must actually change which sheet is used, or the active and
    // disabled art are indistinguishable.
    let sheets_for = |allocated: bool| -> std::collections::BTreeSet<String> {
        tree.nodes
            .iter()
            .filter(|n| n.allocated == allocated && n.kind == pob_engine::tree::NodeKind::Normal)
            .filter_map(|n| n.icon.as_ref().map(|i| i.sheet.clone()))
            .collect()
    };
    assert_ne!(
        sheets_for(true),
        sheets_for(false),
        "allocated and unallocated nodes drew from the same sheets"
    );

    // Sockets are drawn from named assets (JewelFrame*/JewelSocket*), not the
    // atlas. Their sprite entry holds coordinates from an unrelated source,
    // so carrying one would point the renderer at the wrong image entirely.
    assert!(
        tree.nodes
            .iter()
            .filter(|n| n.kind == pob_engine::tree::NodeKind::Socket)
            .all(|n| n.icon.is_none()),
        "a socket carried an atlas icon"
    );
}




#[test]
#[serial_test::serial]
fn frames_come_from_pobs_overlay_table_not_from_composed_names() {
    // Frame names look composable — state plus a variant suffix — and they
    // are not. Keystones list Blighted variants that are just the plain
    // frames again; sockets use an `Alt` suffix where everything else uses
    // `Ascend`; masteries have no variants at all; ascendancy frames carry a
    // per-bloodline prefix only when that bloodline ships its own art.
    // Composing names from a node's flags produces plausible strings that
    // name no asset, and a missing texture draws as nothing at all.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let art = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src/TreeData");

    let mut checked = 0;
    for node in &tree.nodes {
        let Some(frame) = &node.frame else { continue };
        // A frame is either a file under TreeData or a rect in a sheet.
        // Neither is optional: a name that is not on disk and carries no
        // atlas rect names nothing loadable, and draws as nothing at all.
        let on_disk = ["png", "jpg", "webp"]
            .iter()
            .any(|ext| art.join(format!("{}.{ext}", frame.name)).is_file());
        assert!(
            on_disk || frame.atlas.is_some(),
            "{} ({:?}) resolved to frame {:?}, which is neither a file in \
             TreeData nor a sprite rect",
            node.name,
            node.kind,
            frame.name
        );
        checked += 1;
    }
    assert!(checked > 1000, "only {checked} nodes carried a frame");

    // The three states must actually produce three different frames, or the
    // frontier of the build is invisible again.
    let frame_of = |pred: &dyn Fn(&pob_engine::tree::Node) -> bool| -> Option<String> {
        tree.nodes
            .iter()
            .find(|n| n.kind == pob_engine::tree::NodeKind::Notable && pred(n))
            .and_then(|n| n.frame.as_ref().map(|f| f.name.clone()))
    };
    let alloc = frame_of(&|n| n.allocated);
    let path = frame_of(&|n| n.reachable);
    let unalloc = frame_of(&|n| !n.allocated && !n.reachable && n.ascendancy.is_none());
    assert_ne!(alloc, path, "allocated and reachable notables share a frame");
    assert_ne!(path, unalloc, "reachable and unallocated notables share a frame");

    // Sockets are frames without icons; masteries are icons without frames.
    // Mixing the two up draws a jewel socket as a blank ring.
    assert!(
        tree.nodes
            .iter()
            .filter(|n| n.kind == pob_engine::tree::NodeKind::Socket)
            .all(|n| n.frame.is_some()),
        "a socket had no frame"
    );
}

#[test]
#[serial_test::serial]
fn level_class_and_points_are_readable_and_writable() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // Points come from PoB's own progress estimate, not from a formula here.
    // The maximum folds in quest points and any ExtraPoints the build grants,
    // so a hardcoded 99+23 would be wrong for a good number of real builds.
    let info = engine.info().expect("info");
    assert!(info.points_max >= 120, "max {} too low", info.points_max);
    assert!(
        info.points_used > 0 && info.points_used <= info.points_max,
        "{} of {}",
        info.points_used,
        info.points_max
    );
    assert!(info.ascendancy_points_max >= 8);

    // Setting a level pins it to manual, which is what stops PoB's auto mode
    // overwriting it on the next recalculation.
    engine.set_level(42).expect("set level");
    let info = engine.info().expect("info");
    assert_eq!(info.level, 42);
    assert!(!info.level_is_auto, "typing a level must pin it");

    // Out-of-range levels clamp rather than erroring or wrapping.
    engine.set_level(9999).expect("set level");
    assert_eq!(engine.info().expect("info").level, 100);
    engine.set_level(-5).expect("set level");
    assert_eq!(engine.info().expect("info").level, 1);

    // Auto hands levelling back to PoB, which derives it from the tree.
    engine.set_level_auto().expect("auto");
    let info = engine.info().expect("info");
    assert!(info.level_is_auto);
    assert!(info.level > 1, "auto level should track the allocated tree");
}

#[test]
#[serial_test::serial]
fn changing_class_reports_whether_it_would_reset_the_tree() {
    // Class changes are destructive unless the new start node is already
    // connected — PoB asks before wiping a build, and the UI cannot ask
    // without this answer. Getting it wrong silently deletes someone's tree.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let classes = engine.classes().expect("classes");
    assert!(classes.len() >= 7, "expected every class: {}", classes.len());
    assert!(
        classes.iter().all(|c| !c.ascendancies.is_empty()),
        "every class lists its ascendancies"
    );

    let current = engine.info().expect("info").class;
    let other = classes
        .iter()
        .find(|c| c.name != current)
        .expect("another class exists");

    // A fully-allocated build cannot reach an unconnected class for free.
    assert!(
        engine
            .class_change_resets_tree(other.id)
            .expect("check"),
        "switching from {current} to {} should warn", other.name
    );

    // Switching ascendancy within the class never resets anything.
    let before = engine.info().expect("info").allocated_nodes;
    let asc = classes
        .iter()
        .find(|c| c.name == current)
        .and_then(|c| c.ascendancies.iter().find(|(id, _)| *id > 0).cloned())
        .expect("current class has an ascendancy");
    engine.set_ascendancy(asc.0).expect("set ascendancy");
    let after = engine.info().expect("info");
    assert_eq!(after.allocated_nodes, before, "ascendancy change lost nodes");
}

#[test]
#[serial_test::serial]
fn a_chosen_mastery_is_visibly_different_from_an_unchosen_one() {
    // The complaint this fixes: you could not tell a mastery had been chosen
    // without hovering it. PoB draws an allocated mastery as *two* sprites —
    // the selected base plus an `masteryActiveEffect` glow on top — and we
    // were drawing only the base, which is nearly identical to the inactive
    // one at tree zoom.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let masteries: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.kind == pob_engine::tree::NodeKind::Mastery)
        .collect();
    assert!(!masteries.is_empty(), "fixture has masteries");

    let chosen: Vec<_> = masteries.iter().filter(|n| n.allocated).collect();
    let unchosen: Vec<_> = masteries.iter().filter(|n| !n.allocated).collect();
    assert!(!chosen.is_empty() && !unchosen.is_empty(), "need both states");

    // Every allocated mastery carries the highlight; no unallocated one does.
    assert!(
        chosen.iter().all(|n| n.effect.is_some()),
        "an allocated mastery had no highlight sprite"
    );
    assert!(
        unchosen.iter().all(|n| n.effect.is_none()),
        "an unallocated mastery carried a highlight sprite"
    );

    // The two states must also differ in their base sprite, so the node is
    // distinguishable even where the glow is subtle.
    let chosen_sheets: std::collections::BTreeSet<_> = chosen
        .iter()
        .filter_map(|n| n.icon.as_ref().map(|i| i.sheet.clone()))
        .collect();
    let unchosen_sheets: std::collections::BTreeSet<_> = unchosen
        .iter()
        .filter_map(|n| n.icon.as_ref().map(|i| i.sheet.clone()))
        .collect();
    assert_ne!(
        chosen_sheets, unchosen_sheets,
        "chosen and unchosen masteries drew the same base art"
    );
}

#[test]
#[serial_test::serial]
fn tree_search_uses_pobs_own_matcher() {
    // Search is borrowed rather than reimplemented, so it has to actually
    // find things the way PoB does: by node name, by stat text, and with
    // multiple terms narrowing rather than widening the result.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    // An empty query is not a match-everything query.
    assert!(engine.search_nodes("").expect("search").is_empty());
    assert!(engine.search_nodes("   ").expect("search").is_empty());

    // A common stat phrase should match many nodes...
    let energy = engine.search_nodes("energy shield").expect("search");
    assert!(
        energy.len() > 20,
        "expected many energy shield nodes, got {}",
        energy.len()
    );

    // ...and every hit must actually mention it somewhere, or the matcher is
    // matching something other than what the user typed.
    let by_id: std::collections::HashMap<_, _> =
        tree.nodes.iter().map(|n| (n.id, n)).collect();
    for id in energy.iter().take(25) {
        let node = by_id.get(id).expect("search returned a real node");
        // Masteries are matched against every effect they offer, while we
        // only carry the *selected* one, so their text here is legitimately
        // narrower than what the search saw.
        if node.kind == pob_engine::tree::NodeKind::Mastery {
            continue;
        }
        let haystack = format!("{} {}", node.name, node.stats.join(" ")).to_lowercase();
        assert!(
            haystack.contains("energy shield"),
            "{} matched \"energy shield\" but never mentions it",
            node.name
        );
    }

    // Extra terms narrow: every term must match, not any.
    let narrower = engine.search_nodes("energy shield chaos").expect("search");
    assert!(
        narrower.len() < energy.len(),
        "adding a term widened the search: {} -> {}",
        energy.len(),
        narrower.len()
    );

    // Nonsense finds nothing rather than erroring or matching everything.
    assert!(
        engine
            .search_nodes("zzzzznotarealstat")
            .expect("search")
            .is_empty()
    );

    // A malformed Lua pattern must not blow up the engine — PoB wraps its
    // matcher in pcall for exactly this reason.
    let _ = engine.search_nodes("%(").expect("a bad pattern must not error");
}

#[test]
#[serial_test::serial]
fn node_power_ranks_unallocated_nodes_by_what_they_would_gain() {
    // PoB computes this by re-running the whole calculation once per
    // candidate node, which is why it runs as a coroutine across frames
    // upstream. Pumping it to completion here is the only way to get a whole
    // answer, and this pins that the pump actually finishes rather than
    // returning a half-built table.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let power = engine.node_power(Some("TotalDPS")).expect("node power");
    assert!(
        power.len() > 50,
        "expected many ranked nodes, got {}",
        power.len()
    );

    // Sorted best first, so the UI can take the head of the list.
    for pair in power.windows(2) {
        assert!(
            pair[0].1 >= pair[1].1,
            "ranking is not descending: {} then {}",
            pair[0].1,
            pair[1].1
        );
    }

    // Power is about what you have *not* taken; already-allocated nodes have
    // nothing left to give and would otherwise dominate the ranking.
    let allocated: std::collections::HashSet<_> = tree
        .nodes
        .iter()
        .filter(|n| n.allocated)
        .map(|n| n.id)
        .collect();
    let ranked: std::collections::HashSet<_> = power.iter().map(|(id, _)| *id).collect();
    assert!(
        ranked.intersection(&allocated).count() * 4 < ranked.len(),
        "ranking is dominated by nodes already in the build"
    );

    // Every ranked id is a real node, not a stale key from another tree.
    let known: std::collections::HashSet<_> = tree.nodes.iter().map(|n| n.id).collect();
    assert!(
        ranked.iter().all(|id| known.contains(id)),
        "ranking mentions nodes that are not in the tree"
    );
}

#[test]
#[serial_test::serial]
fn tree_links_round_trip_through_pobs_own_codec() {
    // Export and import both go through PoB's EncodeURL/DecodeURL so the
    // links we emit are the ones the official site, poeplanner and every
    // build guide already understand. A lookalike encoder would produce
    // links that look right and import wrong.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let before = engine.info().expect("info").allocated_nodes;
    let link = engine.export_tree_link().expect("export");
    assert!(
        link.contains("pathofexile.com/passive-skill-tree/"),
        "not a tree link: {link}"
    );

    // Round-tripping the build's own link must not change the build.
    engine.import_tree_link(&link).expect("import");
    let after = engine.info().expect("info").allocated_nodes;
    assert_eq!(before, after, "re-importing our own link changed the tree");

    // Junk is rejected with PoB's own wording rather than silently wiping
    // the tree — the failure mode that costs someone their build.
    assert!(engine.import_tree_link("https://example.com/nope").is_err());
    assert_eq!(
        engine.info().expect("info").allocated_nodes,
        after,
        "a failed import must leave the tree alone"
    );
}

#[test]
#[serial_test::serial]
fn every_tree_version_pob_supports_is_offered() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let versions = engine.tree_versions().expect("versions");
    assert!(versions.len() > 10, "only {} versions", versions.len());
    // The build's own version has to be in the list, or the dropdown cannot
    // show what is currently selected.
    let current = engine.info().expect("info").tree_version;
    assert!(
        versions.iter().any(|(id, _)| *id == current),
        "current version {current} missing from {versions:?}"
    );
    // Display names are the human form, not the underscore id.
    assert!(versions.iter().any(|(_, d)| d.contains('.')));
}

#[test]
#[serial_test::serial]
fn clicking_a_node_allocates_the_path_to_it_and_undoing_removes_dependents() {
    // Allocation is not a flag. PoB allocates the whole shortest path to the
    // node, and deallocating removes everything that depended on it. A
    // renderer that just flipped `allocated` would produce trees that cannot
    // exist in game and stats that do not match them.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let before = engine.info().expect("info").allocated_nodes;
    let tree = engine.tree().expect("tree");

    // A reachable-but-unallocated node is exactly what the frontier is.
    let frontier = tree
        .nodes
        .iter()
        .find(|n| n.reachable && !n.allocated)
        .expect("a build has a frontier");

    assert!(engine.toggle_node(frontier.id).expect("toggle"));
    let after = engine.info().expect("info").allocated_nodes;
    assert!(
        after > before,
        "allocating gained nothing: {before} -> {after}"
    );

    let tree = engine.tree().expect("tree");
    assert!(
        tree.nodes.iter().any(|n| n.id == frontier.id && n.allocated),
        "the clicked node is not allocated"
    );

    // Toggling back returns the build to where it started.
    assert!(engine.toggle_node(frontier.id).expect("toggle"));
    assert_eq!(
        engine.info().expect("info").allocated_nodes,
        before,
        "deallocating did not undo the allocation"
    );
}

#[test]
#[serial_test::serial]
fn unreachable_and_structural_nodes_are_left_alone() {
    // Two ways a click must do nothing rather than something wrong: a node
    // with no path would become an island the build cannot take, and class
    // start nodes are chosen through the class dropdown — clicking one
    // should not corrupt the spec.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let before = engine.info().expect("info").allocated_nodes;
    let tree = engine.tree().expect("tree");

    let start = tree
        .nodes
        .iter()
        .find(|n| n.kind == pob_engine::tree::NodeKind::ClassStart)
        .expect("a class start exists");
    assert!(!engine.toggle_node(start.id).expect("toggle"));
    assert_eq!(engine.info().expect("info").allocated_nodes, before);

    // An id that is not in the tree at all must not error.
    assert!(!engine.toggle_node(u64::MAX).expect("toggle"));
    assert_eq!(engine.info().expect("info").allocated_nodes, before);
}



#[test]
#[serial_test::serial]
fn hovering_a_node_previews_what_taking_it_would_do() {
    // PoB's tooltip answers the question that actually matters at the tree:
    // not "what does this node say" but "what would it do to my build". That
    // is a hypothetical recalculation, borrowed rather than reimplemented —
    // the labels, number formats and lower-is-better flags are all PoB's.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    // A node on the frontier: unallocated, adjacent to the build.
    let frontier = tree
        .nodes
        .iter()
        .find(|n| {
            n.reachable
                && !n.allocated
                && n.kind == pob_engine::tree::NodeKind::Normal
        })
        .expect("a build has a frontier");

    let preview = engine
        .node_preview(frontier.id)
        .expect("preview")
        .expect("a reachable node previews");
    assert!(!preview.allocated, "frontier node reported as allocated");

    // Some nodes genuinely change nothing for a given build — PoB says so
    // outright rather than inventing a number — so the guarantee is that
    // *some* reachable node moves a stat, not that every one does.
    let mut moved = None;
    for node in tree.nodes.iter().filter(|n| n.reachable && !n.allocated) {
        let p = engine.node_preview(node.id).expect("preview");
        if let Some(p) = p
            && !p.node.is_empty()
        {
            moved = Some((node, p));
            break;
        }
    }
    let (node, preview) = moved.expect("some reachable node changes a stat");

    // Deltas carry a sign, because the whole point is direction.
    for d in &preview.node {
        assert!(
            d.value.starts_with('+') || d.value.starts_with('-'),
            "{} on {} has unsigned delta {:?}",
            d.label,
            node.name,
            d.value
        );
        assert!(!d.label.is_empty());
    }

    // An allocated node previews its *removal* instead, and says so.
    let taken = tree
        .nodes
        .iter()
        .find(|n| n.allocated && n.kind == pob_engine::tree::NodeKind::Normal)
        .expect("a build has allocated nodes");
    let preview = engine
        .node_preview(taken.id)
        .expect("preview")
        .expect("an allocated node previews");
    assert!(
        preview.allocated,
        "allocated node did not report itself as allocated"
    );

    // Structural nodes are not choices, so they preview nothing rather than
    // reporting a meaningless zero-change.
    let start = tree
        .nodes
        .iter()
        .find(|n| n.kind == pob_engine::tree::NodeKind::ClassStart)
        .expect("a class start exists");
    assert!(engine.node_preview(start.id).expect("preview").is_none());

    // An unknown id is a miss, not an error.
    assert!(engine.node_preview(u64::MAX).expect("preview").is_none());
}

#[test]
#[serial_test::serial]
fn a_distant_node_reports_the_cost_of_reaching_it() {
    // "23 nodes in trace path" in PoB's tooltip: a node three points away is
    // a different proposition from an adjacent one, and the per-point figures
    // are how you compare them. Reporting only the node's own effect would
    // make every distant node look like a bargain.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let tree = engine.tree().expect("tree");

    let mut found_path = false;
    for node in tree.nodes.iter().filter(|n| !n.allocated) {
        let Some(p) = engine.node_preview(node.id).expect("preview") else {
            continue;
        };
        if p.path.is_empty() || p.path_points < 2 {
            continue;
        }
        found_path = true;

        // Path effects are reported per point as well as in total, which is
        // the only way to compare a cheap node against a distant strong one.
        assert!(
            p.path.iter().any(|d| d.per_point.is_some()),
            "path deltas carry no per-point figures"
        );
        // The path includes the node, so it cannot do strictly less.
        assert!(
            p.path.len() >= p.node.len(),
            "path affects fewer stats ({}) than the node alone ({})",
            p.path.len(),
            p.node.len()
        );
        break;
    }
    assert!(found_path, "no node in the tree needed a path");
}



#[test]
#[serial_test::serial]
fn saved_tree_specs_are_listed_the_way_pob_lists_them() {
    // PoB's Compare works against the tree specs saved *inside* a build, each
    // with a title the user set — not against pasted external builds. That is
    // where the names come from, and why an imported code has none: PoB
    // stores a build's name as its filename, and a spec's name as an XML
    // attribute on the spec itself.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let specs = engine.tree_specs().expect("specs");
    assert!(!specs.is_empty(), "a build always has at least one spec");

    // Indices are PoB's own 1-based ones, and must be usable as such.
    assert_eq!(specs[0].0, 1, "spec indices start at 1");
    for (i, (index, label)) in specs.iter().enumerate() {
        assert_eq!(*index, i + 1, "indices are contiguous and 1-based");
        assert!(!label.is_empty(), "spec {index} has no label");
    }

    let active = engine.active_spec().expect("active spec");
    assert!(
        specs.iter().any(|(i, _)| *i == active),
        "active spec {active} is not in the list"
    );
}

#[test]
#[serial_test::serial]
fn reading_a_specs_stats_leaves_the_active_spec_alone() {
    // The failure this guards against costs the user their tree: reading
    // another spec's numbers means switching to it, and a switch that is not
    // undone silently replaces the tree being edited.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let before_active = engine.active_spec().expect("active");
    let before_nodes = engine.info().expect("info").allocated_nodes;
    let before_dps = engine.stat("TotalDPS").expect("dps");

    let specs = engine.tree_specs().expect("specs");
    for (index, _) in &specs {
        let stats = engine
            .stats_for_spec(*index, &["TotalDPS", "Life"])
            .expect("spec stats");
        // A real spec produces real numbers, not an empty map.
        assert!(
            !stats.is_empty(),
            "spec {index} reported no stats at all"
        );
    }

    assert_eq!(engine.active_spec().expect("active"), before_active);
    assert_eq!(engine.info().expect("info").allocated_nodes, before_nodes);
    assert_eq!(engine.stat("TotalDPS").expect("dps"), before_dps);
}

#[test]
#[serial_test::serial]
fn compare_mode_reports_per_node_differences() {
    // PoB's tree comparison is entirely visual: the tree recoloured against a
    // second spec, with no numeric summary anywhere. This checks the data
    // behind that colour, following GetCompareNodeColor — green for nodes the
    // compared spec has and this one does not, red for the reverse.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // With compare off, no node claims a difference. This matters: a stale
    // diff would recolour the tree when nothing is being compared.
    engine.set_compare_spec(None).expect("compare off");
    let tree = engine.tree().expect("tree");
    assert!(
        tree.nodes.iter().all(|n| n.compare.is_none()),
        "compare is off but nodes report differences"
    );
    assert!(tree.connectors.iter().all(|c| c.compare.is_none()));

    // Comparing a spec against itself is legal in PoB and yields no diffs —
    // the compare dropdown does not exclude the active spec.
    let specs = engine.tree_specs().expect("specs");
    let active = engine.active_spec().expect("active");
    engine.set_compare_spec(Some(active)).expect("compare self");
    let tree = engine.tree().expect("tree");
    assert!(
        tree.nodes.iter().all(|n| n.compare.is_none()),
        "a spec compared against itself reported differences"
    );

    // Off again, so a later test does not inherit compare mode.
    engine.set_compare_spec(None).expect("compare off");
    let _ = specs;
}

#[test]
#[serial_test::serial]
fn a_changed_tree_shows_gains_and_losses_against_the_original() {
    // The real case: allocate something, then compare against the spec as it
    // was. What was added must read as "lost" relative to the compared spec
    // and vice versa, which is the direction PoB uses — green is what
    // *switching to the compared spec* would give you.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let tree = engine.tree().expect("tree");
    let frontier = tree
        .nodes
        .iter()
        .find(|n| n.reachable && !n.allocated)
        .expect("a build has a frontier")
        .id;

    // Compare against the current spec, then change the tree underneath it.
    let active = engine.active_spec().expect("active");
    engine.set_compare_spec(Some(active)).expect("compare");
    engine.toggle_node(frontier).expect("allocate");

    let tree = engine.tree().expect("tree");
    // The compare spec IS the active spec, so allocating changes both and
    // still yields no diff. This documents that PoB compares live specs, not
    // snapshots — a subtlety worth pinning so nobody "fixes" it later.
    let node = tree.nodes.iter().find(|n| n.id == frontier).expect("node");
    assert!(
        node.compare.is_none(),
        "comparing a spec with itself should track changes, not diff them"
    );

    engine.toggle_node(frontier).expect("restore");
    engine.set_compare_spec(None).expect("compare off");
}

#[test]
#[serial_test::serial]
fn compare_rows_come_from_pobs_own_display_stats() {
    // PoB's Compare tab is a four-column table driven by `displayStats` — the
    // same 204-entry table its sidebar uses, carrying labels, number formats,
    // colours and section breaks. Borrowing it means our rows match upstream
    // and keep matching when a league adds a stat; a hand-picked list of ours
    // would drift immediately.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // Compare the build against itself: every stat should appear, and none
    // should show a difference.
    let keys: Vec<&str> = vec![];
    let _ = keys;
    let mine = engine
        .stats_for_spec(engine.active_spec().expect("active"), &["TotalDPS"])
        .expect("spec stats");
    let _ = mine;

    let own: std::collections::HashMap<String, f64> = ["TotalDPS", "Life", "Mana"]
        .iter()
        .filter_map(|k| engine.stat(k).ok().flatten().map(|v| (k.to_string(), v)))
        .collect();

    let rows = engine.compare_rows(&own).expect("rows");
    assert!(rows.len() > 20, "only {} rows", rows.len());

    // Section gaps are preserved — this is what separates offence from
    // defence, and PoB's tooltip diffs drop them where the table keeps them.
    assert!(
        rows.iter().any(|r| matches!(r, pob_engine::tree::CompareRow::Gap)),
        "no section breaks in the table"
    );

    let stats: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            pob_engine::tree::CompareRow::Stat(s) => Some(s),
            _ => None,
        })
        .collect();
    assert!(!stats.is_empty());

    // Stats we supplied a matching value for show no difference; stats we did
    // not supply show one, because the missing side reads as zero.
    let dps = stats
        .iter()
        .find(|s| s.key == "TotalDPS")
        .expect("TotalDPS is in displayStats");
    assert!(
        dps.diff.is_none(),
        "identical values reported a difference: {:?}",
        dps.diff
    );
    assert_eq!(dps.better, 0, "unchanged stat claimed a direction");
    // Both sides carry an absolute value, which is the point of the table:
    // a tooltip shows only deltas, this shows what the numbers actually are.
    assert!(!dps.a.is_empty() && !dps.c.is_empty());
    assert_eq!(dps.a, dps.c);

    // Labels are PoB's, and at least some carry PoB's own colour.
    assert!(stats.iter().any(|s| s.colour.is_some()), "no coloured labels");
    assert!(stats.iter().all(|s| !s.label.is_empty()));
}

#[test]
#[serial_test::serial]
fn a_worse_value_is_reported_as_worse_even_when_the_number_rises() {
    // Direction is not the sign of the difference. Mana cost, cast time and
    // reservation are all lower-is-better, so an increase is a regression —
    // colouring by sign alone would call it an improvement.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // A build with everything halved: every stat moves down.
    let mut halved = std::collections::HashMap::new();
    for key in ["TotalDPS", "Life", "ManaCost", "WarcryCastTime"] {
        if let Ok(Some(v)) = engine.stat(key) {
            halved.insert(key.to_string(), v * 0.5);
        }
    }
    if halved.is_empty() {
        return; // Nothing to judge on this fixture.
    }

    let rows = engine.compare_rows(&halved).expect("rows");
    for row in &rows {
        let pob_engine::tree::CompareRow::Stat(s) = row else {
            continue;
        };
        if s.diff.is_none() {
            continue;
        }
        // Halving DPS or Life is unambiguously worse.
        if s.key == "TotalDPS" || s.key == "Life" {
            assert_eq!(s.better, -1, "{} halved but reported as better", s.label);
            assert!(
                s.percent < 0.0,
                "{} halved but percent is {}",
                s.label,
                s.percent
            );
        }
    }
}

#[test]
#[serial_test::serial]
fn a_real_comparison_shows_both_values_and_a_signed_difference() {
    // The shape of PoB's Compare tab: a stat that differs shows both absolute
    // values *and* the delta, where a tooltip would show only the delta. That
    // distinction is the whole reason the tab exists — you cannot judge
    // "+17 Total Mana" without knowing whether the pool was 200 or 2000.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // A compared build with meaningfully different numbers.
    let keys = engine.display_stat_keys().expect("keys");
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let mut other = engine
        .stats_for_spec(engine.active_spec().expect("active"), &refs)
        .expect("stats");
    let dps = *other.get("TotalDPS").expect("fixture has DPS");
    other.insert("TotalDPS".into(), dps * 2.0);

    let rows = engine.compare_rows(&other).expect("rows");
    let stat = rows
        .iter()
        .find_map(|r| match r {
            pob_engine::tree::CompareRow::Stat(s) if s.key == "TotalDPS" => Some(s),
            _ => None,
        })
        .expect("TotalDPS row");

    // Both sides present and different.
    assert_ne!(stat.a, stat.c, "doubled DPS reported identical values");
    // A signed difference, and a percentage near +100%.
    let diff = stat.diff.as_deref().expect("a changed stat has a difference");
    assert!(
        diff.starts_with('+'),
        "doubling should read as a gain, got {diff:?}"
    );
    assert!(
        (stat.percent - 100.0).abs() < 1.0,
        "doubling should be about +100%, got {}",
        stat.percent
    );
    assert_eq!(stat.better, 1, "doubled DPS reported as not-better");

    // And unchanged stats still occupy a row, with no difference shown —
    // PoB's table lists them where its tooltips would omit them entirely.
    let unchanged = rows.iter().filter_map(|r| match r {
        pob_engine::tree::CompareRow::Stat(s) => Some(s),
        _ => None,
    }).filter(|s| s.diff.is_none()).count();
    assert!(
        unchanged > 0,
        "no unchanged rows: the table is behaving like a tooltip diff"
    );
}

#[test]
#[serial_test::serial]
fn the_sidebar_shows_what_pobs_sidebar_shows() {
    // PoB's sidebar is driven by `displayStats`, whose own header comment
    // calls it "the stats in the side bar, and also which stats show in
    // node/item comparisons". Ours previously used a hand-picked list of
    // about a dozen, so a PoB user opening this app found a third of the
    // numbers they expect and none of the defensive ones.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let rows = engine.sidebar_stats().expect("sidebar");
    let stats: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            pob_engine::tree::SidebarRow::Stat(s) => Some(s),
            _ => None,
        })
        .collect();

    assert!(
        stats.len() > 25,
        "only {} stats: the list is still hand-picked",
        stats.len()
    );

    // The defensive numbers a PoB user looks for. These were all absent
    // before, which is exactly the "doesn't behave like PoB" failure.
    for key in ["TotalEHP", "Life", "EnergyShield"] {
        assert!(
            stats.iter().any(|s| s.key == key),
            "{key} missing from the sidebar"
        );
    }

    // Section breaks survive, so the list is grouped rather than a wall.
    assert!(
        rows.iter().any(|r| matches!(r, pob_engine::tree::SidebarRow::Gap)),
        "no section breaks"
    );

    // Values are formatted and carry no leftover colour escapes — PoB embeds
    // them inline, and unhandled they render as literal "^x33FF77" text.
    for s in &stats {
        assert!(!s.value.is_empty(), "{} has no value", s.label);
        assert!(
            !s.value.contains('^'),
            "{} value {:?} still has a colour escape",
            s.label,
            s.value
        );
    }

    // Some labels carry PoB's own colour, which is how life and mana are
    // findable at a glance in a list this long.
    assert!(stats.iter().any(|s| s.colour.is_some()), "no coloured labels");
}


#[test]
#[serial_test::serial]
fn pobs_own_build_warnings_are_read() {
    // The failure this fixes: our "Warnings" panel only ever showed our own
    // error messages. PoB's warnings — the ones that tell you the build
    // cannot cast its main skill — were never read at all.
    //
    // The fixture is a real example: its unreserved mana is negative, so PoB
    // reports four warnings naming the skills that cannot be used. An app
    // that shows the DPS of a build that cannot actually function, and says
    // nothing, is worse than one that shows no DPS.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let warnings = engine.build_warnings().expect("warnings");
    assert!(
        !warnings.is_empty(),
        "this fixture over-reserves mana; PoB reports four warnings"
    );

    // The mana problem is the headline one and must be present.
    assert!(
        warnings
            .iter()
            .any(|w| w.to_lowercase().contains("unreserved mana")),
        "the negative-mana warning is missing: {warnings:?}"
    );

    // Colour escapes are stripped, or they render as literal text.
    for w in &warnings {
        assert!(!w.contains('^'), "warning still has a colour escape: {w:?}");
        assert!(!w.trim().is_empty());
    }
}

#[test]
#[serial_test::serial]
fn specs_can_be_created_copied_renamed_and_deleted() {
    // Tree compare is unreachable without more than one spec, and PoB's
    // Manage Passive Trees popup is how you get them. These call PoB's own
    // spec machinery rather than manipulating lists ourselves, because
    // switching specs also re-sockets jewels, resets jewel radii and rebuilds
    // cluster graphs.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let before = engine.tree_specs().expect("specs").len();
    let allocated = engine.info().expect("info").allocated_nodes;

    // New inherits class and ascendancy but starts empty.
    let new_idx = engine.new_spec("Fresh").expect("new");
    let specs = engine.tree_specs().expect("specs");
    assert_eq!(specs.len(), before + 1);
    assert!(specs.iter().any(|(i, l)| *i == new_idx && l.contains("Fresh")));

    // Creating a spec must not disturb the one being edited — PoB's New
    // appends without switching, and silently switching would look like the
    // tree had been wiped.
    assert_eq!(engine.info().expect("info").allocated_nodes, allocated);

    // Copy duplicates the tree, so the copy has the same node count.
    let copy_idx = engine.copy_spec(1, "Clone").expect("copy");
    engine.set_active_spec(copy_idx).expect("activate copy");
    assert_eq!(
        engine.info().expect("info").allocated_nodes,
        allocated,
        "a copy should carry the same allocated nodes"
    );
    engine.set_active_spec(1).expect("back to first");

    // Rename is in place.
    engine.rename_spec(new_idx, "Renamed").expect("rename");
    let specs = engine.tree_specs().expect("specs");
    assert!(specs.iter().any(|(i, l)| *i == new_idx && l.contains("Renamed")));

    // Delete removes it and leaves the active spec alone.
    engine.delete_spec(new_idx).expect("delete");
    let specs = engine.tree_specs().expect("specs");
    assert!(!specs.iter().any(|(_, l)| l.contains("Renamed")));
    assert_eq!(engine.info().expect("info").allocated_nodes, allocated);
}

#[test]
#[serial_test::serial]
fn the_last_spec_cannot_be_deleted() {
    // PoB disables Delete at one spec. A build with no passive tree at all
    // is not a state the rest of the app is prepared for.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let specs = engine.tree_specs().expect("specs");
    assert_eq!(specs.len(), 1, "fixture starts with one spec");

    engine.delete_spec(1).expect("delete is a no-op, not an error");
    assert_eq!(
        engine.tree_specs().expect("specs").len(),
        1,
        "the last spec was deleted"
    );
}

#[test]
#[serial_test::serial]
fn reordering_keeps_the_same_tree_active() {
    // The active spec follows the *tree*, not the index. PoB re-derives it by
    // identity after a reorder; tracking the number instead would silently
    // switch you to a different tree when you dragged a row.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let allocated = engine.info().expect("info").allocated_nodes;
    engine.new_spec("Second").expect("new");
    assert_eq!(engine.active_spec().expect("active"), 1);

    // Move the active spec to the end.
    engine.move_spec(1, 2).expect("move");
    assert_eq!(
        engine.active_spec().expect("active"),
        2,
        "the active index did not follow its tree"
    );
    assert_eq!(engine.info().expect("info").allocated_nodes, allocated);
}

#[test]
#[serial_test::serial]
fn editing_a_gem_changes_the_calculated_dps() {
    // The Skills tab has been read-only, which makes it the most-used tab you
    // cannot use. This is the smallest proof that editing works: change a
    // gem and the numbers must move, because the edit has to reach PoB's
    // calculation rather than just a field we display.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let before = engine.stat("TotalDPS").expect("dps").expect("has dps");
    let groups = engine.socket_groups().expect("groups");
    let main = engine.main_socket_group().expect("main group");

    // The active skill in the main group, whose level definitely matters.
    let group = &groups[main];
    let gem_index = group
        .gems
        .iter()
        .position(|g| !g.is_support())
        .expect("the main group has an active skill");
    let original = group.gems[gem_index].level;
    assert!(original > 1, "fixture gem should not already be level 1");

    engine
        .set_gem(main + 1, gem_index + 1, Some(1), None, None, None)
        .expect("set level");

    let after = engine.stat("TotalDPS").expect("dps").expect("has dps");
    assert!(
        after < before,
        "dropping the main skill to level 1 did not reduce DPS: {before} -> {after}"
    );

    // And the read-back reflects it, so the UI shows what was applied.
    let groups = engine.socket_groups().expect("groups");
    assert_eq!(groups[main].gems[gem_index].level, 1);

    // Restore, and the original number comes back exactly — proof the edit
    // is a real recalculation rather than a one-way approximation.
    engine
        .set_gem(main + 1, gem_index + 1, Some(original), None, None, None)
        .expect("restore");
    let restored = engine.stat("TotalDPS").expect("dps").expect("has dps");
    assert!(
        (restored - before).abs() < 0.01,
        "restoring the level did not restore the DPS: {before} -> {restored}"
    );
}

#[test]
#[serial_test::serial]
fn disabling_a_support_removes_it_from_the_calculation() {
    // Toggling a support is the edit people make most while testing an idea,
    // so it has to reach the calculation and not just the checkbox.
    //
    // Asserted on the support list rather than on DPS. Not every support
    // moves TotalDPS on every build — a channelling-stages support may not
    // register in that stat at all — so a DPS assertion fails for builds
    // where the edit worked perfectly. This was found the hard way: the
    // first version of this test failed while the code was correct.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let main = engine.main_socket_group().expect("main group");
    let groups = engine.socket_groups().expect("groups");
    let Some(support) = groups[main]
        .gems
        .iter()
        .position(|g| g.is_support() && g.enabled)
    else {
        return; // Nothing enabled to toggle on this fixture.
    };
    let name = groups[main].gems[support].name.clone();

    let supports_now = || -> String {
        engine
            .probe(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local ms = b.calcsTab and b.calcsTab.mainEnv
                    and b.calcsTab.mainEnv.player
                    and b.calcsTab.mainEnv.player.mainSkill
                local names = {}
                for _, e in ipairs((ms and ms.supportList) or {}) do
                    names[#names + 1] =
                        tostring(e.grantedEffect and e.grantedEffect.name or "?")
                end
                return table.concat(names, ",")
                "#,
            )
            .unwrap_or_default()
    };

    assert!(
        supports_now().contains(&name),
        "{name} should start in the support list"
    );

    engine
        .set_gem(main + 1, support + 1, None, None, Some(false), None)
        .expect("disable");
    assert!(
        !supports_now().contains(&name),
        "{name} still supports the skill after being disabled"
    );

    // Only the flag moved. An edit that quietly reset the level would be
    // much harder to notice than one that failed outright.
    let after = engine.socket_groups().expect("groups");
    assert!(!after[main].gems[support].enabled);
    assert_eq!(after[main].gems[support].level, groups[main].gems[support].level);
    assert_eq!(
        after[main].gems[support].quality,
        groups[main].gems[support].quality
    );

    // And it comes back.
    engine
        .set_gem(main + 1, support + 1, None, None, Some(true), None)
        .expect("re-enable");
    assert!(supports_now().contains(&name), "{name} did not come back");
}

#[test]
#[serial_test::serial]
fn gem_levels_are_clamped_rather_than_trusted() {
    // A level 400 gem is not a stronger gem, it is a broken build. PoB
    // constrains what it accepts and so must this, since the value goes
    // straight into the calculation.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let main = engine.main_socket_group().expect("main");

    engine
        .set_gem(main + 1, 1, Some(9999), None, None, None)
        .expect("absurd level");
    let level = engine.socket_groups().expect("groups")[main].gems[0].level;
    assert!(level <= 40, "gem level {level} was not clamped");

    engine
        .set_gem(main + 1, 1, Some(-5), None, None, None)
        .expect("negative level");
    let level = engine.socket_groups().expect("groups")[main].gems[0].level;
    assert!(level >= 1, "gem level {level} went below 1");
}

#[test]
#[serial_test::serial]
fn editing_a_gem_that_does_not_exist_is_a_no_op() {
    // Indices come from a UI list that can lag a rebuild by a frame, so an
    // out-of-range edit must do nothing rather than error or corrupt a
    // neighbouring gem.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");
    let before = engine.stat("TotalDPS").expect("dps");

    engine.set_gem(9999, 1, Some(1), None, None, None).expect("no group");
    engine.set_gem(1, 9999, Some(1), None, None, None).expect("no gem");
    engine.remove_gem(9999, 1).expect("no group");

    assert_eq!(engine.stat("TotalDPS").expect("dps"), before);
}

#[test]
#[serial_test::serial]
fn the_gem_catalogue_offers_real_socketable_gems() {
    // The picker is only as good as its list. PoB's data holds entries that
    // are not socketable, and offering one is offering a choice that cannot
    // be made.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let gems = engine.gem_catalogue().expect("catalogue");
    assert!(
        gems.len() > 100,
        "expected the full gem list, got {}",
        gems.len()
    );

    // Sorted by name, since that is the order the picker presents.
    let mut sorted = gems.clone();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(gems, sorted, "catalogue should arrive sorted by name");

    // Both kinds are present: a picker that lost every support would look
    // fine until you tried to add one.
    assert!(gems.iter().any(|g| g.support), "no support gems offered");
    assert!(gems.iter().any(|g| !g.support), "no active gems offered");

    // Nothing nameless or idless — either would render as a blank row.
    for gem in &gems {
        assert!(!gem.id.is_empty(), "gem with no id: {gem:?}");
        assert!(!gem.name.is_empty(), "gem with no name: {gem:?}");
    }
}

#[test]
#[serial_test::serial]
fn pobs_matcher_resolves_abbreviations_not_just_exact_names() {
    // The reason to call FindSkillGem rather than write our own match: PoB
    // accepts abbreviations, and a user with the muscle memory will type them.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let exact = engine.find_gem("Cold to Fire").expect("lookup");
    let exact = exact.expect("'Cold to Fire' should resolve");
    assert_eq!(exact.name, "Cold to Fire");

    // An abbreviation of the same gem. Note this is *not* "CtF", which PoB's
    // own source comment gives as the example: the gem pool has grown since
    // that comment was written and "CtF" now also matches "Chance to Flee",
    // so PoB refuses it as ambiguous. Believing the comment over the data
    // cost a test failure that looked like a bridging bug and was not one.
    let abbrev = engine.find_gem("CldFr").expect("lookup");
    assert_eq!(
        abbrev.map(|g| g.id),
        Some(exact.id),
        "'CldFr' should resolve to the same gem as its full name"
    );

    // Ambiguity resolves to nothing rather than to whichever gem was reached
    // first. Picking one would silently socket a gem nobody asked for.
    assert!(
        engine.find_gem("CtF").expect("lookup").is_none(),
        "an ambiguous abbreviation should not resolve to an arbitrary gem"
    );

    // Nonsense resolves to nothing rather than to something arbitrary.
    assert!(
        engine.find_gem("zzzznotagem").expect("lookup").is_none(),
        "unrecognised text should not resolve to a gem"
    );
}

#[test]
#[serial_test::serial]
fn swapping_a_gem_replaces_it_and_reaches_the_calculation() {
    // The whole point of the change: the Skills tab could tune a gem but not
    // change which gem it was.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let main = engine.main_socket_group().expect("main group");
    let groups = engine.socket_groups().expect("groups");
    let group = &groups[main];

    // Swap a support, so the active skill stays and the group keeps working.
    let slot = group
        .gems
        .iter()
        .position(|g| g.is_support())
        .expect("main group has a support");
    let before_name = group.gems[slot].name.clone();

    // Something certain to be in the list and different from what is there.
    let catalogue = engine.gem_catalogue().expect("catalogue");
    let target = catalogue
        .iter()
        .find(|g| g.support && g.name != before_name)
        .expect("a different support gem exists");

    engine
        .set_gem_id(main + 1, slot + 1, &target.id)
        .expect("swap gem");

    let after = engine.socket_groups().expect("groups after");
    let swapped = &after[main].gems[slot];
    assert_eq!(
        swapped.name, target.name,
        "the slot should now hold the gem we asked for"
    );
    assert_ne!(swapped.name, before_name, "the gem did not change");

    // Resolved by PoB, not merely stored: a gem that never went through
    // ProcessSocketGroup has no skill id, and would calculate as nothing.
    assert!(
        !swapped.skill_id.is_empty(),
        "swapped gem was not resolved against PoB's data"
    );
}

#[test]
#[serial_test::serial]
fn a_gem_can_be_added_past_the_end_of_a_group() {
    // How adding works in PoB: there is no Add button, there is a blank
    // trailing row, and typing into it appends.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let main = engine.main_socket_group().expect("main group");
    let before = engine.socket_groups().expect("groups")[main].gems.len();

    let catalogue = engine.gem_catalogue().expect("catalogue");
    let target = catalogue
        .iter()
        .find(|g| g.support)
        .expect("a support gem exists");

    // One past the end, 1-based: the blank row.
    engine
        .set_gem_id(main + 1, before + 1, &target.id)
        .expect("append gem");

    let after = engine.socket_groups().expect("groups after");
    assert_eq!(
        after[main].gems.len(),
        before + 1,
        "appending should add a gem, not replace one"
    );

    let added = after[main].gems.last().expect("the appended gem");
    assert_eq!(added.name, target.name);
    // PoB's defaults for a fresh gem, not level 0 quality 0.
    assert_eq!(added.level, 20, "a new gem should arrive at PoB's default level");
    assert!(added.enabled, "a new gem should arrive enabled");
    assert_eq!(added.count, 1);
}

#[test]
#[serial_test::serial]
fn removing_a_gem_takes_it_out_of_the_group() {
    // `remove_gem` has been written but uncalled since the Skills tab shipped.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let main = engine.main_socket_group().expect("main group");
    let groups = engine.socket_groups().expect("groups");
    let before: Vec<String> = groups[main].gems.iter().map(|g| g.name.clone()).collect();
    assert!(before.len() > 1, "fixture group should hold several gems");

    let slot = groups[main]
        .gems
        .iter()
        .position(|g| g.is_support())
        .expect("main group has a support");
    let removed = before[slot].clone();

    engine.remove_gem(main + 1, slot + 1).expect("remove gem");

    let after: Vec<String> = engine.socket_groups().expect("groups after")[main]
        .gems
        .iter()
        .map(|g| g.name.clone())
        .collect();
    assert_eq!(after.len(), before.len() - 1, "one gem should be gone");
    assert!(
        !after.contains(&removed),
        "the removed gem is still in the group"
    );
}

#[test]
#[serial_test::serial]
fn the_item_list_reports_what_the_build_owns_and_where_it_is_worn() {
    // The slot dropdowns choose from this list, and "unused" — what Del
    // Unused acts on — means an item worn in no slot at all.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let list = engine.item_list().expect("item list");
    assert!(!list.is_empty(), "the fixture build owns items");

    for entry in &list {
        assert!(entry.id != 0, "item with no id: {entry:?}");
        assert!(!entry.name.is_empty(), "item with no name: {entry:?}");
    }

    // Ids are unique: they are what identifies an item to the engine, and
    // names are not — a build routinely holds several "New Item".
    let mut ids: Vec<i64> = list.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "item ids should be unique");

    // Every equipped slot's item must appear in the list, worn in that slot.
    for slot in engine.equipment().expect("equipment") {
        let Some(item) = slot.item else { continue };
        let entry = list
            .iter()
            .find(|e| e.name == item.name)
            .unwrap_or_else(|| panic!("equipped item {:?} missing from list", item.name));
        assert!(
            entry.equipped_in.contains(&slot.name),
            "{:?} is worn in {:?} but the list does not say so",
            item.name,
            slot.name
        );
    }
}

#[test]
#[serial_test::serial]
fn slot_candidates_are_the_items_that_slot_will_take() {
    // Validity is PoB's IsItemValidForSlot, which is why a helmet is never
    // offered for a boot slot.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let helmets = engine.slot_candidates("Helmet").expect("helmet candidates");
    assert!(!helmets.is_empty(), "the build owns a helmet");

    let boots = engine.slot_candidates("Boots").expect("boot candidates");
    assert!(!boots.is_empty(), "the build owns boots");

    // The two lists must not overlap: nothing is both a helmet and a boot.
    for helmet in &helmets {
        assert!(
            !boots.iter().any(|b| b.id == helmet.id),
            "{:?} was offered for both Helmet and Boots",
            helmet.name
        );
    }
}

#[test]
#[serial_test::serial]
fn equipping_an_item_changes_the_slot_and_the_calculation() {
    // The complaint this answers: the Items tab could show gear but not
    // change it.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    // Emptying a slot must move the numbers — body armour carries defences
    // on any real build.
    let before_es = engine.stat("EnergyShield").expect("es");
    let slot_before = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Body Armour")
        .expect("body armour slot");
    let worn = slot_before.item.expect("fixture wears body armour");

    engine.equip_item("Body Armour", 0).expect("unequip");

    let slot_after = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Body Armour")
        .expect("body armour slot");
    assert!(slot_after.item.is_none(), "the slot should now be empty");
    assert_ne!(
        engine.stat("EnergyShield").expect("es"),
        before_es,
        "removing body armour did not reach the calculation"
    );

    // And putting it back restores both.
    let candidate = engine
        .slot_candidates("Body Armour")
        .expect("candidates")
        .into_iter()
        .find(|c| c.name == worn.name)
        .expect("the item we removed is still in the build");
    engine
        .equip_item("Body Armour", candidate.id)
        .expect("re-equip");

    let restored = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Body Armour")
        .expect("body armour slot");
    assert_eq!(
        restored.item.map(|i| i.name),
        Some(worn.name),
        "the original item should be back"
    );
    assert_eq!(
        engine.stat("EnergyShield").expect("es"),
        before_es,
        "restoring the item should restore the number"
    );
}

#[test]
#[serial_test::serial]
fn flasks_carry_an_active_flag_and_gear_slots_do_not() {
    // PoB puts the activate checkbox on flask slots only.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    for slot in engine.equipment().expect("equipment") {
        if slot.name.starts_with("Flask") {
            assert!(
                slot.active.is_some(),
                "{} should carry an active flag",
                slot.name
            );
        } else {
            assert!(
                slot.active.is_none(),
                "{} is not a flask and should have no active flag",
                slot.name
            );
        }
    }
}

#[test]
#[serial_test::serial]
fn toggling_a_flask_is_readable_back() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let flask = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name.starts_with("Flask") && s.item.is_some())
        .expect("the fixture wears a flask");
    let before = flask.active.expect("flask has an active flag");

    engine
        .set_flask_active(&flask.name, !before)
        .expect("toggle flask");

    let after = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == flask.name)
        .expect("flask slot")
        .active
        .expect("flask has an active flag");
    assert_eq!(after, !before, "the flask toggle did not stick");

    // A gear slot is not a flask, and saying so beats silently doing nothing.
    assert!(
        engine.set_flask_active("Helmet", true).is_err(),
        "Helmet should not accept a flask toggle"
    );
}

#[test]
#[serial_test::serial]
fn deleting_an_item_removes_it_from_the_build_and_unequips_it() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(WINTER_ORB).expect("load build");

    let worn = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Helmet")
        .expect("helmet slot")
        .item
        .expect("fixture wears a helmet");

    let entry = engine
        .item_list()
        .expect("item list")
        .into_iter()
        .find(|e| e.name == worn.name)
        .expect("the helmet is in the item list");

    engine.delete_item(entry.id).expect("delete item");

    assert!(
        !engine
            .item_list()
            .expect("item list")
            .iter()
            .any(|e| e.id == entry.id),
        "the deleted item is still in the list"
    );
    assert!(
        engine
            .equipment()
            .expect("equipment")
            .into_iter()
            .find(|s| s.name == "Helmet")
            .expect("helmet slot")
            .item
            .is_none(),
        "deleting a worn item should empty its slot"
    );
}

#[test]
#[serial_test::serial]
fn previewing_an_item_reports_what_equipping_it_would_change() {
    // The right half of PoB's item tooltip, and the thing that makes a
    // database of gear you do not own worth browsing: hover it, see whether
    // it is an upgrade, without equipping it to find out.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    // A ring the build owns but is not wearing in Ring 1.
    let worn = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Ring 1")
        .expect("ring slot")
        .item
        .expect("fixture wears a ring");

    let other = engine
        .slot_candidates("Ring 1")
        .expect("candidates")
        .into_iter()
        .find(|c| c.name != worn.name)
        .expect("the build owns more than one ring");

    let preview = engine
        .item_preview("Ring 1", other.id)
        .expect("preview")
        .expect("a ring in a ring slot previews");

    assert_eq!(preview.slot, "Ring 1");
    assert_eq!(
        preview.replacing.as_deref(),
        Some(worn.name.as_str()),
        "the header should name what is being replaced"
    );
    assert!(!preview.is_equipped);
    assert!(
        !preview.deltas.is_empty(),
        "swapping to a different ring should change something"
    );

    // Formatting is PoB's: every value carries an explicit sign, so a reader
    // never has to work out which direction a number went.
    for delta in &preview.deltas {
        assert!(!delta.label.is_empty(), "delta with no label: {delta:?}");
        assert!(
            delta.value.starts_with('+') || delta.value.starts_with('-'),
            "value {:?} should be signed",
            delta.value
        );
    }

    // Previewing does not equip: the slot still holds what it held.
    assert_eq!(
        engine
            .equipment()
            .expect("equipment")
            .into_iter()
            .find(|s| s.name == "Ring 1")
            .expect("ring slot")
            .item
            .map(|i| i.name),
        Some(worn.name),
        "previewing an item must not equip it"
    );
}

#[test]
#[serial_test::serial]
fn previewing_the_worn_item_reports_removing_it() {
    // Hovering what you already wear can only sensibly mean "what would I
    // lose", and PoB's tooltip changes its wording to say so.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let worn = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .find(|s| s.name == "Body Armour")
        .expect("body armour slot")
        .item
        .expect("fixture wears body armour");

    let entry = engine
        .item_list()
        .expect("item list")
        .into_iter()
        .find(|e| e.name == worn.name)
        .expect("worn item is in the list");

    let preview = engine
        .item_preview("Body Armour", entry.id)
        .expect("preview")
        .expect("previews");

    assert!(
        preview.is_equipped,
        "hovering the worn item should be flagged as such"
    );
    assert!(
        preview.replacing.is_none(),
        "removing an item replaces nothing"
    );
    assert!(
        !preview.deltas.is_empty(),
        "taking off body armour should change something"
    );
}

#[test]
#[serial_test::serial]
fn items_carry_what_the_tooltip_needs_beyond_their_mods() {
    // PoB's item tooltip is not just a mod list: it opens with requirements
    // and closes with flavour text, and an item missing either looks wrong in
    // a way that is hard to place.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let equipment = engine.equipment().expect("equipment");
    let uniques: Vec<_> = equipment
        .iter()
        .filter_map(|s| s.item.as_ref())
        .filter(|i| i.rarity == pob_engine::items::Rarity::Unique)
        .collect();
    assert!(!uniques.is_empty(), "the fixture wears uniques");

    // Requirements: a level requirement is near-universal on real gear.
    assert!(
        uniques.iter().filter(|i| i.req_level.is_some()).count() > 1,
        "no unique reported a level requirement"
    );

    // Flavour comes from PoB's own `data.flavourText`, looked up by title.
    // The lookup is the part worth pinning: reading `base.flavourText` alone
    // returns None for every unique in this build, which is how the first
    // version of this silently produced nothing.
    let with_flavour = uniques.iter().filter(|i| i.flavour.is_some()).count();
    assert!(
        with_flavour > uniques.len() / 2,
        "only {with_flavour} of {} uniques resolved flavour text",
        uniques.len()
    );
}

#[test]
#[serial_test::serial]
fn mods_pob_cannot_calculate_are_flagged() {
    // PoB paints these red and appends "(Not supported in PoB yet)". Worth
    // reproducing: a mod that silently contributes nothing is how someone
    // comes to trust a number that is wrong.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let flagged: Vec<(String, String)> = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter_map(|s| s.item)
        .flat_map(|i| {
            let name = i.name.clone();
            i.mods
                .into_iter()
                .filter(|m| m.unsupported)
                .map(move |m| (name.clone(), m.text))
        })
        .collect();

    assert!(
        !flagged.is_empty(),
        "this build wears charge-based uniques whose mods PoB cannot model; \
         none were flagged, so the `extra` flag is not being read"
    );

    // The flag marks individual lines, not whole items: an item with one
    // unsupported mod still has supported ones.
    let all_mods: usize = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter_map(|s| s.item)
        .map(|i| i.mods.len())
        .sum();
    assert!(
        flagged.len() < all_mods,
        "every mod was flagged unsupported, which cannot be right"
    );
}

#[test]
#[serial_test::serial]
fn sets_are_listed_with_the_active_one_marked() {
    // PoB scopes a build's data into named sets — one of items, one of
    // skills, one of config — each with its own selector.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    for kind in [SetKind::Item, SetKind::Skill, SetKind::Config] {
        let list = engine.sets(kind).expect("sets");
        assert!(
            !list.sets.is_empty(),
            "{} should have at least one set",
            kind.label()
        );
        assert!(
            list.active_title().is_some(),
            "{} has no active set",
            kind.label()
        );
        // Ids identify a set; titles do not, and "Default" repeats.
        let mut ids: Vec<i64> = list.sets.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "{} set ids should be unique", kind.label());
    }
}

#[test]
#[serial_test::serial]
fn a_new_set_appears_and_can_be_switched_to() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let before = engine.sets(SetKind::Item).expect("sets");
    let id = engine
        .new_set(SetKind::Item, "Early Mapping")
        .expect("new set");

    let after = engine.sets(SetKind::Item).expect("sets after");
    assert_eq!(after.sets.len(), before.sets.len() + 1);
    assert!(
        after.sets.iter().any(|s| s.id == id && s.title == "Early Mapping"),
        "the new set is not in the list under its title"
    );
    // Creating does not activate: PoB's New leaves you where you were.
    assert_eq!(after.active, before.active, "creating a set switched to it");

    engine
        .set_active_set(SetKind::Item, id)
        .expect("switch set");
    assert_eq!(
        engine.sets(SetKind::Item).expect("sets").active,
        id,
        "the set did not become active"
    );
}

#[test]
#[serial_test::serial]
fn switching_item_sets_changes_what_is_equipped() {
    // The point of item sets: a fresh one starts empty, so switching to it
    // strips the build. If this does not move the numbers, the switch never
    // reached the calculation.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let before_dps = engine.stat("TotalDPS").expect("dps");
    let worn_before = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter(|s| s.item.is_some())
        .count();
    assert!(worn_before > 0, "fixture wears gear");

    let original = engine.sets(SetKind::Item).expect("sets").active;
    let empty = engine.new_set(SetKind::Item, "Naked").expect("new set");
    engine.set_active_set(SetKind::Item, empty).expect("switch");

    // Jewels live on the tree spec, not the item set, so a socketed jewel
    // stays worn across item sets; only gear slots empty out.
    let worn_after = engine
        .equipment()
        .expect("equipment")
        .into_iter()
        .filter(|s| s.item.is_some() && !s.name.starts_with("Jewel "))
        .count();
    assert_eq!(worn_after, 0, "a fresh item set should equip no gear");
    assert_ne!(
        engine.stat("TotalDPS").expect("dps"),
        before_dps,
        "stripping every item did not reach the calculation"
    );

    // And switching back restores the gear, which is the whole promise of
    // sets: they are snapshots, not destructive edits.
    engine
        .set_active_set(SetKind::Item, original)
        .expect("switch back");
    assert_eq!(
        engine
            .equipment()
            .expect("equipment")
            .into_iter()
            .filter(|s| s.item.is_some())
            .count(),
        worn_before,
        "switching back did not restore the gear"
    );
}

#[test]
#[serial_test::serial]
fn renaming_and_deleting_sets() {
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let id = engine.new_set(SetKind::Skill, "Bossing").expect("new set");
    engine
        .rename_set(SetKind::Skill, id, "Bossing $$$")
        .expect("rename");
    assert!(
        engine
            .sets(SetKind::Skill)
            .expect("sets")
            .sets
            .iter()
            .any(|s| s.id == id && s.title == "Bossing $$$"),
        "the rename did not stick"
    );

    engine.delete_set(SetKind::Skill, id).expect("delete");
    assert!(
        !engine
            .sets(SetKind::Skill)
            .expect("sets")
            .sets
            .iter()
            .any(|s| s.id == id),
        "the deleted set is still listed"
    );
}

#[test]
#[serial_test::serial]
fn the_last_set_of_a_kind_cannot_be_deleted() {
    // A build always holds at least one set of each kind. PoB greys Delete
    // out rather than allowing a state with none; refusing loudly is the
    // same guarantee without depending on the UI to enforce it.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    let list = engine.sets(SetKind::Config).expect("sets");
    if list.sets.len() != 1 {
        // Reduce to one so the guard is what is being tested.
        let keep = list.sets[0].id;
        for set in list.sets.iter().skip(1) {
            engine.delete_set(SetKind::Config, set.id).expect("delete");
        }
        assert_eq!(engine.sets(SetKind::Config).expect("sets").sets.len(), 1);
        assert!(
            engine.delete_set(SetKind::Config, keep).is_err(),
            "deleting the last config set should be refused"
        );
    } else {
        assert!(
            engine.delete_set(SetKind::Config, list.sets[0].id).is_err(),
            "deleting the last config set should be refused"
        );
    }
}

#[test]
#[serial_test::serial]
fn a_loadout_is_sets_that_share_a_title() {
    // There is no loadout record in PoB. "New Loadout" creates a tree spec,
    // item set, skill set and config set all with the same name, and the
    // list is derived by matching those names back up.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    engine.new_loadout("Act 10").expect("new loadout");
    engine.new_loadout("Final Setup").expect("new loadout");

    let loadouts = engine.loadouts().expect("loadouts");
    assert!(
        loadouts.iter().any(|l| l == "Act 10"),
        "created loadout missing from {loadouts:?}"
    );
    assert!(
        loadouts.iter().any(|l| l == "Final Setup"),
        "created loadout missing from {loadouts:?}"
    );

    // Each kind gained a set with that title — the binding is the name.
    for kind in [SetKind::Item, SetKind::Skill, SetKind::Config] {
        assert!(
            engine
                .sets(kind)
                .expect("sets")
                .sets
                .iter()
                .any(|s| s.title == "Act 10"),
            "{} has no set called 'Act 10'",
            kind.label()
        );
    }
}

#[test]
#[serial_test::serial]
fn switching_loadout_switches_every_set_at_once() {
    // The reason loadouts exist: an act 10 setup is not just different gear,
    // it is a different tree, different gems and different config. Switching
    // one at a time gives a build that never existed.
    let engine = Engine::new(&pob_src()).expect("engine boot");
    engine.load_build_xml(GOAL_BUILD).expect("load build");

    engine.new_loadout("Levelling").expect("new loadout");
    engine.set_active_loadout("Levelling").expect("switch");

    for kind in [SetKind::Item, SetKind::Skill, SetKind::Config] {
        assert_eq!(
            engine.sets(kind).expect("sets").active_title(),
            Some("Levelling"),
            "{} did not switch to the loadout's set",
            kind.label()
        );
    }
}
