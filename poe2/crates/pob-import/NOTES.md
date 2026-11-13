# Import/export codec notes

Verified against `tests/fixtures/occultist-100.txt`, a real PoB build code
(level 100 Witch Occultist, tree 3_29, 16.5M poison DPS).

## Round-trip preserves the build; bytes are not guaranteed

    base64url(zlib.compress(xml, level=9)) == original code

Confirmed on the fixture **using Python's zlib**. It does NOT hold from Rust:
`flate2` emits a different (equally valid) deflate stream for the same input
at the same level, so a re-encoded code decodes to identical XML but is not
the same string.

That distinction matters for anything comparing codes as strings — spotting a
change, deduping saved builds, or asserting an export matches its import.
`pob-import` tests the round trip on the XML, and separately asserts the byte
difference so that if a future backend does match PoB exactly, the test fails
and tells us the stronger guarantee became available.

Two things the original check still pins down:

- **zlib level 9.** The stream header is `78 DA`. Lower levels produce a
  different header and a non-identical code, so an export at the default
  level would round-trip *semantically* but not *byte-wise*.
- **base64url, unpadded.** `-`/`_` alphabet; PoB strips `=` padding. Decoders
  must re-pad before decoding.

Byte-identical round-trip is worth keeping as a test invariant: it catches
both attribute-ordering drift and compression-level drift in one assertion.

## Bare attributes must be sanitised before a strict parse

PoB writes some attribute values unquoted, e.g. `viewMode=TREE`. A strict XML
parser rejects the document as-is; the fixture fails to parse without a
pre-pass that quotes bare values (`=([A-Za-z0-9_.]+)([ />])`). This matches
`sanitise()` in the prototype's `pob-import.js`.

## Structure observed in the fixture

    Build     94 children  (PlayerStat entries + class/level/bandit/pantheon)
    Tree       1 Spec      (nodes, masteryEffects, Sockets)
    Skills     1 SkillSet  (13 socket groups, 35 gems)
    Items     38 children
    Calcs     53 children
    Config/Notes/Party/Import/TreeView

Spec attributes of note:

- `treeVersion="3_29"` — current, so this build shows no convert banner.
  (`sample-build.xml` is 3_26 and does; keep both to exercise each path.)
- `secondaryAscendClassId="9"` — an alternate ascendancy, which exercises the
  per-bloodline sprite sheet path in the tree renderer.
- 134 allocated nodes, 13 jewel sockets, 4 mastery effects.

## Support-gem detection

18 of the fixture's 35 gems have a `skillId` starting with `Support`. Detect
on `skillId`, never the display name, per the handoff.
