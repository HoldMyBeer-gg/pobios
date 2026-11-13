//! The passive tree, read from PoB's own processed data.
//!
//! Deliberately *not* a port of PoB's geometry. PoB computes each node's
//! position from its group, orbit index, orbit radius and an angle table —
//! and two of those tables (orbits of 16 and 40 nodes) are irregular rather
//! than evenly divided. Reimplementing that is where a tree renderer drifts:
//! everything looks nearly right, worst on the large wheels.
//!
//! Since PoB is already running in-process and has done the work, we read its
//! results. There is no second implementation to disagree with.

/// What kind of node this is, which drives its size and frame art.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Normal,
    Notable,
    Keystone,
    Mastery,
    /// A jewel socket.
    Socket,
    /// The class start location.
    ClassStart,
    /// An ascendancy start.
    AscendancyStart,
    /// The picture in the middle of a group. Decoration: PoB2 draws it faint
    /// under the connectors, and it cannot be allocated.
    OnlyImage,
}

impl NodeKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "Notable" => Self::Notable,
            "Keystone" => Self::Keystone,
            "Mastery" => Self::Mastery,
            "Socket" => Self::Socket,
            "ClassStart" => Self::ClassStart,
            "AscendClassStart" => Self::AscendancyStart,
            "OnlyImage" => Self::OnlyImage,
            _ => Self::Normal,
        }
    }

    /// The node's ring diameter in tree units, for a node on the main tree.
    ///
    /// These are PoB2's own numbers, from `GetNodeTargetSize` in
    /// `PassiveTree.lua`, **doubled**: `DrawAsset` offsets by the width and
    /// draws at `width * 2`, so the doubling is part of the size. Ascendancy
    /// nodes use larger rings; `NodeArt::of` has the full table.
    pub fn size(self) -> f32 {
        match self {
            Self::Normal => 108.0,
            Self::Notable => 160.0,
            Self::Socket => 152.0,
            Self::Mastery => 160.0,
            Self::Keystone => 240.0,
            Self::ClassStart => 74.0,
            Self::AscendancyStart => 100.0,
            Self::OnlyImage => 760.0,
        }
    }
}

/// How large PoB2 draws a node's three pieces of art, in tree units.
///
/// Straight from `GetNodeTargetSize`, doubled as `DrawAsset` doubles them.
/// Nothing here is tuned by eye: the icon is about 68% of its ring for every
/// kind, and guessing that fraction is how icons ended up poking past rings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeArt {
    /// The icon (PoB2's "base").
    pub icon: f32,
    /// The ring around it (PoB2's "overlay").
    pub ring: f32,
    /// The glow under it, for the kinds that have one.
    pub glow: f32,
}

impl NodeArt {
    pub fn of(kind: NodeKind, in_ascendancy: bool) -> Self {
        let (icon, ring, glow) = match (kind, in_ascendancy) {
            (NodeKind::AscendancyStart, _) => (0.0, 50.0, 380.0),
            (NodeKind::Normal, true) => (37.0, 80.0, 0.0),
            (NodeKind::Socket, _) => (76.0, 76.0, 0.0),
            // Any other ascendancy node, which in practice is a notable.
            (NodeKind::Notable | NodeKind::Keystone | NodeKind::Mastery, true) => {
                (54.0, 100.0, 0.0)
            }
            (NodeKind::Notable | NodeKind::Mastery, false) => (54.0, 80.0, 380.0),
            (NodeKind::Keystone, false) => (82.0, 120.0, 380.0),
            (NodeKind::Normal, false) => (37.0, 54.0, 0.0),
            (NodeKind::OnlyImage, _) => (380.0, 0.0, 0.0),
            (NodeKind::ClassStart, _) => (37.0, 1.0, 0.0),
        };
        Self { icon: icon * 2.0, ring: ring * 2.0, glow: glow * 2.0 }
    }
}

/// How a node differs between the active spec and the one being compared.
///
/// Mirrors PoB's `GetCompareNodeColor`. Nodes identical in both specs carry
/// no diff at all and draw exactly as they would with compare off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeDiff {
    /// Allocated in the compared spec but not the active one: what switching
    /// would gain. PoB draws these green.
    Gained,
    /// Allocated in the active spec but not the compared one: what switching
    /// would lose. Red.
    Lost,
    /// Allocated in both, but the choice differs — a different mastery effect
    /// or a different jewel in the socket. Blue.
    Changed,
    /// Present in the compared spec's node table and absent from the active
    /// one entirely, which happens with cluster-jewel subgraphs. PoB draws
    /// these from scratch in green, always in the allocated state.
    OnlyInCompare,
}

