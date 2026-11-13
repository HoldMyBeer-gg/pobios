//! Our numbers against PoB2's own.
//!
//! A build exported from PoB2 carries the `<PlayerStat>` values PoB2
//! computed for it. Loading the same build through our engine and reading
//! the same stats back says whether the calculation is PoB2's, unchanged.
//! What diverges is printed in full, because a mismatch is either our bug or
//! a difference between the PoB2 that exported the build and the one we
//! embed, and only the list says which.

use pob_engine::Engine;
use std::path::PathBuf;

const CODE: &str = include_str!("fixtures/gemling-mercenary-flameblast.pob2.txt");

fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing")
}

/// The stats PoB2 recorded at export, in the order it wrote them.
fn recorded(xml: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for tag in xml.split("<PlayerStat ").skip(1) {
        let field = |name: &str| {
            let key = format!("{name}=\"");
            let start = tag.find(&key)? + key.len();
            let end = start + tag[start..].find('"')?;
            Some(tag[start..end].to_owned())
        };
        if let (Some(stat), Some(value)) = (field("stat"), field("value"))
            && let Ok(value) = value.parse::<f64>()
        {
            out.push((stat, value));
        }
    }
    out
}

/// Equal the way PoB2 displays them: to a relative 1e-6, so a value that
/// went through its own rounding on export still matches.
fn same(ours: f64, theirs: f64) -> bool {
    (ours - theirs).abs() <= 1e-6 * theirs.abs().max(1.0)
}

#[test]
#[serial_test::serial]
fn our_stats_are_pob2s_stats() {
    let xml = pob_import::decode(CODE.trim()).expect("decode");
    let stats = recorded(&xml);
    assert_eq!(stats.len(), 106, "the fixture records 106 stats");

    let engine = Engine::new(&pob2_src()).expect("PoB2 boot");
    engine.load_build_xml(&xml).expect("load");

    let (mut matched, mut diverged, mut missing) = (0, Vec::new(), Vec::new());
    for (stat, theirs) in &stats {
        match engine.stat(stat).expect("read stat") {
            Some(ours) if same(ours, *theirs) => matched += 1,
            Some(ours) => diverged.push((stat.clone(), *theirs, ours)),
            None => missing.push(stat.clone()),
        }
    }

    let head = std::process::Command::new("git")
        .args(["-C", pob2_src().to_str().unwrap(), "log", "-1", "--format=%h %cs %s"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    println!("PoB2 at {head}");
    println!("{matched} of {} match; {} diverge; {} missing", stats.len(), diverged.len(), missing.len());
    for (stat, theirs, ours) in &diverged {
        let pct = if *theirs != 0.0 { (ours / theirs - 1.0) * 100.0 } else { f64::NAN };
        println!("  {stat:32} theirs {theirs:>16.6}  ours {ours:>16.6}  {pct:+.2}%");
    }
    for stat in &missing {
        println!("  {stat:32} not computed");
    }

    // Everything the fixture records, we compute.
    assert!(missing.is_empty(), "{} stats not computed", missing.len());
    // Against the release the fixture was exported from (v0.23.1) every
    // stat matches. A `dev` checkout counts Bonded runes differently and
    // gets 85; that is PoB2 changing, not us, but the clone is pinned to
    // the release and this holds it there.
    assert_eq!(matched, stats.len(), "{} stats diverge", diverged.len());
}
