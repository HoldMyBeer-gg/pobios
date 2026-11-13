# iPad target

Status: **the Rust side cross-compiles**. There is no Xcode project yet.

## Building for iOS

```sh
TARGET_SYS=iOS cargo build --workspace --target aarch64-apple-ios
```

That environment variable is the whole trick, and it is not obvious. Without
it the LuaJIT build fails in one of four ways depending on what you try to
patch around it, each failure pointing somewhere unhelpful:

1. `Makefile:322: *** missing: export MACOSX_DEPLOYMENT_TARGET=XX.YY`
2. `make: *** [luajit.h] Killed: 9` — the host tool `minilua` got built as an
   iOS binary and the Mac kernel refused to run it
3. Same, if you set `HOST_CC`: `luajit-src` only forwards it for 32-bit targets
4. `error: use of undeclared identifier 'lj_cf_jit_util_ircalladdr'` — the one
   that actually explains the others

That last error is the tell. `TARGET_SYS=iOS` adds `-DTARGET_OS_IPHONE=1`,
which reaches `LJ_TARGET_IOS` → `LJ_OS_NOJIT` → `LJ_HASJIT 0`, and the JIT
functions live behind `#if LJ_HASJIT`. Clearing `TARGET_ARCH` (a plausible fix
for error 2) strips that define from the *host* flags only, so `buildvm`
generates a header registering JIT functions that the target build never
compiles. Host and target have to agree, and `TARGET_SYS=iOS` is how they do.

Do **not** set `MACOSX_DEPLOYMENT_TARGET`, `IPHONEOS_DEPLOYMENT_TARGET`,
`HOST_CC` or `TARGET_ARCH`. Each looks like progress and is a step away from
the answer.

`luajit-src` sets `TARGET_SYS` for Linux and Windows only, so iOS works but is
not configured by default. Worth an upstream PR.

## What this means for the engine

LuaJIT runs **interpreter-only** on iOS. Apple forbids runtime code
generation, so the JIT is compiled out — verified: zero JIT recorder symbols,
164 interpreter VM symbols. This is the standard App-Store-legal
configuration, and bundling an interpreter with its scripts is allowed; what
is banned is generating or downloading code at runtime.

`bit` and `ffi` survive, which matters: PoB uses LuaJIT extensions that plain
Lua 5.1 lacks.

Plain Lua 5.1 is **not** a fallback. `mlua` with `["lua51", "vendored"]` fails
to build for iOS outright — `lua-src` maps 5.1 to `LUA_USE_POSIX`, which does
not gate `os.execute`, so it dies on `'system' is unavailable: not available
on iOS`. Only 5.4/5.5 get `LUA_USE_IOS`. The option that seemed like the safe
retreat is the one that does not compile.

**Measured on the iPad Pro 12.9" (5th gen), interpreter-only:** every edit
on the Skills tab (gem level, enable, main skill, Full DPS) re-runs the full
calculation and feels immediate. Fast enough for the editing loop.

Two things that are not the editing loop and should be assumed slow until
measured: **node power** (PoB re-runs the calculation once per node and there
are thousands; on desktop pobng it hung the UI) and anything that walks the
whole tree the same way, such as Find Timeless Jewel. The engine is single
threaded, one per process, and cannot leave the thread that made it, so these
need to be stepped in chunks with the UI drawing between them, never run in
one call. `BuildState` already has `node_power_start`/`step`/`results` for
exactly that shape.

Tests that boot an engine must be marked serial (`serial_test`): two engines
in one process is refused, and cargo runs tests in parallel by default.

## The shell question

Two ways to get on the iPad, and they cannot be combined.

**eframe owns the app.** Works with the code as it stands — the workspace
already cross-compiles. Swift reduces to a stub. But there is no room for
native UI alongside it, so iCloud and the document picker would need solving
another way, and eframe's persistence is a no-op on iOS regardless.

**SwiftUI hosts egui.** What the README always intended, and what Lockbook
ships on the App Store: egui + egui_wgpu drawing into a `CAMetalLayer`, an
`MTKViewDelegate` driving frames, SwiftUI hosting it. Gets iCloud, the
document picker and proper text input.

The blocker for combining them is winit: it calls `UIApplicationMain` and owns
the app lifecycle, and the request to allow embedding has been open since
2019 ([winit#802](https://github.com/rust-windowing/winit/issues/802)).

**Smaller than it first looks.** `egui-wgpu` and `wgpu` are already in the
dependency tree — eframe pulls them in — and they already cross-compile here.
The iOS shell is not adopting a new renderer, it is using the one we ship
without eframe's window management. eframe's native layer is ~5,000 lines but
most of it is desktop concerns (multiple viewports, decorations, app icons,
the macOS menu bar, glow/wgpu switching). The iOS-relevant core is far less.

**iPadOS 26 may change the calculation.** It brought real windowing —
resizable windows, minimise/maximise, a menu bar. This app was built for a
desktop window model: dockable panels, splitters, a 960x600 minimum. On an
iPad Pro 12.9" (1024x1366 points) that fits in a window with room to spare, so
the UI may need no rethinking for touch at all. Being researched.

## Still to build

- Xcode project. `ios/project.yml` is a good template — XcodeGen,
  automatic signing, the same team. Needs `TARGETED_DEVICE_FAMILY: "2"` for
  iPad and a build phase for the Rust staticlib.
- A SwiftUI shell hosting the egui view.
- `PathOfBuilding/src` in the bundle (done: folder references in `ios/project.yml`), and the engine's `set_current_dir`
  reworked for the sandbox.

## The size problem

`PathOfBuilding/src` is **766MB**, of which 550MB is `TreeData` — roughly
20-25MB per tree version across 25 versions. Shipping all of it is not
reasonable for an iPad app.

Shipping the current version plus one or two prior gets it under 100MB, at the
cost of tree-version conversion, which currently works. That is a product
decision, not a technical one.

## Devices

The Apple team comes from `POBIOS_TEAM` in the environment (see
`ios/project.yml`); export your own before `xcodegen generate`. Two iPads
are already paired, so wireless deploy needs no setup:

- iPad Pro 12.9" (5th gen), **iPadOS 26.6**, available now
- iPad mini (A17 Pro), currently offline

The Pro being on 26.6 matters: the new windowing is available on the real
target device, not just in principle.