impl NodeDiff {
    /// Decode the engine's tag. An empty string means no difference, which is
    /// the overwhelmingly common case — most of a tree is identical between
    /// two specs, and those nodes must draw exactly as they would with
    /// compare off.
    pub fn parse(tag: &str) -> Option<Self> {
        match tag {
            "gained" => Some(Self::Gained),
            "lost" => Some(Self::Lost),
            "changed" => Some(Self::Changed),
            "only" => Some(Self::OnlyInCompare),
            _ => None,
        }
    }

    /// PoB's colour for this difference, as a plain RGB triple.
    ///
    /// These are the pure primaries PoB uses — `SetDrawColor(0, 1, 0)` and so
    /// on — multiplied over the node's normal sprite rather than replacing
    /// it. Not from our token palette, because matching PoB is the point.
    pub fn tint(self) -> [u8; 3] {
        match self {
            Self::Gained | Self::OnlyInCompare => [0x00, 0xff, 0x00],
            Self::Lost => [0xff, 0x00, 0x00],
            Self::Changed => [0x00, 0x00, 0xff],
        }
    }
}

/// How a connector is drawn, which follows from its endpoints' allocation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectorState {
    /// Both ends allocated.
    Active,
    /// On the path under the pointer. PoB2 draws this only while hovering,
    /// so nothing reports it yet.
    Intermediate,
    /// Anything else, one end allocated included.
    Normal,
}

impl ConnectorState {
    /// Where this state's file sits in an entry of `Tree::line_art`.
    pub fn art_index(self) -> usize {
        match self {
            Self::Normal => 0,
            Self::Intermediate => 1,
            Self::Active => 2,
        }
    }
}

/// A node's frame: a named asset, or a rect inside a sprite sheet.
///
/// Both forms are real. Most frames are standalone files under `TreeData`,
/// but the per-bloodline ascendancy frames are packed into `bloodline-3.webp`
/// and exist only as sprite rects — asking for `AulAscendancyFrameSmallNormal`
/// as a file finds nothing, and a missing texture draws as nothing at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub name: String,
    /// Set when the frame is packed into a sheet rather than being its own
    /// file. Coordinates are pixels, as for [`Icon`].
    pub atlas: Option<Icon>,
}

/// Where a node's icon lives.
///
/// PoB2 keys node art by an icon *path* — `key`, e.g.
/// `Art/2DArt/SkillIcons/passives/LightningDamagenode.dds` — which the art
/// layer resolves through the tree's `ddsCoords` to a DDS sheet file plus an
/// array slice index, then decodes. Unlike PoB1 there is no sub-rect: each
/// slice is the whole icon.
///
/// `px`/`sheet` are the PoB1 sprite-sheet form (a rect in sheet pixels and
/// the sheet filename). They are left in place for the frame/effect paths
/// that still emit them, but are empty for PoB2 node icons, where `key`
/// carries the icon instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    /// Left, top, right, bottom in sheet pixels (PoB1 sprite form; empty for
    /// PoB2 node icons).
    pub px: (f32, f32, f32, f32),
    /// Sheet filename as saved under `TreeData` (PoB1 sprite form).
    pub sheet: String,
    /// PoB2 icon path, resolved through `ddsCoords` in the art layer. Empty
    /// when the icon is carried by `px`/`sheet` instead.
    pub key: String,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub kind: NodeKind,
    pub name: String,
    /// Position in tree units, already computed by PoB.
    pub x: f32,
    pub y: f32,
    pub allocated: bool,
    /// Adjacent to an allocated node without being allocated itself.
    ///
    /// PoB draws these with the "CanAllocate" frame — the decorated border
    /// that marks what you could take next. Without it every unallocated node
    /// looks identical and the frontier of the build is invisible.
    pub reachable: bool,
    /// How this node differs from the spec being compared against.
    ///
    /// PoB's whole Compare feature is this: the tree itself recoloured, with
    /// no numeric summary anywhere. `None` when compare mode is off.
    pub compare: Option<NodeDiff>,
    /// Set for ascendancy nodes, naming which ascendancy they belong to.
    pub ascendancy: Option<String>,
    /// Stat lines shown in the tooltip.
    pub stats: Vec<String>,
    /// Centre of the node's group, and its angle on that group's orbit.
    ///
    /// Kept so same-orbit edges can be swept as arcs. The angle is PoB's own,
    /// taken from its irregular orbit tables, so the arc follows the wheel
    /// exactly rather than approximating it.
    pub group_x: f32,
    pub group_y: f32,
    pub angle: f32,
    pub orbit: u8,
    pub group: u32,
    /// Where this node's icon sits in the skill sprite sheet.
    ///
    /// `(x0, y0, x1, y1)` in pixels within `skills-3.jpg` (allocated) or
    /// `skills-disabled-3.jpg` (unallocated). PoB resolves this per node, so
    /// the icon inside a frame comes from the same atlas the game uses.
    pub icon: Option<Icon>,
    /// The frame ("overlay") asset drawn around the node, already resolved
    /// against PoB's `nodeOverlay` table.
    ///
    /// Resolved by the engine rather than the renderer because the variants
    /// are irregular: ascendancy frames carry a per-bloodline prefix, Abyss
    /// notables and keystones swap in their own art, Blighted keystones fall
    /// back to the plain frames, and jewel sockets use `Alt` where everything
    /// else uses `Ascend`. Composing the name from a node's flags is how you
    /// end up naming assets that do not exist.
    pub frame: Option<Frame>,
    /// Effect art drawn over the node: a tattoo's glow, or the highlight on a
    /// mastery whose effect has been chosen.
    ///
    /// `None` for ordinary nodes, and for every node on trees before 3.22,
    /// which ship no effect sprites. An allocated mastery without this reads
    /// as unallocated — the base sprite alone is nearly identical.
    pub effect: Option<Icon>,
    /// For a mastery: which effect the build has chosen, if any.
    ///
    /// Only the chosen one is carried. A mastery offers six or more effects
    /// and listing them all would overflow a tooltip, which is also what PoB
    /// does — it shows the choice, or how many remain to choose from.
    pub mastery_choice: Option<MasteryChoice>,
    /// How many effects this mastery offers in total.
    pub mastery_effect_count: usize,
    /// True for Ascendant's mirrored copies of other ascendancies' notables.
    ///
    /// These carry the *source* ascendancy's name while sitting in the
    /// Ascendant wheel, so they must be excluded when sizing a wheel.
    pub is_ascendant_mirror: bool,
    /// The radius rings drawn around an allocated socket holding a jewel
    /// with a radius, as PoB's `drawJewelRadius` picks them.
    ///
    /// Empty for every other node, and for sockets whose jewel has no
    /// radius, is abyss-conquered, or is an Impossible Escape (whose ring
    /// PoB draws on the keystone, not the socket). In compare mode a socket
    /// whose jewel differs between specs carries two sets, tinted.
    pub jewel_rings: Vec<JewelRings>,
    /// The jewel in an allocated socket: its overlay art and what its
    /// tooltip says. PoB shows the item's tooltip on a filled socket rather
    /// than the socket's name, and lays type-coloured art over the frame.
    pub socket_jewel: Option<SocketJewel>,
    /// A socket with no radius to show: a charm's, one inside another
    /// passive, or one the tree marks so (the Sinister sockets).
    pub no_radius: bool,
    /// A socket that an item grants and no point buys: the Sinister sockets,
    /// which the Voices jewel allocates, the first so many of five. One that
    /// was not granted cannot be allocated at all.
    pub granted_only: bool,
}

