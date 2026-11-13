//! PoB is written against stock LuaJIT v2.1 from 2026-07-20 on, which
//! backports v3.0 compound assignment (`+=`, `..=`). Upstream's
//! Modules/Main.lua and Classes/CompareBuySimilar.lua use it, so the Lua
//! we vendor must parse it or the engine will not boot.
use mlua::Lua;

#[test]
fn compound_assignment_parses() {
    let lua = Lua::new();
    let (n, s): (i64, String) = lua
        .load(r#"local n = 1; n += 1; local s = "a"; s ..= "b"; return n, s"#)
        .eval()
        .expect("vendored LuaJIT rejects compound assignment");
    assert_eq!((n, s.as_str()), (2, "ab"));
}
