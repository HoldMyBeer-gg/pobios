Please do not use AI signatures in commits.

# Layout

Self-contained: nothing here depends on `../../pobng` (where the engine was
first built; its history is worth reading for the why behind the crates).
This is the PoE1 app; the PoE2 port lives in the sibling `../poe2/`.

- `crates/` — the Rust workspace. `pob-engine` embeds PoB's Lua via
  mlua/LuaJIT; `pob-import` decodes share codes; `pob-core` is the build
  state plus the passive-tree renderer (egui, no eframe); `pobng-ffi` is
  the uniffi bridge SwiftUI calls; `pobng-ios` hosts the tree canvas on a
  CAMetalLayer. The `pobng-` names are kept because they are also the
  staticlib and C-symbol names; renaming is cosmetic and can come later.
- `ios/` — the Xcode project, generated from `ios/project.yml` with
  XcodeGen. Run `xcodegen generate` in `ios/` after adding Swift files.
  The build phase compiles the Rust for the current SDK and regenerates
  `ios/PoB/Generated/` from the built library, so bindings never drift.
- `PathOfBuilding/` — upstream PoB, a plain clone that is not tracked.
  `scripts/fetch-pob.sh [ref]` clones or updates it; bump it per league.
- `assets/fonts/` — fonts the egui canvas embeds.
- `docs/IOS.md` — the LuaJIT cross-compile story (`TARGET_SYS=iOS`) and
  the four ways it fails without it. `docs/A11Y.md` — the accessibility
  bar: every control addressable by name in the tree.

# Design

The UI is built to the Claude Design canvas "Path of Building iOS port"
(`PoB iPad.dc.html`, Ascent design system). `Tokens.swift` mirrors its
tokens by name. The passive tree stays egui on purpose: it is a custom
Metal surface in any toolkit, and it is hosted natively in an MTKView.

# Building

Xcode drives everything: `xcodebuild -project ios/PoB.xcodeproj -scheme PoB`
against the paired iPad, then `xcrun devicectl device install app` and
`process launch`. The first Rust build takes minutes (LuaJIT, wgpu).