/// A jewel sitting in a socket node.
#[derive(Clone, Debug, PartialEq)]
pub struct SocketJewel {
    /// The name of the jewel's art, laid over an allocated socket: the base
    /// ("Ruby"), or a unique's own title where the tree carries art for it.
    pub overlay: Option<String>,
    /// The item's name; "Title, Base" for uniques.
    pub name: String,
    /// Enchant, implicit and explicit mod lines, in that order.
    pub lines: Vec<String>,
    /// PoB's rarity string: NORMAL, MAGIC, RARE, UNIQUE or RELIC.
    pub rarity: String,
    /// Thread of Hope's kind: the jewel affects an annulus, so PoB shows the
    /// ringed radii on hover rather than the discs.
    pub variable_radius: bool,
    /// The socket is allocated, so the jewel is in effect. A jewel in an
    /// unallocated socket is held, not used: PoB draws nothing for it.
    pub active: bool,
    /// What the jewel does to each passive in its radius: a timeless
    /// jewel's replacements and additions, read off the spec PoB rewrote
    /// against the base tree. Empty for jewels that change nothing.
    pub changes: Vec<PassiveChange>,
}

/// One passive a socketed jewel rewrites.
#[derive(Clone, Debug, PartialEq)]
pub struct PassiveChange {
    /// The passive's name on the base tree.
    pub from: String,
    /// Its new name, or for an addition the lines it gained, joined.
    pub to: String,
    /// A replacement (new name) rather than lines added to the original.
    pub replaced: bool,
    pub allocated: bool,
    /// A small passive, as opposed to a notable or keystone. There are
    /// dozens in a radius and they all get the same thing, so a list
    /// summarises them.
    pub small: bool,
}

impl SocketJewel {
    /// The name's colour, PoB's `colorCodes` for the rarity.
    pub fn name_colour(&self) -> [u8; 3] {
        match self.rarity.as_str() {
            "MAGIC" => [0x88, 0x88, 0xff],
            "RARE" => [0xff, 0xff, 0x77],
            "UNIQUE" => [0xaf, 0x60, 0x25],
            "RELIC" => [0x60, 0xc0, 0x60],
            _ => [0xc8, 0xc8, 0xc8],
        }
    }
}

