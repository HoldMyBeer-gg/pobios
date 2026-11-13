Please do not use AI signatures in commits.

# Two apps, side by side

Path of Building is two divergent forks now, so this repo hosts two apps
as siblings. Each is self-contained: its own Cargo workspace, its own
Xcode project, its own upstream PoB clone. They share nothing at the code
level yet (the engine Lua itself diverges heavily between the games), so a
change in one does not touch the other.

- `poe1/` — the Path of Exile 1 port. The mature app. See `poe1/CLAUDE.md`.
  Its upstream clone is `poe1/PathOfBuilding/`.
- `poe2/` — the Path of Exile 2 port. Newer. Upstream clone is
  `poe2/PathOfBuilding-PoE2/`. PoE2 ships tree art as `.dds.zst`
  (zstd-compressed BC7/BC1 DDS) with the asset map inlined at the top of
  each `TreeData/<ver>/tree.lua`, where PoE1 used sprite sheets plus
  `sprites.lua`. That asset pipeline is the one substantial rebuild; the
  FFI / iOS-host / import plumbing transfers structurally from poe1.

Work inside one app folder; that folder's own `CLAUDE.md` has the detail.
If a shared layer earns its keep once poe2 is real, factor it out then,
not before.
