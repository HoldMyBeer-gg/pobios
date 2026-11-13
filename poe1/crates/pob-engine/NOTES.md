# Embedding PoB's engine

Answers the question the architecture rests on: can we run upstream's Lua
unmodified and get PoB's own numbers back? Yes. Verified against a real
exported build (`tests/fixtures/occultist-100.xml`, level 100 Occultist).

    TotalDPS       1829571.8491040005   (PoB recorded 1829571.849104)
    AverageHit      237836.18600000002  (PoB recorded  237836.186)
    Speed                7.69257142857  (PoB recorded       7.6925714285714)
    WithPoisonDPS  16577674.199258717   (PoB recorded 16577674.199259)

Differences are PoB's 6-decimal rounding in the XML, not calculation drift.

## What we run

`src/HeadlessWrapper.lua` — upstream's own headless entry point, used by their
test suite. It `dofile`s `_SimpleGraphic.def.lua`, their stub of the host API.
We did NOT write SimpleGraphic shims; staying on upstream's wrapper means
their headless path stays maintained for us.

## LuaJIT is required, not preferred

`Launch.lua` calls `jit.opt.start`, and PoB targets 5.1 semantics
(`collectgarbage("setpause")` is 5.1-only; Lua 5.5 rejects it). mlua with
`features = ["luajit", "vendored"]` builds LuaJIT from source — no system
dependency, builds clean on ARM64 macOS.

## Three things that had to be solved

**1. Module search path.** PoB vendors pure-Lua deps in `runtime/lua/`, a
sibling of `src/`. Upstream's CI sets `LUA_PATH` to
`../runtime/lua/?.lua;../runtime/lua/?/init.lua`
(`.github/workflows/test.yml`). We set `package.path` in-process instead, so
we don't depend on the caller's environment.

**2. `lua-utf8` is a native module PoB ships only as a Windows DLL.** We
supply `lua/lua-utf8.lua` as a stand-in. Scope check before writing it:
`utf8.` appears 12 times in PoB's source — 6 in `Classes/EditControl.lua` (a
text-entry widget we never load) and 6 in `Modules/Common.lua`'s
`formatNumber` (thousands separators, display only). No calc path depends on
it, so the shim cannot move a number.

**3. The working directory must stay at `src/` for the engine's lifetime.**
This one cost the most time and is the most important to preserve. PoB
resolves modules with relative paths *lazily* — `Modules/BuildDisplayStats.lua`
is not loaded until a frame runs after a build is applied. Setting the CWD
only across boot and restoring it afterwards makes that later load fail.

The failure is silent: PoB writes startup errors to `__mainObject__.promptMsg`
and carries on rather than raising. The symptom was a build whose header
parsed (level 100, correct class) but whose tree and skills were empty —
1 allocated node, 0 socket groups, TotalDPS 0.06 — with no error anywhere.

`Engine::prompt_msg` now checks that channel after boot and after every load,
so this class of failure surfaces as an error instead of as wrong numbers.

Consequence: the CWD is process-global, which makes it the one genuinely
invasive thing the engine does. Hold exactly one `Engine` per process.

## Why this matters for league updates

3.30 lands, upstream updates `Modules/Calc*.lua` and `Data/`, we re-vendor and
re-run this test. If their changes move a number we find out here rather than
from users. The fixture's `<PlayerStat>` values are PoB's own answers, so the
test is a genuine cross-check and not a self-consistency check.