/// Ring art for a jewel's radius, as PoB's `drawJewelRadius` lays it out.
///
/// Timeless jewels have their own two-sprite circles; everything else uses
/// PoB's shaded outer and inner rings. Sprites turn at their own rates in
/// opposite directions, which is the shimmer you see on the desktop.
///
/// Usually on the socket node. From Nothing's rings sit on the keystone the
/// jewel names instead, which is where PoB2 draws them.
#[derive(Clone, Debug, PartialEq)]
pub struct JewelRings {
    /// Sprites in draw order.
    pub sprites: Vec<RingSprite>,
    /// The largest radius in tree units, for culling.
    pub outer: f32,
    /// Draw colour, premultiplied-alpha RGBA as PoB's `SetDrawColor` takes
    /// it: white at 0.7 normally, red for a jewel only the active spec has,
    /// green for one only the compared spec has.
    pub tint: [u8; 4],
}

/// One entry of PoB's `jewelRadius` table for the tree version.
#[derive(Clone, Debug, PartialEq)]
pub struct JewelRadius {
    /// Inner radius in tree units; 0 for the plain discs, non-zero for the
    /// annuli Thread of Hope's kind use.
    pub inner: f32,
    pub outer: f32,
    /// The colour PoB draws this radius in when a socket is hovered.
    pub colour: [u8; 3],
    pub label: String,
}

/// One ring sprite.
#[derive(Clone, Debug, PartialEq)]
pub struct RingSprite {
    /// Art name under `TreeData` or `Assets`.
    pub art: String,
    /// Half the sprite's drawn width, in tree units.
    pub radius: f32,
    /// PoB's rotation factor: angle = speed × GetTime() × 0.00003.
    pub speed: f32,
}

