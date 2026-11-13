# PoB2 (Path of Exile 2) port

Sibling to `../poe1/`. Path of Building 2 is a **hard fork** of PoB, not a
data pack: the engine Lua itself diverges (`CalcOffence` ~40% churned,
`ModParser` ~35%, `CalcSetup` nearly rewritten), classes/ascendancies and
item bases are different, and the tree-art format changed. So the two apps
stay separate at the code level; only the *plumbing techniques* carry over.

Upstream clone: `PathOfBuilding-PoE2/` (plain clone, untracked, like poe1).

## What transfers from poe1, and what is new

The expensive, hard-won parts of poe1 transfer structurally, because the
module system is identical (`Launch.lua` -> `HeadlessWrapper.lua` ->
`LoadModule`/`dofile`, LuaJIT, the `_SimpleGraphic.def.lua` shim):

| Layer | Reuse | Note |
|---|---|---|
| engine embedding scaffold (`pob-engine`) | High | Same dofile/HeadlessWrapper/package.path approach, pointed at the PoE2 clone. |
| uniffi bridge (`pobng-ffi`) | High | Interface shape unchanged. |
| iOS CAMetalLayer host (`pobng-ios`) | High | Same. |
| SwiftUI shell, XcodeGen, a11y approach | High | Same. |
| share-code import (`pob-import`) | Medium | Transport (base64url + zlib of build XML) is the same; the XML *field meanings* are PoE2-shaped. Verify against a real PoE2 code. |
| calc/config/stat reads (`pob-core`) | Medium | Reading `Modules/*.lua` declaratively still works; the specific stats/options are PoE2's. |
| **tree renderer asset path** (`tree_art.rs`) | **Low — the one real rebuild** | `sprites.lua` is gone; art is `.dds.zst`. See below. |

## The tree-art pipeline (the de-risked part)

PoE1 shipped sprite sheets (webp/png/jpg) + `sprites.lua` with pixel rects.
PoE2 ships, per `TreeData/<ver>/`:

- **`tree.lua`** with the asset model inlined:
  - `assets` — name -> zoom level -> filename, for whole-image art
    (orbits, backgrounds).
  - `ddsCoords` — atlas filename -> sprite name -> **array slice index**.
  - `nodeOverlay` — which frame asset to draw per node kind/state.
- **`*.dds.zst`** — zstd-compressed **DX10 DDS**. BC1 (`skills*`, the icon
  sheets) and BC7 (backgrounds, portraits), with mip chains. The icon
  sheets are **BC texture arrays**: one square slice per icon, and the
  `ddsCoords` integer is the slice index (not a pixel rect).

**Key constraint:** Apple GPUs cannot sample BC/S3TC formats in Metal, so
the blocks must be CPU-decoded to `Rgba8`. That is not a detour: poe1
already CPU-decodes png/jpg to `Rgba8` and hands it to egui, so the decoded
output drops straight into the existing `tree_art` texture path.

### `crates/tree-assets` — the working prototype

Proves the whole chain on the shipped assets: `.dds.zst` -> zstd inflate ->
DX10 DDS parse -> BC1/BC7 block decode -> `Rgba8` -> PNG.

```
cargo run -p tree-assets --bin decode        # defaults to TreeData/0_5
cargo run -p tree-assets --bin decode <TreeDataVerDir> <OutDir>
```

Verified output against `0_5`:

```
file                                     dims    format slices mips  raw KiB
group-background_92_92_BC7.dds.zst       92x92   Bc7      1     7        11
skills_64_64_BC1.dds.zst                 64x64   Bc1    175     7       469
ddsCoords for skills_64_64_BC1: 175 sprites   # == array_size: model confirmed
  slice 1 = "Art/2DArt/SkillIcons/ExplosiveGrenade.dds"
```

Both decoded to correct imagery (the gold group-background frame; the
ExplosiveGrenade icon). The `ddsCoords` sprite count matching the DDS array
size is the proof the sheets are texture arrays.

`tree-assets` is the seed of the real PoE2 `tree_art`: it already produces
the `Rgba8` egui wants. What it still needs for production is (a) reading
`assets`/`ddsCoords` from the live Lua table the engine already `dofile`s
(the prototype light-scans the text instead), and (b) lazy per-slice decode
+ an egui texture cache, mirroring poe1's `tree_art.rs`.

## The engine: boots, loads, computes (done)

`pob-import` and `pob-engine` are copied from poe1 and re-pointed at the
PoE2 checkout. Nothing in the engine source hardcoded a PoB path (it takes
`pob_src` as a parameter), and PoB2 ships the same vendored Lua deps plus
`HeadlessWrapper.lua`, so the poe1 boot scaffold carries over unchanged.

- `tests/boot.rs` — PoB2's Lua boots headless through the exact poe1 path
  (loads the 0_5 tree, assets, uniques, rares).
- `tests/load_build.rs` — decodes a real PoB2 share code, loads it, and
  reads a live stat block back.

### Parity: 106/106 against the release the build came from

`tests/parity_probe.rs` loads the Flameblast fixture and compares every
`<PlayerStat>` PoB2 recorded at export with what our engine computes for
the same build.

- Against **v0.23.1** (the public release, 2026-07-28): **106 of 106 match
  exactly.** The fixture was exported from that release.
- Against `dev` (2026-09-10 and 2026-09-30): 85 of 106. The 21 that
  diverge start at Spirit and Chaos Resistance and come from `dev` having
  changed how a Bonded rune's lines and "+Spirit per Idol" are counted on
  the Morior Invictus in the build. That is PoB2 changing between versions,
  reproduced faithfully by us either way.

An earlier version of this section blamed the 21 on an upstream spirit
reservation bug. That was wrong: no bug, just two PoB2 versions.

The clone is pinned to the release tag for this reason: what a user sees in
PoB2 is the release, so that is what the app should compute. Bring it
forward with `git -C PathOfBuilding-PoE2 fetch --depth 1 origin tag vX.Y.Z`
and `git checkout vX.Y.Z`, then run the workspace tests. The engine boots
either branch: `dev` renamed the headless launch global to `__mainObject__`
and the engine falls back to `launch`, which both set.

## Next steps

1. Re-verify each parser against PoE2 schemas: `tree.rs` (classes/nodes),
   `items.rs` (bases: crossbow/flail/focus/spear/sceptre/talisman), config
   and stat maps.
2. Grow `tree-assets` into `tree_art`: slice cache, mip selection for zoom,
   `nodeOverlay` frames.
3. Scaffold `pobng-ffi`/`pobng-ios` against the PoE2 clone (copy poe1's,
   re-point paths); the engine crates are already in place.
