#!/usr/bin/env python3
"""Write ios/PoB/Resources/THIRD-PARTY.txt: every Rust crate the app links,
with its licence, plus the fonts and LuaJIT. Run after changing Cargo deps.
PoB's own licence ships separately as LICENSE.md, verbatim."""
import json, subprocess, pathlib
root = pathlib.Path(__file__).resolve().parent.parent
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1"], cwd=root))
ours = {"pob-engine", "pob-import", "pob-core", "pobng-ffi", "pobng-ios"}
crates = sorted({(p["name"], p["version"], p.get("license") or "see repository", p.get("repository") or "")
                 for p in meta["packages"] if p["name"] not in ours}, key=lambda t: t[0].lower())
lines = ["Third-party software in Path of Building for iPad", "",
         "Path of Building itself is MIT licensed; its full notice, including its own",
         "dependencies, is in LICENSE.md.", "",
         "LuaJIT  Copyright (C) 2005-2025 Mike Pall. MIT licence. https://luajit.org/", "",
         "Fonts (SIL Open Font License 1.1 unless noted):",
         "  Inter (The Inter Project Authors), JetBrains Mono (JetBrains),",
         "  Cinzel Decorative (Natanael Gama), OpenDyslexic (Abbie Gonzalez).", "",
         "Rust crates (licence as declared by each crate):"]
for name, ver, lic, repo in crates:
    lines.append(f"  {name} {ver}  {lic}" + (f"  {repo}" if repo else ""))
lines += ["", "uniffi is under the Mozilla Public License 2.0 and is used unmodified;",
          "its source is at https://github.com/mozilla/uniffi-rs."]
out = root / "ios/PoB/Resources/THIRD-PARTY.txt"
out.write_text("\n".join(lines) + "\n")
print(f"{out.relative_to(root)}: {len(crates)} crates")