impl Node {
    /// What a mastery's tooltip should say.
    ///
    /// An allocated mastery shows its chosen effect; an allocated one with no
    /// choice yet says so; an unallocated one reports how many options it
    /// offers. Never the full list — that is what overflows.
    pub fn mastery_summary(&self) -> String {
        match &self.mastery_choice {
            Some(choice) if !choice.stats.is_empty() => choice.stats.join("\n"),
            _ if self.allocated => "No effect chosen yet".to_owned(),
            _ => format!("{} effects to choose from", self.mastery_effect_count),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Connector {
    /// The family of line art it is drawn in: the tree's own, an
    /// ascendancy's, or a planned node's. A key into `Tree::line_art`.
    pub art: String,
    /// A colour PoB2 multiplies the art by: a weapon set's on a lit line,
    /// grey on the lines of an ascendancy that is not the build's.
    pub tint: Option<[u8; 3]>,
    /// Set when this connector's active-ness differs between the two specs.
    /// PoB forces the line to its Active art either way, so a gained path is
    /// visible rather than absent.
    pub compare: Option<NodeDiff>,
    pub from: u64,
    pub to: u64,
    pub state: ConnectorState,
    /// Points along the edge, in tree units.
    ///
    /// A straight edge has two. An arc is swept into a polyline here, using
    /// the endpoints' own orbit angles, because PoB draws arcs as textured
    /// quads whose vertices only exist in screen space at draw time.
    pub path: Vec<(f32, f32)>,
    /// Axis-aligned bounds of `path`, in tree units.
    ///
    /// Precomputed so the renderer can reject an off-screen connector with
    /// two comparisons instead of transforming every point in its path. The
    /// tree has ~3,100 connectors totalling ~26,500 points, and at any zoom
    /// most are outside the viewport, so testing them the expensive way is
    /// most of a frame's work spent discovering there is nothing to draw.
    pub bounds: (f32, f32, f32, f32),
    /// True when this edge follows an orbit rather than cutting across it.
    pub is_arc: bool,
}

/// Sweep an arc between two nodes on the same orbit.
///
/// Uses each node's own angle, so the curve matches PoB's irregular orbit
/// spacing rather than assuming even division. Returns the short way round:
/// PoB only draws an arc when the sweep is under 180°, and takes the chord
/// otherwise.
pub fn arc_path(
    group: (f32, f32),
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    segments: usize,
) -> Vec<(f32, f32)> {
    use std::f32::consts::PI;

    let (mut a, mut b) = (start_angle, end_angle);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    let mut sweep = b - a;
    // Take the short way round.
    if sweep > PI {
        a = b;
        sweep = 2.0 * PI - sweep;
    }

    let steps = segments.max(2);
    (0..=steps)
        .map(|i| {
            let t = a + sweep * (i as f32 / steps as f32);
            // Same convention as PoB: x uses sin, y uses -cos.
            (
                group.0 + t.sin() * radius,
                group.1 - t.cos() * radius,
            )
        })
        .collect()
}

/// Sweep an arc from one node to another about a centre.
///
/// PoE2 turns some connections about a point worked out from the two
/// endpoints rather than about their group, so the centre is given, and the
/// radius is whatever puts the first node on the curve. `through` is any
/// point out along the side the curve bulges towards: two nodes and a centre
/// allow two arcs, and at a half turn nothing else tells them apart.
pub fn arc_between(
    centre: (f32, f32),
    from: (f32, f32),
    to: (f32, f32),
    through: (f32, f32),
    segments: usize,
) -> Vec<(f32, f32)> {
    use std::f32::consts::TAU;

    let angle = |p: (f32, f32)| (p.1 - centre.1).atan2(p.0 - centre.0);
    let radius = ((from.0 - centre.0).powi(2) + (from.1 - centre.1).powi(2)).sqrt();
    let start = angle(from);
    // Both measured going the same way round from the start.
    let sweep = (angle(to) - start).rem_euclid(TAU);
    let via = (angle(through) - start).rem_euclid(TAU);
    let sweep = if via <= sweep { sweep } else { sweep - TAU };

    let steps = segments.max(2);
    let mut path: Vec<_> = (0..=steps)
        .map(|i| {
            let t = start + sweep * (i as f32 / steps as f32);
            (centre.0 + t.cos() * radius, centre.1 + t.sin() * radius)
        })
        .collect();
    // The ends are the nodes themselves, not where rounding put them.
    path[0] = from;
    path[steps] = to;
    path
}

/// A group's background art ring, drawn beneath its nodes.
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub x: f32,
    pub y: f32,
    /// Which background image size to use, if any.
    pub background: Option<u8>,
    /// A cluster jewel's group, which PoB draws with the expansion ring art
    /// rather than the tree's own.
    pub expansion: bool,
}

/// The effect a build has selected on a mastery node.
#[derive(Clone, Debug, PartialEq)]
pub struct MasteryChoice {
    pub effect_id: u64,
    pub stats: Vec<String>,
}

/// An ascendancy wheel: the ring of nodes for one ascendancy, plus the art
/// plate drawn behind it.
#[derive(Clone, Debug)]
pub struct Wheel {
    pub name: String,
    /// Centre, from the midpoint of this wheel's own nodes — *not* the
    /// ascendancy start group, which for multi-group wheels (Ascendant spans
    /// 26 groups) sits well off centre.
    pub x: f32,
    pub y: f32,
    /// Radius of the node ring itself.
    pub radius: f32,
    /// Radius the art plate should be drawn at: the ring scaled up, then
    /// capped so neighbouring wheels cannot overlap.
    pub art_radius: f32,
    /// The plate asset, `Classes<Name>` (~740x700).
    ///
    /// Deliberately not `AscendancyMiddle`, which is a 40x40 button glyph —
    /// scaling that up to wheel size produces a pixelated smear.
    pub art: String,
    pub node_count: usize,
}

/// How much larger than its node ring a wheel's art is drawn.
pub const WHEEL_ART_SCALE: f32 = 1.06;

/// Fraction of the distance to the nearest wheel that art may occupy.
pub const WHEEL_ART_CAP: f32 = 0.98;

/// The base attribute region a class starts in, which decides its backdrop
/// art and where that art sits.
///
/// PoB hardcodes these positions with the comment that the tree JSON does not
/// carry them, so they are transcribed rather than derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassArt {
    Str,
    Dex,
    Int,
    StrDex,
    StrInt,
    DexInt,
}

impl ClassArt {
    /// PoB's class ids, from `PassiveTreeView.lua`.
    pub fn from_class_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::Str,
            2 => Self::Dex,
            3 => Self::Int,
            4 => Self::StrDex,
            5 => Self::StrInt,
            6 => Self::DexInt,
            // 0 is Scion, which has no separate backdrop.
            _ => return None,
        })
    }

    pub fn asset(self) -> &'static str {
        match self {
            Self::Str => "BackgroundStr",
            Self::Dex => "BackgroundDex",
            Self::Int => "BackgroundInt",
            Self::StrDex => "BackgroundStrDex",
            Self::StrInt => "BackgroundStrInt",
            Self::DexInt => "BackgroundDexInt",
        }
    }

    /// Where the backdrop is centred, in tree units.
    pub fn position(self) -> (f32, f32) {
        match self {
            Self::Str => (-2750.0, 1600.0),
            Self::Dex => (2550.0, 1600.0),
            Self::Int => (-250.0, -2200.0),
            Self::StrDex => (-150.0, 2350.0),
            Self::StrInt => (-2100.0, -1500.0),
            Self::DexInt => (2350.0, -1950.0),
        }
    }
}

