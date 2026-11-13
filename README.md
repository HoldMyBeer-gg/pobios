# pobios

Path of Building, native on iPad.

pobios runs the real [Path of Building](https://github.com/PathOfBuildingCommunity/PathOfBuilding)
calculation engine (PoB's own Lua, embedded through LuaJIT) behind a SwiftUI
interface built for touch, with the passive tree drawn on Metal. The numbers
are PoB's numbers: the test suite checks the engine against stats PoB itself
records.

Two apps live side by side, one per game:

- `poe1/`: Path of Exile 1. The mature app. See `poe1/CLAUDE.md` for layout.
- `poe2/`: Path of Exile 2, on Path of Building 2. Newer. See `poe2/README.md`.

## Building

Requirements: macOS with Xcode, Rust (stable), and [XcodeGen](https://github.com/yonaskolb/XcodeGen).

```sh
cd poe1
scripts/fetch-pob.sh                  # clones upstream PoB next to the app (not tracked here)
cargo test --workspace                # engine, import, core

export POBIOS_TEAM=XXXXXXXXXX         # your Apple developer team id
cd ios && xcodegen generate
xcodebuild -project PoB.xcodeproj -scheme PoB -destination 'generic/platform=iOS'
```

Upstream PoB is never copied into this repository. Each app fetches its own
clone, and the iOS build bundles it from there.

## License

pobios is licensed under the GNU Affero General Public License v3 with the
Commons Clause: you may use, study, modify and share it, but not sell it.
See [LICENSE](LICENSE).

Path of Building is MIT-licensed by its community authors; LuaJIT, mlua,
uniffi, egui, wgpu and the bundled fonts carry their own licenses, listed on
the app's About screen.

## Not affiliated

pobios is a fan project. It is not affiliated with or endorsed by Grinding
Gear Games or the Path of Building Community. Path of Exile is a trademark of
Grinding Gear Games.
