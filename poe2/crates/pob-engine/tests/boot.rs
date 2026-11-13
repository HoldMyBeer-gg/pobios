//! The premise test for the PoE2 port: does PoB2's Lua boot headless
//! through the exact scaffold poe1 uses?
//!
//! If this passes, the single hardest, scariest part of the PoE1 build
//! (embed PoB with no SimpleGraphic, cross-built LuaJIT, HeadlessWrapper)
//! transfers to PoE2 unchanged, and everything after is adapt-and-verify.

use pob_engine::Engine;
use std::path::PathBuf;

/// The PoE2 upstream checkout's `src/`.
fn pob2_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PathOfBuilding-PoE2 checkout missing")
}

#[test]
#[serial_test::serial]
fn pob2_boots_headless() {
    let engine = Engine::new(&pob2_src()).expect("PoB2 engine boot");
    // Booting at all is the milestone: HeadlessWrapper ran, PoB2's Data and
    // Modules loaded lazily behind it. Keep the handle alive to the end so a
    // teardown crash would still surface here.
    let _ = engine;
    println!("PoB2 booted headless.");
}