/// One stat's change from allocating (or unallocating) a node.
///
/// Structured rather than a formatted line: PoB builds these as colour-escaped
/// strings for its own renderer, and re-parsing them here would mean deciding
/// what "^7" meant on every league update. The renderer decides presentation;
/// this decides the numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct StatDelta {
    /// PoB's display label, e.g. "Total Mana".
    pub label: String,
    /// Change in the stat's own units, already formatted the way PoB formats
    /// it — significant digits differ per stat, and PoB owns that decision.
    pub value: String,
    /// Percent change against the current build, when PoB considers a
    /// percentage meaningful for this stat and neither value is zero.
    pub percent: Option<f64>,
    /// The same change divided by the number of points spent, for path
    /// comparisons. `None` for a single node.
    pub per_point: Option<String>,
    pub per_point_percent: Option<f64>,
    /// Whether this change is an improvement. Not simply `value > 0`: for
    /// stats where lower is better a negative delta is the good outcome.
    pub is_improvement: bool,
}

/// What allocating or unallocating a node would do to the build.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodePreview {
    /// Effect of this node alone.
    pub node: Vec<StatDelta>,
    /// Effect of this node plus every node on the path to it.
    pub path: Vec<StatDelta>,
    /// Points that would be spent reaching it, PoB's `pathDist`.
    pub path_points: u32,
    /// True when the node is already allocated, so these are the numbers for
    /// *removing* it. The wording flips entirely on this.
    pub allocated: bool,
}

/// A large picture under the tree: the class portrait and the frame round it
/// at the centre, and each ascendancy's plate out at the rim.
///
/// PoB2 carries where these go in the tree data itself, so they are reported
/// already placed, in the order PoB2 draws them.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// The asset's name, resolved through `ddsCoords` like a node icon.
    pub key: String,
    /// Centre, in tree units.
    pub x: f32,
    pub y: f32,
    /// Half the drawn width. PoB2's `DrawAsset` draws at twice the width the
    /// tree data gives, centred.
    pub half: f32,
    /// Radians the picture is turned about its centre. Only the ring that
    /// marks the class's start is turned, to point at that start.
    pub turn: f32,
    /// An ascendancy that is not the build's own, which PoB2 draws at half
    /// brightness.
    pub dimmed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// Pictures under the tree, in draw order.
    pub backdrops: Vec<Backdrop>,
    /// The file each family of line art is kept in, under the tree
    /// version's directory: plain, in-between, lit.
    pub line_art: std::collections::HashMap<String, [String; 3]>,
    /// PoB's jewel radii for this tree version, in its order: the discs
    /// first, then the annuli.
    pub jewel_radii: Vec<JewelRadius>,
    pub connectors: Vec<Connector>,
    pub groups: Vec<Group>,
    pub wheels: Vec<Wheel>,
    /// Extent of the tree in its own units, for fitting the view.
    pub bounds: (f32, f32, f32, f32),
    pub version: String,
    /// The build's class backdrop, if it has one.
    pub class_art: Option<ClassArt>,
    /// Art for the allocated class start node, e.g. "centerwitch".
    pub start_art: Option<String>,
    /// Where the class start node sits.
    pub start_pos: Option<(f32, f32)>,
}

impl Tree {
    pub fn allocated_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.allocated).count()
    }

    /// Centre of the tree's extent, the natural place to open the view.
    pub fn center(&self) -> (f32, f32) {
        let (min_x, min_y, max_x, max_y) = self.bounds;
        ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
    }

    /// Width and height in tree units.
    pub fn extent(&self) -> (f32, f32) {
        let (min_x, min_y, max_x, max_y) = self.bounds;
        (max_x - min_x, max_y - min_y)
    }

    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Compute each ascendancy's wheel from its nodes.
    ///
    /// Two rules do the work here, and getting either wrong produces wheels
    /// that overlap or sit in the wrong place:
    ///
    /// 1. Ascendant *mirror* nodes are excluded. Those carry another
    ///    ascendancy's name (their icon path gives them away) but live in the
    ///    Ascendant wheel, ~1,700 units from their namesake. Including them
    ///    drags the centre off and roughly triples the radius.
    /// 2. Art is capped at 98% of the distance to the nearest other wheel.
    ///    Nine alternate wheels sit in a row about 1,400 units apart, so
    ///    uncapped art swallows its neighbours.
    pub fn compute_wheels(nodes: &[Node]) -> Vec<Wheel> {
        use std::collections::HashMap;

        let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in nodes {
            let Some(asc) = node.ascendancy.as_deref() else {
                continue;
            };
            if node.is_ascendant_mirror {
                continue;
            }
            by_name.entry(asc).or_default().push(node);
        }

        let mut wheels: Vec<Wheel> = by_name
            .into_iter()
            .map(|(name, ns)| {
                let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
                let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
                for n in &ns {
                    min_x = min_x.min(n.x);
                    max_x = max_x.max(n.x);
                    min_y = min_y.min(n.y);
                    max_y = max_y.max(n.y);
                }
                let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                let radius = ns
                    .iter()
                    .map(|n| ((n.x - cx).powi(2) + (n.y - cy).powi(2)).sqrt())
                    .fold(0.0f32, f32::max);

                Wheel {
                    name: name.to_owned(),
                    x: cx,
                    y: cy,
                    radius,
                    art_radius: radius * WHEEL_ART_SCALE,
                    art: format!("Classes{name}"),
                    node_count: ns.len(),
                }
            })
            .collect();

        wheels.sort_by(|a, b| a.name.cmp(&b.name));

        // Cap art against the nearest neighbour so wheels cannot collide.
        let centres: Vec<(f32, f32)> = wheels.iter().map(|w| (w.x, w.y)).collect();
        for (i, wheel) in wheels.iter_mut().enumerate() {
            let nearest = centres
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (x, y))| ((wheel.x - x).powi(2) + (wheel.y - y).powi(2)).sqrt())
                .fold(f32::MAX, f32::min);

            if nearest.is_finite() {
                wheel.art_radius = wheel.art_radius.min(nearest * WHEEL_ART_CAP);
            }
        }
        wheels
    }
}

