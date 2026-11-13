# pobios

Path of Building, native on iPad.

pobios runs the real [Path of Building](https://github.com/PathOfBuildingCommunity/PathOfBuilding)
calculation engine (PoB's own Lua, embedded through LuaJIT) behind a SwiftUI
interface built for touch, with the passive tree drawn on Metal. The numbers
are PoB's numbers: the test suite checks the engine against stats PoB itself
records.

Website: [pob.holdmybeer.gg](https://pob.holdmybeer.gg)

![The passive tree in pobios on iPad](https://pob.holdmybeer.gg/media/tree.webp)

<table>
  <tr>
    <td width="50%"><img src="https://pob.holdmybeer.gg/media/builds.webp" alt="Build library"><br><b>Builds</b>: DPS, EHP and level on every card</td>
    <td width="50%"><img src="https://pob.holdmybeer.gg/media/skills.webp" alt="Skills"><br><b>Skills</b>: socket groups, gems, supports</td>
  </tr>
  <tr>
    <td><img src="https://pob.holdmybeer.gg/media/items.webp" alt="Items"><br><b>Items</b>: tooltips, compare, weighted trade search</td>
    <td><img src="https://pob.holdmybeer.gg/media/calcs.webp" alt="Calcs"><br><b>Calcs</b>: PoB's full breakdown</td>
  </tr>
  <tr>
    <td><img src="https://pob.holdmybeer.gg/media/timeless.webp" alt="Timeless jewel finder"><br><b>Timeless jewels</b>: PoB's seed search, with trade links</td>
    <td><img src="https://pob.holdmybeer.gg/media/tree-detail.webp" alt="Passive tree close-up"><br><b>Tree</b>: Metal, the game's own art, jewel radii</td>
  </tr>
</table>

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