/// Orbit radii, from PoB's tree constants.
pub const ORBIT_RADII: [f32; 7] = [0.0, 82.0, 162.0, 335.0, 493.0, 662.0, 846.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arc_follows_its_orbit_rather_than_cutting_across() {
        use std::f32::consts::PI;
        // Quarter turn on a radius-100 orbit centred at the origin.
        let path = arc_path((0.0, 0.0), 100.0, 0.0, PI / 2.0, 8);

        // Every point sits on the orbit, which a chord would not.
        for (x, y) in &path {
            let r = (x * x + y * y).sqrt();
            assert!((r - 100.0).abs() < 0.01, "point off-orbit: r = {r}");
        }

        // PoB's convention: angle 0 is straight up, and sweeps clockwise.
        let (x0, y0) = path[0];
        assert!(x0.abs() < 0.01 && (y0 + 100.0).abs() < 0.01, "start {x0},{y0}");
        let (x1, y1) = path[path.len() - 1];
        assert!((x1 - 100.0).abs() < 0.01 && y1.abs() < 0.01, "end {x1},{y1}");
    }

    #[test]
    fn an_arc_takes_the_short_way_round() {
        use std::f32::consts::PI;
        // 300° apart the short way is 60°, not 300°. Sweeping the long way
        // would draw a curve most of the way around the wheel.
        let path = arc_path((0.0, 0.0), 100.0, 0.1, 0.1 + 5.0 * PI / 3.0, 16);
        let (sx, sy) = path[0];
        let (ex, ey) = path[path.len() - 1];
        let chord = ((ex - sx).powi(2) + (ey - sy).powi(2)).sqrt();
        // A 60° chord on r=100 is 100 units; a 300° sweep would also end
        // there but the path length would be five times longer.
        let length: f32 = path
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum();
        assert!(chord < 101.0, "chord {chord}");
        assert!(length < 120.0, "took the long way: length {length}");
    }

    #[test]
    fn a_mastery_reports_its_choice_rather_than_every_option() {
        // Masteries offer six or more effects. A tooltip listing all of them
        // would overflow, so only the selected one is carried — matching what
        // PoB shows.
        let chosen = Node {
            id: 1,
            kind: NodeKind::Mastery,
            name: "Shield Mastery".into(),
            x: 0.0,
            y: 0.0,
            allocated: true,
            reachable: false,
            compare: None,
            ascendancy: None,
            stats: vec![],
            group_x: 0.0,
            group_y: 0.0,
            angle: 0.0,
            orbit: 0,
            group: 0,
            icon: None,
            frame: None,
            effect: None,
            mastery_choice: Some(MasteryChoice {
                effect_id: 30612,
                stats: vec!["+1% Chance to Block".into()],
            }),
            mastery_effect_count: 6,
            is_ascendant_mirror: false,
            jewel_rings: Vec::new(),
            socket_jewel: None,
            no_radius: false,
            granted_only: false,
        };
        assert_eq!(chosen.mastery_summary(), "+1% Chance to Block");

        let unchosen = Node {
            mastery_choice: None,
            allocated: true,
            ..chosen.clone()
        };
        assert_eq!(unchosen.mastery_summary(), "No effect chosen yet");

        let unallocated = Node {
            mastery_choice: None,
            allocated: false,
            ..chosen
        };
        assert_eq!(unallocated.mastery_summary(), "6 effects to choose from");
    }

    #[test]
    fn node_sizes_match_pobs_overlay_table() {
        // The overlay widths in PoB2's GetNodeTargetSize, doubled by
        // DrawAsset. PoE2 sizes nodes from this table, not from the art's own
        // pixel width as PoE1 does.
        for (kind, overlay) in [
            (NodeKind::Normal, 54.0),
            (NodeKind::Notable, 80.0),
            (NodeKind::Socket, 76.0),
            (NodeKind::Keystone, 120.0),
        ] {
            assert_eq!(kind.size(), overlay * 2.0, "{kind:?}");
        }
    }

    #[test]
    fn an_arc_bulges_towards_the_point_it_is_told_to_pass() {
        // Opposite ends of a circle: a half turn either way, so only the
        // point to pass through decides between over the top and underneath.
        let (from, to) = ((-100.0, 0.0), (100.0, 0.0));
        for side in [-1.0f32, 1.0] {
            let path = arc_between((0.0, 0.0), from, to, (0.0, 140.0 * side), 8);
            assert_eq!((path[0], path[8]), (from, to));
            let middle = path[4];
            assert!((middle.1 - 100.0 * side).abs() < 0.01, "middle at {middle:?}");
            for (x, y) in path {
                assert!(((x * x + y * y).sqrt() - 100.0).abs() < 0.01);
            }
        }
    }

    #[test]
    fn an_arc_turns_about_the_centre_it_is_given() {
        // A shallow curve between two nodes whose centre is far from both,
        // as a connection with its own orbit has.
        let path = arc_between((0.0, 500.0), (-300.0, 100.0), (300.0, 100.0), (0.0, -200.0), 8);
        assert!((path[4].0).abs() < 0.01 && (path[4].1 - 0.0).abs() < 0.01, "{:?}", path[4]);
    }

    #[test]
    fn the_picture_in_a_group_is_its_own_kind() {
        // Left to fall through as a normal node it gets a ring, a hit area
        // and an icon slot, none of which PoB2 gives it.
        assert_eq!(NodeKind::parse("OnlyImage"), NodeKind::OnlyImage);
    }

    #[test]
    fn a_nodes_icon_is_smaller_than_its_frame() {
        // Icon sprites are 26/37/52px against frames of 40/58/84, so an icon
        // sits at roughly 64% of its frame. Both go through the same sizing
        // rule, and the ratio falling out of the real numbers is what makes
        // an inset unnecessary — inventing one is how the frames ended up
        // half-size without anyone noticing.
        for (kind, sprite, art_width) in [
            (NodeKind::Normal, 26.0, 40.0),
            (NodeKind::Notable, 37.0, 58.0),
            (NodeKind::Keystone, 52.0, 84.0),
        ] {
            let ratio = sprite / art_width;
            assert!(
                (0.55..0.75).contains(&ratio),
                "{kind:?}: icon/frame ratio {ratio} is outside the expected band"
            );
        }
    }

    #[test]
    fn unknown_node_types_fall_back_to_normal() {
        // Upstream adds node types; an unrecognised one should draw as a
        // plain node rather than vanish or panic.
        assert_eq!(NodeKind::parse("SomeFutureType"), NodeKind::Normal);
        assert_eq!(NodeKind::parse("Keystone"), NodeKind::Keystone);
    }

    #[test]
    fn an_empty_tree_has_a_usable_centre() {
        // Guards the fit-to-view maths against a division by zero before any
        // tree is loaded.
        let tree = Tree::default();
        assert_eq!(tree.center(), (0.0, 0.0));
        assert_eq!(tree.extent(), (0.0, 0.0));
    }
}

/// One line of a side-by-side stat comparison.
///
/// PoB's Compare tab keeps the section gaps its sidebar uses, so the rows are
/// a sequence rather than a flat list of stats — offence, defence and
/// resistances stay visually grouped instead of running together.
#[derive(Clone, Debug, PartialEq)]
pub enum CompareRow {
    Stat(CompareStat),
    /// A section break between groups of stats.
    Gap,
}

/// One stat, shown for both builds with the difference between them.
///
/// Values arrive pre-formatted because the format belongs to PoB: each stat
/// carries its own spec (`%d`, `%.2fs`, `%d%%`), and reimplementing that in
/// Rust would drift from upstream the first time a league changes one.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareStat {
    pub label: String,
    /// PoB's stat key, for tests and tooltips.
    pub key: String,
    /// The current build's value.
    pub a: String,
    /// The compared build's value.
    pub c: String,
    /// The difference, signed. `None` when the two are equal — PoB shows the
    /// row anyway, with an empty difference cell.
    pub diff: Option<String>,
    /// Percentage change against the current build, 0 when not meaningful.
    pub percent: f64,
    /// 1 if the change is an improvement, -1 if not, 0 if unchanged. Not
    /// simply the sign of the diff: for a lower-is-better stat a decrease is
    /// the good outcome.
    pub better: i8,
    /// The stat's own colour from PoB, for the label.
    pub colour: Option<[u8; 3]>,
}

/// One line of PoB's sidebar stat list.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarRow {
    Stat(SidebarStat),
    /// A section break, which PoB's sidebar honours.
    Gap,
}

/// A single sidebar stat, formatted by PoB.
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarStat {
    pub label: String,
    pub key: String,
    /// Already formatted, colour escapes stripped.
    pub value: String,
    /// The stat's own colour, e.g. life red or mana blue.
    pub colour: Option<[u8; 3]>,
}
