//! What SwiftUI can call.
//!
//! The iPad build splits the UI: SwiftUI owns the chrome — tabs, toolbars,
//! text fields, dialogues — and egui draws only the passive tree and the
//! dense panels Lua feeds. This crate is the seam between them.
//!
//! `BuildState` is already a clean API over the engine with no egui in it, so
//! this mostly forwards to it. What it adds is a shape uniffi can generate
//! Swift from: plain records rather than the engine's own types, and one
//! object holding the state rather than a `BuildState` Swift would have to
//! own.
//!
//! # Threading
//!
//! PoB supports exactly one engine per process, its Lua state is not `Send`,
//! and it finds its modules through a process-global working directory. None
//! of that is incidental — see the LIVE guard in `pob-engine`.
//!
//! uniffi requires exported objects to be `Send + Sync`, so the state cannot
//! simply be held here. It lives in a thread-local instead, created on the
//! thread that first calls `Build::new` and only ever touched from there.
//! Every method asserts it is on that thread rather than silently doing the
//! wrong thing somewhere else.
//!
//! In practice that thread is the main one, which is where UIKit calls from
//! anyway. The cost is that a background call returns empty rather than
//! blocking, which is the right failure: PoB's calculations are slow enough
//! interpreter-only that blocking the UI thread from a background queue
//! would be worse than not answering.

use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};

uniffi::setup_scaffolding!();

/// One equipment slot and what is in it.
#[derive(uniffi::Record)]
pub struct Slot {
    pub name: String,
    /// What to call it on screen; a tree socket shows its node's name.
    pub label: String,
    /// The item's display name, absent when the slot is empty.
    pub item: Option<String>,
    /// PoB's rarity colour as `#rrggbb`, for tinting the name.
    pub colour: String,
    /// Whether a flask is active. `None` for slots that are not flasks —
    /// PoB puts the checkbox only on those.
    pub flask_active: Option<bool>,
}

/// The outcome of a weighted "Find best" trade search for one slot. Exactly
/// one field is set: `query` is the trade query JSON (the caller turns it into
/// a pathofexile.com/trade URL with the user's league and realm), or `error`
/// is PoB's own message, e.g. when the slot has no mods worth searching for.
#[derive(uniffi::Record)]
pub struct TradeResult {
    pub query: Option<String>,
    pub error: Option<String>,
}

/// One weighted sort stat for the trade search. `stat` is a
/// `data.powerStatList` key (e.g. "FullDPS", "TotalEHP"); `weight_mult` is its
/// multiplier. PoB's default list is Full DPS x1.0 and Effective Hit Pool x0.5.
#[derive(uniffi::Record)]
pub struct StatWeight {
    pub stat: String,
    pub weight_mult: f64,
}

/// One mod line on an item.
#[derive(uniffi::Record)]
pub struct ModText {
    pub text: String,
    /// implicit, explicit, crafted, enchant, scourge, crucible.
    pub kind: String,
    /// PoB parsed it but cannot apply all of it; PoB paints these red.
    pub unsupported: bool,
}

/// An item in full, for the detail card and tooltip.
#[derive(uniffi::Record)]
pub struct ItemDetail {
    pub name: String,
    pub base: String,
    pub rarity: String,
    /// Rarity colour as `#rrggbb`.
    pub colour: String,
    pub item_level: i64,
    pub quality: i64,
    pub req_level: Option<i64>,
    pub req_str: Option<i64>,
    pub req_dex: Option<i64>,
    pub req_int: Option<i64>,
    pub mods: Vec<ModText>,
    pub flavour: Option<String>,
    /// PoB's headline numbers for the item: DPS, crit, APS, armour, ES...
    pub stats: Vec<OptionValue>,
}

/// One line of "what would change if this were equipped".
#[derive(uniffi::Record)]
pub struct StatChange {
    pub label: String,
    /// Already formatted by PoB.
    pub value: String,
    pub percent: Option<f64>,
}

/// The right half of PoB's item tooltip.
#[derive(uniffi::Record)]
pub struct ItemPreview {
    pub replacing: Option<String>,
    /// The item is the one equipped, so the numbers are for removing it.
    pub is_equipped: bool,
    pub changes: Vec<StatChange>,
}

/// One entry of PoB's unique database.
#[derive(uniffi::Record)]
pub struct DbItem {
    pub name: String,
    pub base: String,
    /// PoB's item type, e.g. "Body Armour", "Wand".
    pub item_type: String,
    pub colour: String,
    pub league: Option<String>,
    pub req_level: Option<i64>,
}

/// A craftable base, by PoB's type.
#[derive(uniffi::Record)]
pub struct CraftBase {
    pub item_type: String,
    pub name: String,
}

/// One row of PoB's Compare summary. A gap row separates groups.
#[derive(uniffi::Record)]
pub struct CompareRow {
    pub is_gap: bool,
    pub label: String,
    pub key: String,
    /// This build's value, formatted by PoB.
    pub a: String,
    /// The compared build's value.
    pub c: String,
    /// The signed difference; empty when equal.
    pub diff: String,
    pub percent: f64,
    /// 1 better, -1 worse, 0 unchanged; lower-is-better stats accounted for.
    pub better: i8,
    pub colour: String,
}

/// A named option value, as text.
#[derive(uniffi::Record)]
pub struct OptionValue {
    pub name: String,
    pub value: String,
}

/// One item the build owns.
#[derive(uniffi::Record)]
pub struct Item {
    pub id: i64,
    pub name: String,
    pub colour: String,
    /// Slots this item is worn in. Empty means unused.
    pub worn_in: Vec<String>,
}

/// A stat as PoB formats it, for the sidebar.
#[derive(uniffi::Record)]
pub struct Stat {
    pub label: String,
    pub value: String,
    pub colour: String,
}

/// One group of sidebar stats. PoB separates its sidebar with blank lines;
/// each run between blanks is a section here.
#[derive(uniffi::Record)]
pub struct StatSection {
    pub rows: Vec<Stat>,
}

/// One passive tree ("spec") in the build. Index is 1-based.
#[derive(uniffi::Record)]
pub struct TreeSpec {
    pub index: u32,
    pub title: String,
}

/// A tree version PoB can convert to.
#[derive(uniffi::Record)]
pub struct TreeVersion {
    pub id: String,
    pub label: String,
}

/// One choice in PoB's node-power dropdown. An empty `stat` is the
/// combined Offence/Defence measure.
#[derive(uniffi::Record)]
pub struct PowerStat {
    pub stat: String,
    pub label: String,
}

/// One row of PoB's Power Report, formatted by PoB.
#[derive(uniffi::Record)]
pub struct PowerReportRow {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub power: f64,
    pub power_text: String,
    pub per_point_text: String,
    pub points_text: String,
    pub allocated: bool,
    pub lower_is_better: bool,
}

/// One selectable effect on a mastery passive: PoB's effect id and the stat
/// line(s) it grants.
#[derive(uniffi::Record)]
pub struct MasteryEffect {
    pub id: u32,
    pub label: String,
}

/// One node's ranking, raw. Normalise before drawing.
#[derive(uniffi::Record)]
pub struct NodePower {
    pub id: u64,
    pub name: String,
    pub value: f64,
    pub allocated: bool,
}

// MARK: Find Timeless Jewel records. Mirrors `pob_engine::timeless`.

#[derive(uniffi::Record)]
pub struct TimelessChoice {
    pub id: u8,
    pub label: String,
}

#[derive(uniffi::Record)]
pub struct TimelessSocket {
    pub id: i64,
    pub label: String,
    pub keystone: String,
    pub allocated: bool,
}

#[derive(uniffi::Record)]
pub struct LegionNode {
    pub id: String,
    pub name: String,
    pub stats: Vec<String>,
    pub notable: bool,
    pub total: bool,
    pub stat_count: u32,
    pub primary_stat: String,
    pub secondary_stat: String,
}

/// A desired or fallback node line. Weights are text because "required" is
/// a valid weight alongside numbers, exactly as PoB stores them.
#[derive(uniffi::Record, Clone)]
pub struct DesiredNode {
    pub id: String,
    pub weight1: String,
    pub weight2: String,
    pub minimum: String,
}

#[derive(uniffi::Record)]
pub struct TimelessState {
    pub jewel_type: u8,
    pub conqueror: u8,
    pub devotion1: u8,
    pub devotion2: u8,
    pub socket: i64,
    pub socket_filter: bool,
    pub socket_distance: u32,
    pub desired: Vec<DesiredNode>,
    pub fallback: Vec<DesiredNode>,
    pub fallback_mode: u32,
    pub total_minimum_weight: Option<f64>,
}

#[derive(uniffi::Record)]
pub struct TimelessOptions {
    pub jewel_types: Vec<TimelessChoice>,
    pub conquerors: Vec<TimelessChoice>,
    pub devotion_variants: Vec<TimelessChoice>,
    pub sockets: Vec<TimelessSocket>,
    pub nodes: Vec<LegionNode>,
    pub fallback_modes: Vec<String>,
    pub protect_candidates: Vec<String>,
    pub abyss_ascendancies: Vec<LegionNode>,
}

#[derive(uniffi::Record)]
pub struct NodeMatch {
    pub name: String,
    pub weight: f64,
    pub targets: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct TimelessResult {
    pub seed: i64,
    pub total: f64,
    pub socket_id: Option<i64>,
    pub socket_label: String,
    pub matches: Vec<NodeMatch>,
    pub added: bool,
}

#[derive(uniffi::Record)]
pub struct TimelessSearch {
    pub desired: Vec<String>,
    pub results: Vec<TimelessResult>,
}

fn desired_out(rows: Vec<pob_engine::timeless::DesiredNode>) -> Vec<DesiredNode> {
    rows.into_iter()
        .map(|d| DesiredNode { id: d.id, weight1: d.weight1, weight2: d.weight2, minimum: d.minimum })
        .collect()
}

fn desired_in(rows: &[DesiredNode]) -> Vec<pob_engine::timeless::DesiredNode> {
    rows.iter()
        .map(|d| pob_engine::timeless::DesiredNode {
            id: d.id.clone(),
            weight1: d.weight1.clone(),
            weight2: d.weight2.clone(),
            minimum: d.minimum.clone(),
        })
        .collect()
}

fn legion_nodes_out(rows: Vec<pob_engine::timeless::LegionNode>) -> Vec<LegionNode> {
    rows.into_iter()
        .map(|n| LegionNode {
            id: n.id,
            name: n.name,
            stats: n.stats,
            notable: n.notable,
            total: n.total,
            stat_count: n.stat_count as u32,
            primary_stat: n.primary_stat,
            secondary_stat: n.secondary_stat,
        })
        .collect()
}

fn search_out(s: pob_engine::timeless::TimelessSearch) -> TimelessSearch {
    TimelessSearch {
        desired: s.desired,
        results: s
            .results
            .into_iter()
            .map(|r| TimelessResult {
                seed: r.seed,
                total: r.total,
                socket_id: r.socket_id,
                socket_label: r.socket_label,
                matches: r
                    .matches
                    .into_iter()
                    .map(|m| NodeMatch { name: m.name, weight: m.weight, targets: m.targets })
                    .collect(),
                added: r.added,
            })
            .collect(),
    }
}

/// A named set of items, skills or config.
#[derive(uniffi::Record)]
pub struct NamedSet {
    pub id: i64,
    pub title: String,
    pub active: bool,
}

/// Which of PoB's undo histories a call refers to. Each tab keeps its own.
#[derive(uniffi::Enum, Clone, Copy)]
pub enum UndoScope {
    Tree,
    Skills,
    Items,
    Config,
}

impl UndoScope {
    fn key(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Skills => "skills",
            Self::Items => "items",
            Self::Config => "config",
        }
    }
}

/// Whether a scope has anything to undo or redo.
#[derive(uniffi::Record)]
pub struct UndoState {
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Which kind of set a call refers to.
#[derive(uniffi::Enum)]
pub enum SetKind {
    Item,
    Skill,
    Config,
}

impl From<SetKind> for pob_engine::items::SetKind {
    fn from(k: SetKind) -> Self {
        match k {
            SetKind::Item => Self::Item,
            SetKind::Skill => Self::Skill,
            SetKind::Config => Self::Config,
        }
    }
}

/// One gem in a socket group.
#[derive(uniffi::Record)]
pub struct Gem {
    pub name: String,
    pub level: i64,
    pub quality: i64,
    /// Quality after item/socket bonuses; > `quality` when something (a red
    /// socket's +quality, Dialla's) boosts it. The extra is not typed.
    pub effective_quality: i64,
    pub enabled: bool,
    pub count: i64,
    /// True for supports, which PoB identifies by skill id rather than by
    /// the display name.
    pub is_support: bool,
    /// The socket colour this gem needs, as `#rrggbb`.
    pub colour: String,
}

/// A gem the build could socket, from PoB's own gem list.
#[derive(uniffi::Record)]
pub struct GemChoice {
    /// PoB's gem id, which `set_gem_id` is keyed by.
    pub id: String,
    pub name: String,
    pub is_support: bool,
    /// Socket colour as `#rrggbb`, for tinting the name.
    pub colour: String,
}

/// A support gem ranked by the DPS it would add to a socket group.
#[derive(uniffi::Record)]
pub struct RankedGem {
    pub id: String,
    pub name: String,
    pub colour: String,
    /// For a candidate: DPS it would add if socketed. For one already in the
    /// group (`socketed`): the DPS it is currently contributing.
    pub delta: f64,
    /// Already in the group; `delta` is its worth, what a candidate is
    /// weighed against.
    pub socketed: bool,
}

/// PoB's main-skill controls: active skill, part, stages, mines, minion.
/// Empty lists mean PoB would hide that control. Indices are 1-based.
#[derive(uniffi::Record)]
pub struct MainSkillControls {
    pub active_skills: Vec<String>,
    pub active_index: u32,
    pub parts: Vec<String>,
    pub part_index: u32,
    pub stages: Option<i64>,
    pub mines: Option<i64>,
    pub minions: Vec<String>,
    /// What `set_main_skill_minion` takes, parallel to `minions`.
    pub minion_values: Vec<String>,
    pub minion_index: u32,
    pub minion_by_item_set: bool,
    pub minion_skills: Vec<String>,
    pub minion_skill_index: u32,
}

/// A socket group: the gems in one item, and whether they are live.
#[derive(uniffi::Record)]
pub struct SkillGroup {
    pub label: String,
    /// Where it is socketed, absent for groups granted by an item.
    pub slot: Option<String>,
    pub enabled: bool,
    /// Granted by an item or effect rather than socketed, and so not
    /// editable — PoB labels these not editable too.
    pub granted: bool,
    /// Counted in the Full DPS total.
    pub full_dps: bool,
    /// The group PoB treats as the main skill.
    pub is_main: bool,
    /// The equipped item's socket colours, e.g. "R-B-B-B-B-B"; empty for a
    /// granted group or an empty slot.
    pub item_sockets: String,
    pub gems: Vec<Gem>,
}

/// One cell of a calc row.
#[derive(uniffi::Record)]
pub struct CalcCell {
    pub value: String,
    /// PoB shows a breakdown when this is tapped.
    pub has_breakdown: bool,
}

/// One row of a calc section.
#[derive(uniffi::Record)]
pub struct CalcRow {
    pub label: String,
    pub cells: Vec<CalcCell>,
    /// True for the row that names the damage-type columns.
    pub is_header: bool,
}

/// A section of the Calcs tab, already formatted and filtered by PoB.
#[derive(uniffi::Record)]
pub struct CalcSection {
    pub id: String,
    pub label: String,
    pub rows: Vec<CalcRow>,
    /// PoB's column: 1 offence, 2 resources, 3 defence.
    pub column: u8,
    pub colour: String,
    /// The header value after the title, e.g. "35,895.7 DPS".
    pub extra: String,
    pub collapsed_default: bool,
    /// For `calc_breakdown`.
    pub section_index: u32,
    pub sub_index: u32,
}

/// One part of a cell's breakdown: text lines, or a table.
#[derive(uniffi::Record)]
pub struct BreakdownPart {
    pub label: Option<String>,
    pub footer: Option<String>,
    pub lines: Vec<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The Calcs tab's header inputs.
#[derive(uniffi::Record)]
pub struct CalcsInputs {
    pub groups: Vec<String>,
    pub group_index: u32,
    pub controls: MainSkillControls,
    pub buff_mode: String,
    pub has_minion: bool,
    pub show_minion: bool,
}

/// What kind of control a config option wants.
#[derive(uniffi::Enum)]
pub enum ConfigKind {
    Check,
    Number,
    List,
}

/// One configuration option.
#[derive(uniffi::Record)]
pub struct ConfigOption {
    /// PoB's variable name, which is what a write is keyed by.
    pub var: String,
    pub label: String,
    pub section: String,
    pub kind: ConfigKind,
    /// The current value as text. Empty means unset, which for PoB means
    /// "use the default" — not the same as zero or false.
    pub value: String,
    /// For a list option: the choices, as (key, label).
    pub choices: Vec<ConfigChoice>,
    /// PoB would show this option for the current build.
    pub relevant: bool,
    pub tooltip: Option<String>,
}

/// One block of custom modifiers on the Config tab.
#[derive(uniffi::Record)]
pub struct CustomModBlock {
    pub title: String,
    pub enabled: bool,
    pub text: String,
}

/// One choice in a list-valued config option.
#[derive(uniffi::Record)]
pub struct ConfigChoice {
    pub key: String,
    pub label: String,
}

/// One ascendancy of a class.
#[derive(uniffi::Record)]
pub struct AscendancyChoice {
    pub id: u8,
    pub name: String,
}

/// A class and its ascendancies, in PoB's order.
#[derive(uniffi::Record)]
pub struct ClassChoice {
    pub id: u8,
    pub name: String,
    pub ascendancies: Vec<AscendancyChoice>,
}

/// The character line: level, class, points spent.
#[derive(uniffi::Record)]
pub struct BuildInfo {
    pub level: i64,
    pub class: String,
    pub ascendancy: String,
    pub points_used: u32,
    pub points_max: u32,
    pub ascendancy_points_used: u32,
    pub ascendancy_points_max: u32,
}

/// Bumped whenever the build changes from the Rust side, so SwiftUI can tell
/// its cached view of the build is stale. Allocating a node in the egui
/// canvas is the case: the engine recalculates, but no Swift call was made.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Note a change the Swift side did not initiate.
pub fn bump_generation() {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// One job for the engine thread: it gets the slot the state lives in, so a
/// job can also create or replace the state.
type Job = Box<dyn FnOnce(&mut Option<pob_core::build_state::BuildState>) + Send>;

/// The thread that owns the engine.
///
/// PoB's Lua state cannot leave the thread that made it, and a full
/// recalculation takes long enough to feel on a touch screen. So the engine
/// lives on a thread of its own and every call from the UI is a job sent
/// over a channel. The channel is FIFO: a cheap read queued behind a long
/// import waits for it, which is what a spinner is for.
struct EngineThread {
    tx: mpsc::Sender<Job>,
}

static ENGINE: OnceLock<EngineThread> = OnceLock::new();

/// The tree as of the last finished job, for the canvas. Read on the render
/// thread at the start of every frame; written by the engine thread after
/// every job. An `Arc` swap, so neither side waits on the other.
static TREE: RwLock<Option<Arc<pob_engine::tree::Tree>>> = RwLock::new(None);

/// A node tapped twice on the canvas, waiting for the host to apply it.
///
/// The canvas cannot toggle the node itself any more: that is an engine
/// call, and the render thread must not block on the engine mid-frame. It
/// records the tap and the host picks it up after the frame.
static PENDING_TAP: Mutex<Option<u64>> = Mutex::new(None);

/// Raise the calling thread to User-interactive QoS.
///
/// The main thread waits on the engine thread (boot, every blocking read).
/// Left at Default, that wait is a priority inversion: the scheduler may run
/// lower work ahead of the thread the UI is stuck on.
#[cfg(target_vendor = "apple")]
fn run_at_ui_qos() {
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos: u32, relative_priority: std::ffi::c_int) -> std::ffi::c_int;
    }
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
}

#[cfg(not(target_vendor = "apple"))]
fn run_at_ui_qos() {}

fn engine() -> &'static EngineThread {
    ENGINE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        // PoB's Lua recurses (mod parsing, calc breakdowns) deeper than a
        // spawned thread's default stack allows for; the main thread it
        // used to run on has 8 MB on a Mac and 1 MB on iOS, so give it more
        // than either.
        std::thread::Builder::new()
            .name("pob-engine".into())
            .stack_size(32 << 20)
            .spawn(move || {
                run_at_ui_qos();
                let mut state: Option<pob_core::build_state::BuildState> = None;
                for job in rx {
                    // A panic inside PoB must not take the thread down with
                    // it: every later call would silently return defaults.
                    // Keep the thread, and say what happened.
                    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut state)));
                    if let Err(panic) = ran {
                        let what = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                            .unwrap_or_else(|| "unknown panic".to_owned());
                        if let Some(s) = state.as_mut() {
                            s.engine_error = Some(format!("The engine hit a bug: {what}"));
                        }
                    }
                    let snapshot = state.as_ref().map(|s| Arc::clone(&s.tree));
                    if let Ok(mut t) = TREE.write() {
                        *t = snapshot;
                    }
                }
            })
            .expect("spawn the engine thread");
        EngineThread { tx }
    })
}

/// Run `f` against the build on the engine thread and wait for the answer,
/// or return `default` when there is no build.
///
/// Blocking: the caller's thread waits for its turn in the queue and for
/// `f` to finish. Cheap reads take microseconds; the long operations have
/// `async` variants that let the caller's thread go.
fn with<T: Send + 'static>(
    default: T,
    f: impl FnOnce(&mut pob_core::build_state::BuildState) -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::sync_channel(1);
    let job: Job = Box::new(move |slot| {
        let out = slot.as_mut().map(f);
        let _ = tx.send(out);
    });
    if engine().tx.send(job).is_err() {
        return default;
    }
    rx.recv().ok().flatten().unwrap_or(default)
}

/// The answer to an async job, filled in by the engine thread.
struct Shared<T> {
    slot: Mutex<(Option<Option<T>>, Option<std::task::Waker>)>,
}

/// A future for an engine job. uniffi polls it from the Swift side and the
/// engine thread wakes it, so no runtime is involved.
struct Reply<T> {
    shared: Arc<Shared<T>>,
    default: Option<T>,
}

impl<T: Send + Unpin> std::future::Future for Reply<T> {
    type Output = T;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<T> {
        let this = self.get_mut();
        let answer = {
            let mut slot = this.shared.slot.lock().unwrap_or_else(|e| e.into_inner());
            match slot.0.take() {
                Some(answer) => answer,
                None => {
                    slot.1 = Some(cx.waker().clone());
                    return std::task::Poll::Pending;
                }
            }
        };
        let default = this.default.take();
        std::task::Poll::Ready(answer.or(default).expect("a Reply is polled once after it is ready"))
    }
}

/// Like `with`, without the wait: the caller's thread is free until the
/// engine has an answer. For the long operations (an import, a compare, a
/// support ranking, a trade query), so the UI keeps drawing under them.
fn with_async<T: Send + Unpin + 'static>(
    default: T,
    f: impl FnOnce(&mut pob_core::build_state::BuildState) -> T + Send + 'static,
) -> Reply<T> {
    let shared = Arc::new(Shared { slot: Mutex::new((None, None)) });
    let done = Arc::clone(&shared);
    let job: Job = Box::new(move |slot| {
        let out = slot.as_mut().map(f);
        let waker = {
            let mut s = done.slot.lock().unwrap_or_else(|e| e.into_inner());
            s.0 = Some(out);
            s.1.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    });
    if engine().tx.send(job).is_err() {
        // No engine thread: answer at once with the default.
        let mut s = shared.slot.lock().unwrap_or_else(|e| e.into_inner());
        s.0 = Some(None);
    }
    Reply { shared, default: Some(default) }
}

/// The loaded build.
///
/// A handle rather than the state itself: the engine cannot leave its thread,
/// so this object carries nothing and every call reaches the thread-local.
/// One per process, because the engine underneath allows only one.
#[derive(uniffi::Object)]
pub struct Build;

#[uniffi::export]
impl Build {
    /// Boot the engine and load a build.
    ///
    /// `data_dir` is PoB's `src/`, which on iOS is inside the app bundle —
    /// a path baked in at build time does not survive leaving the build
    /// machine.
    #[uniffi::constructor]
    pub fn new(data_dir: String, build_xml: Option<String>) -> Self {
        pob_core::build_state::set_pob_src(data_dir);
        let (tx, rx) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |slot| {
            // Drop any earlier engine first: PoB allows one per process and
            // a second boot while the first is alive is refused.
            *slot = None;
            *slot = Some(pob_core::build_state::BuildState::new(build_xml.as_deref()));
            let _ = tx.send(());
        });
        if engine().tx.send(job).is_ok() {
            let _ = rx.recv();
        }
        Self
    }

    /// A node the canvas was asked to allocate or drop, taken once. The
    /// host applies it with `toggle_node`, then refreshes and saves.
    pub fn take_pending_tap(&self) -> Option<u64> {
        PENDING_TAP.lock().ok().and_then(|mut p| p.take())
    }

    /// Allocate a node that is not, or drop one that is.
    pub fn toggle_node(&self, node_id: u64) {
        with((), move |b| b.toggle_node(node_id))
    }

    /// `toggle_node` without blocking the caller: a full recalculation
    /// follows every toggle, and the canvas should keep panning under it.
    pub async fn toggle_node_async(&self, node_id: u64) {
        with_async((), move |b| b.toggle_node(node_id)).await
    }

    /// Changes since start that came from the Rust side. Compare across
    /// frames; a difference means re-read the build.
    pub fn generation(&self) -> u64 {
        GENERATION.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Why the engine is unavailable, if it is. `None` means it is working.
    pub fn engine_error(&self) -> Option<String> {
        with(None, move |b| b.engine_error.clone())
    }

    /// PoB's own findings about the build — over-reserved mana, unmet
    /// requirements. Distinct from our warnings, which mean the app broke
    /// rather than the build being broken.
    pub fn build_warnings(&self) -> Vec<String> {
        with(Vec::new(), move |b| b.build_warnings.clone())
    }

    /// Our own problems: a failed engine, a build that would not load.
    pub fn warnings(&self) -> Vec<String> {
        with(Vec::new(), move |b| b.warnings.clone())
    }

    pub fn info(&self) -> BuildInfo {
        let empty = BuildInfo {
            level: 0,
            class: String::new(),
            ascendancy: String::new(),
            points_used: 0,
            points_max: 0,
            ascendancy_points_used: 0,
            ascendancy_points_max: 0,
        };
        with(empty, move |b| BuildInfo {
            level: b.info.level,
            class: b.info.class.clone(),
            ascendancy: b.info.ascendancy.clone(),
            points_used: b.info.points_used,
            points_max: b.info.points_max,
            ascendancy_points_used: b.info.ascendancy_points_used,
            ascendancy_points_max: b.info.ascendancy_points_max,
        })
    }

    /// PoB's classes and their ascendancies.
    pub fn classes(&self) -> Vec<ClassChoice> {
        with(Vec::new(), move |b| {
            b.classes()
                .into_iter()
                .map(|c| ClassChoice {
                    id: c.id,
                    name: c.name,
                    ascendancies: c.ascendancies.into_iter().map(|(id, name)| AscendancyChoice { id, name }).collect(),
                })
                .collect()
        })
    }

    /// True when changing to this class would unallocate the tree, which
    /// PoB warns about before doing.
    pub fn class_change_resets_tree(&self, class_id: u8) -> bool {
        with(false, move |b| b.class_change_resets_tree(class_id))
    }

    pub fn set_class(&self, class_id: u8) {
        with((), move |b| b.apply(&pob_core::build_edit::BuildEdit::Class(class_id)));
    }

    pub fn set_ascendancy(&self, ascendancy_id: u8) {
        with((), move |b| b.apply(&pob_core::build_edit::BuildEdit::Ascendancy(ascendancy_id)));
    }

    pub fn set_level(&self, level: i64) {
        with((), move |b| b.apply(&pob_core::build_edit::BuildEdit::Level(level)));
    }

    /// PoB's automatic level: the lowest level the allocated points need.
    pub fn set_level_auto(&self) {
        with((), move |b| b.apply(&pob_core::build_edit::BuildEdit::LevelAuto));
    }

    pub fn equipment(&self) -> Vec<Slot> {
        with(Vec::new(), move |b| b.equipment
            .iter()
            .map(|s| Slot {
                name: s.name.clone(),
                label: s.label.clone(),
                item: s.item.as_ref().map(|i| i.name.clone()),
                colour: s
                    .item
                    .as_ref()
                    .map(|i| hex(i.rarity.rgb()))
                    .unwrap_or_else(|| "#8a9aaf".into()),
                flask_active: s.active,
            })
            .collect())
    }

    pub fn weapon_set_second(&self) -> bool {
        with(false, move |b| b.weapon_set_second())
    }

    pub fn set_weapon_set(&self, second: bool) {
        with((), move |b| b.set_weapon_set(second));
    }

    /// PoB's unique database, by name.
    pub fn unique_catalogue(&self) -> Vec<DbItem> {
        with(Vec::new(), move |b| {
            b.unique_catalogue()
                .into_iter()
                .map(|i| DbItem {
                    name: i.name,
                    base: i.base_name,
                    item_type: i.item_type,
                    colour: hex(i.rarity.rgb()),
                    league: i.league,
                    req_level: i.req_level,
                })
                .collect()
        })
    }

    /// A database unique's item text.
    pub fn unique_raw(&self, name: String) -> Option<String> {
        with(None, move |b| b.unique_raw(&name))
    }

    /// A database unique in full, parsed by PoB for the detail card.
    pub fn unique_detail(&self, name: String) -> Option<ItemDetail> {
        with(None, move |b| {
            let raw = b.unique_raw(&name)?;
            let i = b.parse_item_text(&raw)?;
            Some(item_detail_of(&i))
        })
    }

    /// Add a database unique; optionally equip it. Returns the item's id.
    pub fn add_unique(&self, name: String, equip: bool) -> Option<i64> {
        with(None, move |b| b.add_unique(&name, equip))
    }

    pub fn craft_bases(&self) -> Vec<CraftBase> {
        with(Vec::new(), move |b| {
            b.craft_bases().into_iter().map(|c| CraftBase { item_type: c.item_type, name: c.name }).collect()
        })
    }

    /// Create an item on a base. `rarity` is NORMAL, MAGIC or RARE.
    pub fn craft_item(&self, item_type: String, base: String, rarity: String, title: String) -> Option<i64> {
        with(None, move |b| b.craft_item(&item_type, &base, &rarity, &title))
    }

    /// An item's text as PoB writes it.
    pub fn item_raw(&self, item_id: i64) -> Option<String> {
        with(None, move |b| b.item_raw(item_id))
    }

    /// Replace an item from edited text. Returns an error to show, or `None`.
    pub fn replace_item_from_text(&self, item_id: i64, text: String) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| b.replace_item_from_text(item_id, &text))
    }

    /// Parse item text with PoB's parser, without adding it anywhere.
    pub fn parse_item(&self, text: String) -> Option<ItemDetail> {
        with(None, move |b| b.parse_item_text(&text).map(|i| item_detail_of(&i)))
    }

    /// The item in a slot, in full. `None` for an empty slot.
    pub fn item_detail(&self, slot: String) -> Option<ItemDetail> {
        with(None, move |b| {
            let s = b.equipment.iter().find(|s| s.name == slot)?;
            let i = s.item.as_ref()?;
            Some(item_detail_of(i))
        })
    }
}

/// PoB's item as the detail card wants it.
fn item_detail_of(i: &pob_engine::items::Item) -> ItemDetail {
    ItemDetail {
                name: i.name.clone(),
                base: i.base_name.clone(),
                rarity: i.rarity.label().to_owned(),
                colour: hex(i.rarity.rgb()),
                item_level: i.item_level,
                quality: i.quality,
                req_level: i.req_level,
                req_str: i.req_str,
                req_dex: i.req_dex,
                req_int: i.req_int,
                mods: i
                    .mods
                    .iter()
                    .map(|m| ModText {
                        text: m.text.clone(),
                        kind: m.kind.label().to_owned(),
                        unsupported: m.unsupported,
                    })
                    .collect(),
                flavour: i.flavour.clone(),
                stats: i.stats.iter().map(|(name, value)| OptionValue { name: name.clone(), value: value.clone() }).collect(),
    }
}

#[uniffi::export]
impl Build {
    /// What equipping `item_id` in `slot` would change.
    pub fn item_preview(&self, slot: String, item_id: i64) -> Option<ItemPreview> {
        with(None, move |b| {
            b.item_preview(&slot, item_id).map(|p| ItemPreview {
                replacing: p.replacing,
                is_equipped: p.is_equipped,
                changes: p
                    .deltas
                    .into_iter()
                    .map(|d| StatChange { label: d.label, value: d.value, percent: d.percent })
                    .collect(),
            })
        })
    }

    /// The slot PoB would put an item given as text into.
    pub fn item_primary_slot(&self, text: String) -> Option<String> {
        with(None, move |b| b.item_primary_slot(&text))
    }

    /// What equipping an item given as text in `slot` would change.
    pub fn item_preview_text(&self, slot: String, text: String) -> Option<ItemPreview> {
        with(None, move |b| {
            b.item_preview_text(&slot, &text).map(|p| ItemPreview {
                replacing: p.replacing,
                is_equipped: p.is_equipped,
                changes: p.deltas.into_iter().map(|d| StatChange { label: d.label, value: d.value, percent: d.percent }).collect(),
            })
        })
    }

    /// Add an item from pasted text. Returns an error to show, or `None`.
    pub fn add_item_from_text(&self, text: String) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| b.add_item_from_text(&text))
    }

    pub fn item_list(&self) -> Vec<Item> {
        with(Vec::new(), move |b| b.item_list()
            .into_iter()
            .map(|e| Item {
                id: e.id,
                colour: hex(e.rarity.rgb()),
                name: e.name,
                worn_in: e.equipped_in,
            })
            .collect())
    }

    /// What a slot will take, by PoB's own validity rules.
    pub fn slot_candidates(&self, slot: String) -> Vec<Item> {
        with(Vec::new(), move |b| b.slot_candidates(&slot)
            .into_iter()
            .map(|e| Item {
                id: e.id,
                colour: hex(e.rarity.rgb()),
                name: e.name,
                worn_in: e.equipped_in,
            })
            .collect())
    }

    /// Put an item in a slot. An id of 0 empties it.
    pub fn equip_item(&self, slot: String, item_id: i64) {
        with((), move |b| b.equip_item(&slot, item_id));
    }

    /// Whether a tree node is allocated on the active spec.
    pub fn node_allocated(&self, node_id: u64) -> bool {
        with(false, move |b| b.tree.nodes.iter().any(|n| n.id == node_id && n.allocated))
    }

    /// True for a socket that an item grants and no point buys (the
    /// Sinister sockets, from the Voices jewel). One that is not allocated
    /// cannot be: a jewel put in it does nothing.
    pub fn socket_granted_only(&self, node_id: u64) -> bool {
        with(false, move |b| b.tree.nodes.iter().any(|n| n.id == node_id && n.granted_only))
    }

    /// Allocate a node if it is not already; a no-op otherwise. Used when a
    /// jewel is socketed from the tree: a jewel in an unallocated socket
    /// does nothing, and tapping the socket meant "use this".
    pub fn allocate_node(&self, node_id: u64) {
        with((), move |b| {
            if !b.tree.nodes.iter().any(|n| n.id == node_id && n.allocated) {
                b.toggle_node(node_id);
            }
        });
        bump_generation();
    }

    /// A mastery node's selectable effects. Non-empty only for a mastery that
    /// still has an effect to pick, so an empty result also means "not a
    /// mastery worth a picker".
    pub fn mastery_effects(&self, node_id: u64) -> Vec<MasteryEffect> {
        with(Vec::new(), move |b| {
            b.mastery_effects(node_id)
                .into_iter()
                .map(|(id, label)| MasteryEffect { id, label })
                .collect()
        })
    }

    /// The effect currently chosen for a mastery, if any.
    pub fn mastery_selection(&self, node_id: u64) -> Option<u32> {
        with(None, move |b| b.mastery_selection(node_id))
    }

    /// Choose an effect for a mastery, allocating the node if needed.
    pub fn set_mastery_effect(&self, node_id: u64, effect_id: u32) {
        with((), move |b| b.set_mastery_effect(node_id, effect_id));
        bump_generation();
    }

    /// The item slot behind a tree node, "Jewel <id>", when the node is a
    /// jewel socket that takes items; nil for every other node (charm
    /// sockets included: PoB gives them no slot).
    pub fn socket_slot(&self, node_id: u64) -> Option<String> {
        with(None, move |b| {
            b.tree
                .nodes
                .iter()
                .find(|n| n.id == node_id)
                .filter(|n| n.kind == pob_engine::tree::NodeKind::Socket && n.name != "Charm Socket")
                .map(|n| format!("Jewel {}", n.id))
        })
    }

    pub fn set_flask_active(&self, slot: String, active: bool) {
        with((), move |b| b.set_flask_active(&slot, active));
    }

    pub fn delete_item(&self, item_id: i64) {
        with((), move |b| b.delete_item(item_id));
    }

    /// PoB's sidebar stats, already formatted by PoB.
    pub fn sidebar_stats(&self) -> Vec<Stat> {
        with(Vec::new(), move |b| b.sidebar_stats()
            .into_iter()
            .filter_map(|row| match row {
                pob_engine::tree::SidebarRow::Stat(s) => Some(Stat {
                    label: s.label,
                    value: s.value,
                    colour: s
                        .colour
                        .map(|[r, g, b]| hex((r, g, b)))
                        .unwrap_or_else(|| "#e8ecf2".into()),
                }),
                // Gaps are PoB's own grouping. SwiftUI can section a list
                // properly rather than faking blank rows.
                pob_engine::tree::SidebarRow::Gap => None,
            })
            .collect())
    }

    /// The sidebar as PoB groups it, for a sectioned list.
    pub fn sidebar_sections(&self) -> Vec<StatSection> {
        with(Vec::new(), move |b| {
            let mut out: Vec<StatSection> = Vec::new();
            let mut cur: Vec<Stat> = Vec::new();
            for row in b.sidebar_stats() {
                match row {
                    pob_engine::tree::SidebarRow::Stat(s) => cur.push(Stat {
                        label: s.label,
                        value: s.value,
                        colour: s
                            .colour
                            .map(|[r, g, b]| hex((r, g, b)))
                            .unwrap_or_else(|| "#e8ecf2".into()),
                    }),
                    pob_engine::tree::SidebarRow::Gap => {
                        if !cur.is_empty() {
                            out.push(StatSection { rows: std::mem::take(&mut cur) });
                        }
                    }
                }
            }
            if !cur.is_empty() {
                out.push(StatSection { rows: cur });
            }
            out
        })
    }

    pub fn sets(&self, kind: SetKind) -> Vec<NamedSet> {
        with(Vec::new(), move |b| {
            let list = b.sets(kind.into());
            list.sets
                .iter()
                .map(|s| NamedSet {
                    id: s.id,
                    title: s.title.clone(),
                    active: s.id == list.active,
                })
                .collect()
        })
    }

    pub fn set_active_set(&self, kind: SetKind, id: i64) {
        with((), move |b| b.set_active_set(kind.into(), id));
    }

    pub fn new_set(&self, kind: SetKind, title: String) {
        with((), move |b| b.new_set(kind.into(), &title));
    }

    pub fn rename_set(&self, kind: SetKind, id: i64, title: String) {
        with((), move |b| b.rename_set(kind.into(), id, &title));
    }

    pub fn delete_set(&self, kind: SetKind, id: i64) {
        with((), move |b| b.delete_set(kind.into(), id));
    }

    /// The build's loadouts. A loadout is a tree spec, item set, skill set
    /// and config set that share a title — there is no loadout record.
    pub fn loadouts(&self) -> Vec<String> {
        with(Vec::new(), move |b| b.loadouts())
    }

    /// Which loadout is live, or `None` when the sets disagree — a build can
    /// sit between loadouts after one set is switched by hand.
    pub fn active_loadout(&self) -> Option<String> {
        with(None, move |b| b.active_loadout())
    }

    pub fn set_active_loadout(&self, name: String) {
        with((), move |b| b.set_active_loadout(&name));
    }

    pub fn new_loadout(&self, name: String) {
        with((), move |b| b.new_loadout(&name));
    }

    /// The current build as PoB's own XML, for saving.
    ///
    /// Produced by `SaveDB`, which is what PoB itself calls when exporting,
    /// so a file written here opens in PoB.
    pub fn export_xml(&self) -> Option<String> {
        with(None, move |b| b.build_xml())
    }

    /// The current build as a share code: what PoB's Generate button on the
    /// Import tab produces, and what pobb.in, poe.ninja and a Discord paste
    /// take. Deflate, then URL-safe base64.
    pub fn export_code(&self) -> Option<String> {
        with(None, move |b| b.build_xml().and_then(|xml| pob_import::encode(&xml).ok()))
    }

    /// Import a build from a pasted code or raw XML.
    ///
    /// Returns an error message to show, or `None` on success. Share-site
    /// URLs are rejected with an explanation rather than silently doing
    /// nothing: fetching one needs the network, which is its own change.
    pub fn import_paste(&self, text: String) -> Option<String> {
        match import_job(text) {
            Ok(job) => with(Some("No build loaded".to_owned()), job),
            Err(message) => Some(message),
        }
    }

    /// `import_paste` without blocking the caller. A load is the longest
    /// thing the engine does short of a support ranking.
    pub async fn import_paste_async(&self, text: String) -> Option<String> {
        match import_job(text) {
            Ok(job) => with_async(Some("No build loaded".to_owned()), job).await,
            Err(message) => Some(message),
        }
    }

    /// Import a character's passive tree and jewels from the API's
    /// `/character/{name}` JSON. Returns an error message, or nil.
    pub fn import_character_tree(&self, json: String, delete_jewels: bool) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| b.import_character_tree(&json, delete_jewels))
    }

    /// Import a character's items and skills, likewise.
    pub fn import_character_items(
        &self,
        json: String,
        clear_items: bool,
        clear_skills: bool,
        ignore_weapon_swap: bool,
    ) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| {
            b.import_character_items(&json, clear_items, clear_skills, ignore_weapon_swap)
        })
    }

    // MARK: Tree sets, versions, compare, links, reset, node power

    /// PoB's Compare summary against another build's XML. Runs the other
    /// build through the engine for its numbers, so it takes a moment.
    pub fn compare_with_xml(&self, xml: String) -> CompareOutcome {
        with(no_build_compare(), compare_xml_job(xml))
    }

    /// `compare_with_xml` without blocking the caller: the other build is
    /// loaded in place of this one and both are recalculated, seconds of work.
    pub async fn compare_with_xml_async(&self, xml: String) -> CompareOutcome {
        with_async(no_build_compare(), compare_xml_job(xml)).await
    }

    /// PoB's Compare summary against another tree set of this build.
    pub fn compare_with_spec(&self, index: u32) -> CompareOutcome {
        with(no_build_compare(), compare_spec_job(index))
    }

    /// `compare_with_spec` without blocking the caller.
    pub async fn compare_with_spec_async(&self, index: u32) -> CompareOutcome {
        with_async(no_build_compare(), compare_spec_job(index)).await
    }

    pub fn tree_specs(&self) -> Vec<TreeSpec> {
        with(Vec::new(), move |b| {
            b.tree_specs().into_iter().map(|(i, t)| TreeSpec { index: i as u32, title: t }).collect()
        })
    }

    pub fn active_spec(&self) -> u32 {
        with(1, move |b| b.active_spec() as u32)
    }

    pub fn set_active_spec(&self, index: u32) {
        with((), move |b| b.set_active_spec(index as usize));
    }

    pub fn new_spec(&self, title: String) -> Option<u32> {
        with(None, move |b| b.new_spec(&title).map(|i| i as u32))
    }

    pub fn copy_spec(&self, index: u32, title: String) -> Option<u32> {
        with(None, move |b| b.copy_spec(index as usize, &title).map(|i| i as u32))
    }

    pub fn rename_spec(&self, index: u32, title: String) {
        with((), move |b| b.rename_spec(index as usize, &title));
    }

    pub fn delete_spec(&self, index: u32) {
        with((), move |b| b.delete_spec(index as usize));
    }

    /// The active tree's version id, e.g. `3_29`.
    pub fn tree_version(&self) -> String {
        with(String::new(), move |b| b.tree.version.clone())
    }

    pub fn tree_versions(&self) -> Vec<TreeVersion> {
        with(Vec::new(), move |b| {
            b.tree_versions().into_iter().map(|(id, label)| TreeVersion { id, label }).collect()
        })
    }

    /// Returns an error to show, or `None`.
    pub fn convert_tree_version(&self, version: String) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| b.convert_tree_version(&version).err())
    }

    /// Compare the active tree against another spec (1-based), or stop.
    pub fn set_compare_spec(&self, index: Option<u32>) {
        with((), move |b| b.set_compare_spec(index.map(|i| i as usize)));
    }

    pub fn export_tree_link(&self) -> Option<String> {
        with(None, move |b| b.export_tree_link())
    }

    /// Returns an error to show, or `None`.
    pub fn import_tree_link(&self, link: String) -> Option<String> {
        with(Some("No build loaded".to_owned()), move |b| b.import_tree_link(&link).err())
    }

    pub fn undo(&self, scope: UndoScope) -> UndoState {
        with(UndoState { can_undo: false, can_redo: false }, move |b| {
            let (u, r) = b.undo_redo(scope.key(), false);
            UndoState { can_undo: u, can_redo: r }
        })
    }

    pub fn redo(&self, scope: UndoScope) -> UndoState {
        with(UndoState { can_undo: false, can_redo: false }, move |b| {
            let (u, r) = b.undo_redo(scope.key(), true);
            UndoState { can_undo: u, can_redo: r }
        })
    }

    pub fn undo_state(&self, scope: UndoScope) -> UndoState {
        with(UndoState { can_undo: false, can_redo: false }, move |b| {
            let (u, r) = b.undo_state(scope.key());
            UndoState { can_undo: u, can_redo: r }
        })
    }

    /// One of PoB's application options (`main.*`), as text.
    pub fn main_option(&self, name: String) -> Option<String> {
        with(None, move |b| b.main_option(&name))
    }

    /// Set one of PoB's application options; "true"/"false", a number, or text.
    pub fn set_main_option(&self, name: String, value: String) {
        with((), move |b| b.set_main_option(&name, &value));
    }

    /// The Skills tab's per-build gem defaults.
    pub fn skills_options(&self) -> Vec<OptionValue> {
        with(Vec::new(), move |b| b.skills_options().into_iter().map(|(name, value)| OptionValue { name, value }).collect())
    }

    pub fn set_skills_option(&self, name: String, value: String) {
        with((), move |b| b.set_skills_option(&name, &value));
    }

    pub fn reset_tree(&self) {
        with((), move |b| b.reset_tree());
    }

    /// Start ranking nodes for `stat`. Then call `node_power_step` until it
    /// returns true, a few steps per frame, and read `node_power_results`.
    pub fn node_power_start(&self, stat: String) -> bool {
        with(false, move |b| b.node_power_start_for(&stat))
    }

    /// One node's worth of work. True when the ranking is finished.
    pub fn node_power_step(&self) -> bool {
        with(true, move |b| b.node_power_step())
    }

    /// Abandon a ranking in progress and restore PoB's clock.
    pub fn node_power_cancel(&self) {
        with((), move |b| b.node_power_cancel())
    }

    /// PoB's node-power measures.
    pub fn node_power_stats(&self) -> Vec<PowerStat> {
        with(Vec::new(), move |b| {
            b.node_power_stats().into_iter().map(|s| PowerStat { stat: s.stat, label: s.label }).collect()
        })
    }

    /// PoB's Power Report for the finished ranking, best first.
    pub fn node_power_report(&self) -> Vec<PowerReportRow> {
        with(Vec::new(), move |b| {
            b.node_power_report()
                .into_iter()
                .map(|r| PowerReportRow {
                    id: r.id, name: r.name, kind: r.kind, power: r.power, power_text: r.power_text,
                    per_point_text: r.per_point_text, points_text: r.points_text,
                    allocated: r.allocated, lower_is_better: r.lower_is_better,
                })
                .collect()
        })
    }

    /// Percent complete of a ranking in progress.
    pub fn node_power_progress(&self) -> u32 {
        with(0, move |b| b.node_power_progress())
    }

    /// The finished ranking, best first, with names for the report.
    pub fn node_power_results(&self) -> Vec<NodePower> {
        with(Vec::new(), move |b| {
            let names: std::collections::HashMap<u64, (&str, bool)> =
                b.tree.nodes.iter().map(|n| (n.id, (n.name.as_str(), n.allocated))).collect();
            let mut out: Vec<NodePower> = b
                .node_power_results()
                .into_iter()
                .map(|(id, value)| {
                    let (name, allocated) = names.get(&id).copied().unwrap_or(("", false));
                    NodePower { id, name: name.to_owned(), value, allocated }
                })
                .collect();
            out.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
            out
        })
    }

    // MARK: Find Timeless Jewel

    /// The search settings PoB saves with the build.
    pub fn timeless_state(&self) -> Option<TimelessState> {
        with(None, move |b| {
            let s = b.timeless_state();
            Some(TimelessState {
                jewel_type: s.jewel_type,
                conqueror: s.conqueror,
                devotion1: s.devotion1,
                devotion2: s.devotion2,
                socket: s.socket,
                socket_filter: s.socket_filter,
                socket_distance: s.socket_distance,
                desired: desired_out(s.desired),
                fallback: desired_out(s.fallback),
                fallback_mode: s.fallback_mode,
                total_minimum_weight: s.total_minimum_weight,
            })
        })
    }

    pub fn set_timeless_state(&self, state: TimelessState) {
        with((), move |b| {
            b.set_timeless_state(&pob_engine::timeless::TimelessState {
                jewel_type: state.jewel_type,
                conqueror: state.conqueror,
                devotion1: state.devotion1,
                devotion2: state.devotion2,
                socket: state.socket,
                socket_filter: state.socket_filter,
                socket_distance: state.socket_distance,
                desired: desired_in(&state.desired),
                fallback: desired_in(&state.fallback),
                fallback_mode: state.fallback_mode,
                total_minimum_weight: state.total_minimum_weight,
            })
        })
    }

    /// What the dialog offers for the stored settings' jewel type and socket.
    pub fn timeless_options(&self) -> Option<TimelessOptions> {
        with(None, move |b| {
            let o = b.timeless_options();
            let choices = |v: Vec<pob_engine::timeless::Conqueror>| {
                v.into_iter().map(|c| TimelessChoice { id: c.id, label: c.label }).collect()
            };
            Some(TimelessOptions {
                jewel_types: o
                    .jewel_types
                    .into_iter()
                    .map(|j| TimelessChoice { id: j.id, label: j.label })
                    .collect(),
                conquerors: choices(o.conquerors),
                devotion_variants: choices(o.devotion_variants),
                sockets: o
                    .sockets
                    .into_iter()
                    .map(|s| TimelessSocket {
                        id: s.id,
                        label: s.label,
                        keystone: s.keystone,
                        allocated: s.allocated,
                    })
                    .collect(),
                nodes: legion_nodes_out(o.nodes),
                fallback_modes: o.fallback_modes,
                protect_candidates: o.protect_candidates,
                abyss_ascendancies: legion_nodes_out(o.abyss_ascendancies),
            })
        })
    }

    /// Run PoB's search with the stored settings. Blocks for the search:
    /// well under a second for one socket.
    pub fn timeless_search(&self, protected: Vec<String>) -> TimelessSearch {
        with(TimelessSearch { desired: Vec::new(), results: Vec::new() }, move |b| {
            search_out(b.timeless_search(&protected))
        })
    }

    /// `timeless_search` without blocking the caller.
    pub async fn timeless_search_async(&self, protected: Vec<String>) -> TimelessSearch {
        with_async(TimelessSearch { desired: Vec::new(), results: Vec::new() }, move |b| {
            search_out(b.timeless_search(&protected))
        })
        .await
    }

    /// The last search's results, with `added` up to date.
    pub fn timeless_results(&self) -> TimelessSearch {
        with(TimelessSearch { desired: Vec::new(), results: Vec::new() }, move |b| {
            search_out(b.timeless_results())
        })
    }

    /// PoB's "Generate" for fallback weights. Runs a calculation per node.
    pub fn timeless_generate_fallback(&self) -> Vec<DesiredNode> {
        with(Vec::new(), move |b| desired_out(b.timeless_generate_fallback()))
    }

    /// `timeless_generate_fallback` without blocking the caller: a
    /// recalculation per candidate node.
    pub async fn timeless_generate_fallback_async(&self) -> Vec<DesiredNode> {
        with_async(Vec::new(), move |b| desired_out(b.timeless_generate_fallback())).await
    }

    /// The trade-site URL for results from `start` (0-based). `realm` is
    /// 0 PC, 1 Sony, 2 Xbox; `buyout` PoB's five listing filters.
    pub fn timeless_trade_url(
        &self,
        start: u32,
        scope: u8,
        league: String,
        realm: u8,
        buyout: u8,
    ) -> Option<String> {
        with(None, move |b| b.timeless_trade_url(start as usize, scope, &league, realm, buyout))
    }

    /// PoB's weighted "Find best" search for one gear slot. `weights` is the
    /// sort-stat list (empty means PoB's default of Full DPS + Effective Hit
    /// Pool); `buyout` is the 0-based index into PoB's trade-status list
    /// (0 = "Instant buyout" .. 4 = "Any"). Returns the query JSON, or PoB's
    /// message when nothing is worth searching for.
    pub fn trade_query(&self, slot: String, weights: Vec<StatWeight>, buyout: u8) -> TradeResult {
        with(TradeResult { query: None, error: Some("No build is loaded.".into()) }, trade_job(slot, weights, buyout))
    }

    /// `trade_query` without blocking the caller: PoB's generator drains up
    /// to two thousand coroutine frames.
    pub async fn trade_query_async(&self, slot: String, weights: Vec<StatWeight>, buyout: u8) -> TradeResult {
        with_async(TradeResult { query: None, error: Some("No build is loaded.".into()) }, trade_job(slot, weights, buyout)).await
    }

    /// Add the jewel from result `index` (0-based) to the build's items.
    /// The socket label the jewel was socketed into, "" if it was only
    /// added to the item list, nil if nothing happened.
    pub fn timeless_add_to_build(&self, index: u32) -> Option<String> {
        with(None, move |b| b.timeless_add_to_build(index as usize))
    }

    /// Node ids matching a search, using PoB's own matcher.
    ///
    /// An empty query returns nothing, which the canvas reads as "no search"
    /// — distinct from a search that matched nothing, which dims everything
    /// and is the honest answer to "no results".
    pub fn search_nodes(&self, query: String) -> Vec<u64> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        with(Vec::new(), move |b| b.search_nodes(&query))
    }

    pub fn skill_groups(&self) -> Vec<SkillGroup> {
        with(Vec::new(), move |b| {
            let main = b.main_socket_group;
            b.socket_groups
                .iter()
                .enumerate()
                .map(|(i, g)| SkillGroup {
                    is_main: i == main,
                    label: g.display_label(),
                    slot: g.slot.clone(),
                    enabled: g.enabled,
                    granted: g.is_granted(),
                    full_dps: g.include_in_full_dps,
                    item_sockets: g.item_sockets.clone(),
                    gems: g
                        .gems
                        .iter()
                        .map(|gem| Gem {
                            name: if gem.name.is_empty() {
                                gem.skill_id.clone()
                            } else {
                                gem.name.clone()
                            },
                            level: gem.level,
                            quality: gem.quality,
                            effective_quality: gem.effective_quality,
                            enabled: gem.enabled,
                            count: gem.count,
                            is_support: gem.is_support(),
                            colour: gem
                                .color
                                .map(|c| hex(c.rgb()))
                                .unwrap_or_else(|| "#e6ecf3".into()),
                        })
                        .collect(),
                })
                .collect()
        })
    }

    /// Change a gem's level, quality, enabled state or count. Indices are
    /// 1-based, as PoB's own are.
    pub fn set_gem(
        &self,
        group: u32,
        gem: u32,
        level: Option<i64>,
        quality: Option<i64>,
        enabled: Option<bool>,
        count: Option<i64>,
    ) {
        with((), move |b| {
            b.set_gem(group as usize, gem as usize, level, quality, enabled, count)
        });
    }

    pub fn set_group_enabled(&self, group: u32, enabled: bool) {
        with((), move |b| b.set_group_enabled(group as usize, enabled));
    }

    pub fn main_skill_controls(&self) -> MainSkillControls {
        with(
            MainSkillControls {
                active_skills: vec![], active_index: 1, parts: vec![], part_index: 1,
                stages: None, mines: None, minions: vec![], minion_values: vec![],
                minion_index: 1, minion_by_item_set: false, minion_skills: vec![], minion_skill_index: 1,
            },
            move |b| {
                let c = b.main_skill_controls();
                MainSkillControls {
                    active_skills: c.active_skills,
                    active_index: c.active_index as u32,
                    parts: c.parts,
                    part_index: c.part_index as u32,
                    stages: c.stages,
                    mines: c.mines,
                    minions: c.minions,
                    minion_values: c.minion_values,
                    minion_index: c.minion_index as u32,
                    minion_by_item_set: c.minion_by_item_set,
                    minion_skills: c.minion_skills,
                    minion_skill_index: c.minion_skill_index as u32,
                }
            },
        )
    }

    pub fn set_main_active_skill(&self, index: u32) {
        with((), move |b| b.set_main_active_skill(index as usize));
    }
    pub fn set_main_skill_part(&self, index: u32) {
        with((), move |b| b.set_main_skill_part(index as usize));
    }
    pub fn set_main_skill_stages(&self, count: i64) {
        with((), move |b| b.set_main_skill_stages(count));
    }
    pub fn set_main_skill_mines(&self, count: i64) {
        with((), move |b| b.set_main_skill_mines(count));
    }
    pub fn set_main_skill_minion(&self, value: String, by_item_set: bool) {
        with((), move |b| b.set_main_skill_minion(&value, by_item_set));
    }
    pub fn set_main_skill_minion_skill(&self, index: u32) {
        with((), move |b| b.set_main_skill_minion_skill(index as usize));
    }

    /// Add an empty socket group; returns its 1-based index.
    pub fn new_socket_group(&self) -> Option<u32> {
        with(None, move |b| b.new_socket_group().map(|i| i as u32))
    }

    /// Delete a socket group. False when PoB refuses (granted by an item).
    pub fn delete_socket_group(&self, group: u32) -> bool {
        with(false, move |b| b.delete_socket_group(group as usize))
    }

    /// Socket a group in a slot by PoB's slot name, or `None` for none.
    pub fn set_group_slot(&self, group: u32, slot: Option<String>) {
        with((), move |b| b.set_group_slot(group as usize, slot.as_deref()));
    }

    /// Make a group the main skill. 1-based.
    pub fn set_main_group(&self, group: u32) {
        with((), move |b| b.set_main_group(group as usize));
    }

    pub fn set_group_full_dps(&self, group: u32, include: bool) {
        with((), move |b| b.set_group_full_dps(group as usize, include));
    }

    pub fn remove_gem(&self, group: u32, gem: u32) {
        with((), move |b| b.remove_gem(group as usize, gem as usize));
    }

    /// Put a gem in a slot, or add one when `gem` is one past the end.
    /// Supports ranked by the DPS they would add to a socket group, best
    /// first, with their names and colours filled in from the catalogue.
    /// A real calc per gem, so the caller shows a spinner.
    pub fn rank_supports_for_group(&self, group: u32) -> Vec<RankedGem> {
        with(Vec::new(), rank_job(group))
    }

    /// `rank_supports_for_group` without blocking the caller: one full
    /// recalculation per support gem, the longest thing the engine does.
    pub async fn rank_supports_for_group_async(&self, group: u32) -> Vec<RankedGem> {
        with_async(Vec::new(), rank_job(group)).await
    }

    /// A socketed gem's description text, for tapping it open.
    pub fn gem_description(&self, group: u32, gem: u32) -> String {
        with(String::new(), move |b| b.gem_description(group as usize, gem as usize))
    }

    pub fn set_gem_id(&self, group: u32, gem: u32, gem_id: String) {
        with((), move |b| b.set_gem_id(group as usize, gem as usize, &gem_id));
    }

    /// Every gem PoB knows, for the add-gem search.
    pub fn gem_catalogue(&self) -> Vec<GemChoice> {
        with(Vec::new(), move |b| {
            b.gem_catalogue()
                .into_iter()
                .map(|g| GemChoice {
                    id: g.id,
                    name: g.name,
                    is_support: g.support,
                    colour: g.color.map(|c| hex(c.rgb())).unwrap_or_else(|| "#e6ecf3".into()),
                })
                .collect()
        })
    }

    /// The Calcs tab's sections, with every cell already formatted by PoB
    /// rather than by a reimplementation of its format language.
    pub fn calc_sections(&self) -> Vec<CalcSection> {
        with(Vec::new(), move |b| {
            b.calc_sections
                .iter()
                .filter(|s| !s.rows.is_empty())
                .map(|s| CalcSection {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    rows: s
                        .rows
                        .iter()
                        .map(|r| CalcRow {
                            label: r.label.clone(),
                            cells: r.cells.iter().map(|c| CalcCell { value: c.value.clone(), has_breakdown: c.has_breakdown }).collect(),
                            is_header: r.is_header,
                        })
                        .collect(),
                    column: s.column,
                    colour: s.colour.clone(),
                    extra: s.extra.clone(),
                    collapsed_default: s.collapsed_default,
                    section_index: s.section_index as u32,
                    sub_index: s.sub_index as u32,
                })
                .collect()
        })
    }

    /// Another build's Calcs sections, for the Compare tab. Loads it in
    /// place of the current build for the read, then restores.
    pub fn calc_sections_for(&self, xml: String) -> Vec<CalcSection> {
        with(Vec::new(), calc_sections_job(xml))
    }

    /// `calc_sections_for` without blocking the caller: loads the other
    /// build in place of this one for the read, then restores it.
    pub async fn calc_sections_for_async(&self, xml: String) -> Vec<CalcSection> {
        with_async(Vec::new(), calc_sections_job(xml)).await
    }

    /// PoB's breakdown for a cell. `row` and `cell` are 1-based within the
    /// section as PoB lists it (header rows count).
    pub fn calc_breakdown(&self, section_index: u32, sub_index: u32, row: u32, cell: u32) -> Vec<BreakdownPart> {
        with(Vec::new(), move |b| {
            b.calc_breakdown(section_index as usize, sub_index as usize, row as usize, cell as usize)
                .into_iter()
                .map(|p| BreakdownPart { label: p.label, footer: p.footer, lines: p.lines, columns: p.columns, rows: p.rows })
                .collect()
        })
    }

    pub fn calcs_inputs(&self) -> CalcsInputs {
        with(
            CalcsInputs {
                groups: vec![], group_index: 1, buff_mode: "EFFECTIVE".into(), has_minion: false, show_minion: false,
                controls: MainSkillControls {
                    active_skills: vec![], active_index: 1, parts: vec![], part_index: 1, stages: None, mines: None,
                    minions: vec![], minion_values: vec![], minion_index: 1, minion_by_item_set: false,
                    minion_skills: vec![], minion_skill_index: 1,
                },
            },
            move |b| {
                let i = b.calcs_inputs();
                let c = i.controls;
                CalcsInputs {
                    groups: i.groups,
                    group_index: i.group_index as u32,
                    buff_mode: i.buff_mode,
                    has_minion: i.has_minion,
                    show_minion: i.show_minion,
                    controls: MainSkillControls {
                        active_skills: c.active_skills, active_index: c.active_index as u32,
                        parts: c.parts, part_index: c.part_index as u32, stages: c.stages, mines: c.mines,
                        minions: c.minions, minion_values: c.minion_values, minion_index: c.minion_index as u32,
                        minion_by_item_set: c.minion_by_item_set,
                        minion_skills: c.minion_skills, minion_skill_index: c.minion_skill_index as u32,
                    },
                }
            },
        )
    }

    pub fn set_calcs_group(&self, index: u32) {
        with((), move |b| b.set_calcs_group(index as usize));
    }
    pub fn set_calcs_active_skill(&self, index: u32) {
        with((), move |b| b.set_calcs_active_skill(index as usize));
    }
    pub fn set_calcs_part(&self, index: u32) {
        with((), move |b| b.set_calcs_part(index as usize));
    }
    pub fn set_calcs_stages(&self, count: i64) {
        with((), move |b| b.set_calcs_stages(count));
    }
    pub fn set_calcs_mines(&self, count: i64) {
        with((), move |b| b.set_calcs_mines(count));
    }
    pub fn set_calcs_minion(&self, value: String, by_item_set: bool) {
        with((), move |b| b.set_calcs_minion(&value, by_item_set));
    }
    pub fn set_calcs_minion_skill(&self, index: u32) {
        with((), move |b| b.set_calcs_minion_skill(index as usize));
    }
    /// UNBUFFED, BUFFED, COMBAT or EFFECTIVE.
    pub fn set_calcs_buff_mode(&self, mode: String) {
        with((), move |b| b.set_calcs_buff_mode(&mode));
    }
    pub fn set_calcs_show_minion(&self, show: bool) {
        with((), move |b| b.set_calcs_show_minion(show));
    }

    pub fn config_options(&self) -> Vec<ConfigOption> {
        use pob_engine::config::{OptionKind, Value};
        with(Vec::new(), move |b| {
            b.config_options
                .iter()
                .map(|o| ConfigOption {
                    var: o.var.clone(),
                    label: o.label.clone(),
                    section: o.section.clone(),
                    kind: match o.kind {
                        OptionKind::Check => ConfigKind::Check,
                        OptionKind::Number { .. } => ConfigKind::Number,
                        OptionKind::List { .. } => ConfigKind::List,
                    },
                    value: match &o.value {
                        Value::Bool(v) => v.to_string(),
                        Value::Number(n) => n.to_string(),
                        Value::Text(t) => t.clone(),
                        Value::Unset => String::new(),
                    },
                    choices: match &o.kind {
                        OptionKind::List { options } => options
                            .iter()
                            .map(|(key, label)| ConfigChoice {
                                key: key.clone(),
                                label: label.clone(),
                            })
                            .collect(),
                        _ => Vec::new(),
                    },
                    relevant: o.relevant,
                    tooltip: o.tooltip.clone(),
                })
                .collect()
        })
    }

    pub fn custom_mods(&self) -> Vec<CustomModBlock> {
        with(Vec::new(), move |b| {
            b.custom_mods().into_iter().map(|m| CustomModBlock { title: m.title, enabled: m.enabled, text: m.text }).collect()
        })
    }

    pub fn add_custom_mod_block(&self) {
        with((), move |b| b.add_custom_mod_block());
    }

    /// 1-based.
    pub fn remove_custom_mod_block(&self, index: u32) {
        with((), move |b| b.remove_custom_mod_block(index as usize));
    }

    /// 1-based.
    pub fn set_custom_mod_block(&self, index: u32, title: String, enabled: bool, text: String) {
        with((), move |b| b.set_custom_mod_block(index as usize, &title, enabled, &text));
    }

    /// Set a config option. An empty `value` clears it, which for PoB means
    /// restoring its default rather than setting zero or false.
    pub fn set_config(&self, var: String, value: String, kind: ConfigKind) {
        use pob_engine::config::Value;
        let parsed = if value.is_empty() {
            Value::Unset
        } else {
            match kind {
                ConfigKind::Check => Value::Bool(value == "true"),
                ConfigKind::Number => {
                    value.parse().map(Value::Number).unwrap_or(Value::Unset)
                }
                ConfigKind::List => Value::Text(value),
            }
        };
        with((), move |b| b.set_config(&var, parsed.clone()));
    }

    pub fn set_notes(&self, text: String) {
        with((), move |b| b.set_notes(text.clone()));
    }

    pub fn notes(&self) -> String {
        with(String::new(), move |b| b.notes.clone())
    }
}

/// Where a node sits in tree units, for the host to centre on it.
pub fn node_position(id: u64) -> Option<(f32, f32)> {
    let tree = TREE.read().ok()?.clone()?;
    tree.nodes.iter().find(|n| n.id == id).map(|n| (n.x, n.y))
}

/// Draw the passive tree into an egui context the caller owns.
///
/// The one place the two halves of the split meet. SwiftUI owns the chrome
/// and this crate owns the engine; the canvas draws from the engine's tree
/// snapshot rather than the egui host booting a second engine of its own,
/// which it did at first, and which the engine's own guard caught:
/// "an engine is already running in this process".
pub fn draw_tree(
    ui: &mut egui::Ui,
    view: &mut pob_core::tree_view::View,
    art: &mut pob_core::tree_art::TreeArt,
    pinned: &mut Option<u64>,
    matches: &std::collections::HashSet<u64>,
    searching: bool,
    power: &std::collections::HashMap<u64, f32>,
) {
    // The snapshot for this frame. Taken once so a reload landing mid-frame
    // cannot change the tree under the canvas.
    let tree = TREE.read().ok().and_then(|t| t.clone());
    let Some(tree) = tree else {
        ui.centered_and_justified(|ui| {
            ui.label("No build loaded");
        });
        return;
    };
    if tree.nodes.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("No passive tree loaded");
        });
        return;
    }
    let result = pob_core::tree_view::TreeCanvas {
        tree: &tree,
        view,
        art,
        matches,
        searching,
        power,
        tooltip_scrollable: false,
        pinned: *pinned,
    }
    .show(ui);

    // Tap to pin a node's tooltip, tap it again or tap empty space to
    // dismiss. Hover cannot work here: a finger that has lifted hovers
    // nothing, so a hover-driven tooltip only exists while you are
    // covering the thing you are trying to read.
    if result.response.clicked() {
        match result.clicked {
            // Tapping the pinned node again allocates it. The first tap
            // shows you what it does, the second commits — which is the
            // right order on a touch screen, where you cannot look before
            // you touch. The engine is on another thread, so the tap is
            // recorded here and applied by the host after the frame.
            Some(node) if *pinned == Some(node) => {
                if let Ok(mut p) = PENDING_TAP.lock() {
                    *p = Some(node);
                }
                *pinned = None;
            }
            Some(node) => *pinned = Some(node),
            None => *pinned = None,
        }
    }

    if let Some(node) = result.hovered {
        show_node_tooltip(ui, &tree, art, node);
    }
}

/// PoB2's tooltip header: a band of art across the top of the tooltip with
/// the title on it. Returns false when the art is not there, and the caller
/// draws a plain title instead.
fn draw_tooltip_header(
    ui: &mut egui::Ui,
    art: &mut pob_core::tree_art::TreeArt,
    header: pob_core::tree_art::TooltipHeader,
    width: f32,
    title: egui::RichText,
) -> bool {
    let ctx = ui.ctx().clone();
    let Some(left) = art.get(&ctx, header.left).map(|t| t.id()) else { return false };
    let Some(middle) = art.get(&ctx, header.middle).map(|t| t.id()) else { return false };
    let Some(right) = art.get(&ctx, header.right).map(|t| t.id()) else { return false };

    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, header.height), egui::Sense::hover());
    // PoB2 runs the band out to a point inside the tooltip's border and
    // keeps the text six in, so the lines sit inset from the band's ends.
    // The frame's margin is that six here; the band takes it back.
    let margin = ui.style().spacing.window_margin;
    let rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() - (margin.left as f32 - 1.0), rect.top()),
        egui::pos2(rect.right() + (margin.right as f32 - 1.0), rect.bottom()),
    );
    let painter = ui.painter();
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let band = |x: f32, w: f32| egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, header.height));
    painter.image(left, band(rect.left(), header.side), uv, egui::Color32::WHITE);
    // The middle tiles, and PoB2 squeezes one more tile into what is left.
    let end = rect.right() - header.side;
    let mut x = rect.left() + header.side;
    while x + header.tile <= end {
        painter.image(middle, band(x, header.tile), uv, egui::Color32::WHITE);
        x += header.tile;
    }
    if end > x {
        painter.image(middle, band(x, end - x), uv, egui::Color32::WHITE);
    }
    painter.image(right, band(end, header.side), uv, egui::Color32::WHITE);

    let galley = egui::WidgetText::from(title).into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width() - 2.0 * header.side, egui::TextStyle::Body);
    let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + header.text_offset + (header.height - 2.0 * header.text_offset - galley.size().y) / 2.0);
    painter.galley(at, galley, egui::Color32::WHITE);
    true
}

/// The node tooltip: what the node does, and what taking it would change.
fn show_node_tooltip(
    ui: &mut egui::Ui,
    tree: &pob_engine::tree::Tree,
    art: &mut pob_core::tree_art::TreeArt,
    node_id: u64,
) {
    let Some(node) = tree.nodes.iter().find(|n| n.id == node_id) else {
        return;
    };
    // A filled socket shows its jewel, as PoB's AddNodeTooltip does in place
    // of the socket's own name.
    let name = match &node.socket_jewel {
        Some(jewel) if jewel.active => jewel.name.clone(),
        _ => node.name.clone(),
    };
    // A mastery's text lives in its chosen effect, not in `stats` — reading
    // only the latter showed masteries as a bare name with nothing under it.
    let stats = match (&node.socket_jewel, &node.mastery_choice) {
        (Some(jewel), _) if jewel.active => jewel.lines.clone(),
        // Held but not in effect: say so, and what it is.
        (Some(jewel), _) => vec![format!("Holds {}. Allocate the socket to use it.", jewel.name)],
        (None, Some(choice)) => choice.stats.clone(),
        (None, None) => node.stats.clone(),
    };
    let unchosen = node.kind == pob_engine::tree::NodeKind::Mastery
        && node.mastery_choice.is_none();

    egui::Area::new(egui::Id::new("node_tooltip"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(12.0, 12.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(360.0);
                let mut title = egui::RichText::new(&name).strong().size(14.0);
                if let Some(jewel) = &node.socket_jewel {
                    let [r, g, b] = jewel.name_colour();
                    title = title.color(egui::Color32::from_rgb(r, g, b));
                }
                // The header spans the tooltip, which is as wide as its
                // widest line, or the title plus PoB2's 50 of margin. Lines
                // wider than the tooltip's limit wrap, so they count at it.
                let measure = |text: &str, size: f32| {
                    ui.fonts_mut(|f| {
                        f.layout_no_wrap(text.to_owned(), egui::FontId::proportional(size), egui::Color32::WHITE)
                            .size()
                            .x
                    })
                };
                let widest = stats.iter().map(|l| measure(l, 12.0)).fold(0.0f32, f32::max);
                let width = (measure(&name, 14.0) + 50.0).max(widest).min(360.0);
                let header = pob_core::tree_art::tooltip_header(node.kind, node.ascendancy.is_some());
                if draw_tooltip_header(ui, art, header, width, title.clone()) {
                    ui.set_min_width(width);
                } else {
                    ui.label(title);
                }
                if unchosen {
                    ui.label(
                        egui::RichText::new("No effect chosen")
                            .size(12.0)
                            .italics(),
                    );
                }
                for line in &stats {
                    ui.label(egui::RichText::new(line).size(12.0));
                }
                // What the jewel does to the passives around it, the whole
                // trade in one place rather than a tap per node.
                if let Some(jewel) = &node.socket_jewel
                    && jewel.active
                    && !jewel.changes.is_empty()
                {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Passives in radius").size(12.0).strong());
                    for c in jewel.changes.iter().filter(|c| !c.small) {
                        let colour = if c.allocated { egui::Color32::from_gray(220) } else { egui::Color32::from_gray(120) };
                        let text = if c.replaced {
                            format!("{} → {}", c.from, c.to)
                        } else {
                            format!("{}: +{}", c.from, c.to)
                        };
                        let text = if c.allocated { text } else { format!("{text}  (not taken)") };
                        ui.label(egui::RichText::new(text).size(12.0).color(colour));
                    }
                    // Small passives, summarised: "+4 to Strength on 12 small
                    // passives (7 taken)", one line per distinct effect.
                    let mut effects: Vec<(String, usize, usize)> = Vec::new();
                    for c in jewel.changes.iter().filter(|c| c.small) {
                        match effects.iter_mut().find(|(t, _, _)| *t == c.to) {
                            Some(e) => { e.1 += 1; if c.allocated { e.2 += 1 } }
                            None => effects.push((c.to.clone(), 1, usize::from(c.allocated))),
                        }
                    }
                    for (to, n, taken) in effects {
                        let colour = if taken > 0 { egui::Color32::from_gray(200) } else { egui::Color32::from_gray(120) };
                        ui.label(egui::RichText::new(format!("{to} on {n} small passives ({taken} taken)")).size(12.0).color(colour));
                    }
                }
                // A socket's tooltip explains the radius rings the tap drew:
                // PoB shows them on hover without saying what each colour is.
                if node.kind == pob_engine::tree::NodeKind::Socket && !node.no_radius {
                    let annular = node.socket_jewel.as_ref().is_some_and(|j| j.variable_radius);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Rings: jewel radii from this socket").size(11.0).italics());
                    ui.horizontal_wrapped(|ui| {
                        for r in tree.jewel_radii.iter().filter(|r| if annular { r.inner != 0.0 } else { r.inner == 0.0 }) {
                            let [cr, cg, cb] = r.colour;
                            let label = if annular { format!("{}–{}", r.inner as i32, r.outer as i32) } else { r.label.clone() };
                            ui.label(egui::RichText::new(label).size(11.0).color(egui::Color32::from_rgb(cr, cg, cb)));
                        }
                    });
                }
            });
        });
}


// MARK: Jobs shared by a blocking method and its async twin.

type BuildJob<T> = Box<dyn FnOnce(&mut pob_core::build_state::BuildState) -> T + Send>;

/// A comparison's rows, or why there are none. The engine's own message
/// reaches the screen; "could not compare" on its own helps nobody.
#[derive(uniffi::Record)]
pub struct CompareOutcome {
    pub rows: Vec<CompareRow>,
    pub error: Option<String>,
}

fn no_build_compare() -> CompareOutcome {
    CompareOutcome { rows: Vec::new(), error: Some("No build loaded".into()) }
}

/// Parse a paste on the caller's thread (cheap, and its errors are the
/// caller's to show), then the load itself as a job.
fn import_job(text: String) -> Result<BuildJob<Option<String>>, String> {
    let xml = match pob_import::parse_paste(&text) {
        Ok(pob_import::Pasted::Xml(xml)) => xml,
        Ok(pob_import::Pasted::Link { site, .. }) => {
            return Err(format!(
                "That is a {site} link. Open it and paste the build code \
                 itself — fetching links is not supported yet."
            ));
        }
        Err(e) => return Err(e.to_string()),
    };
    Ok(Box::new(move |b| {
        let before = b.warnings.len();
        b.load(&xml);
        // `load` reports failure by pushing a warning rather than
        // returning, so a new one means the import did not take.
        b.warnings.get(before).cloned()
    }))
}

fn compare_xml_job(xml: String) -> BuildJob<CompareOutcome> {
    Box::new(move |b| {
        let keys = b.display_stat_keys();
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        match b.stats_of(&xml, &key_refs) {
            Ok(other) => CompareOutcome { rows: compare_rows_of(b.compare_rows(&other)), error: None },
            Err(e) => CompareOutcome { rows: Vec::new(), error: Some(e) },
        }
    })
}

fn compare_spec_job(index: u32) -> BuildJob<CompareOutcome> {
    Box::new(move |b| {
        let keys = b.display_stat_keys();
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        match b.stats_for_spec(index as usize, &key_refs) {
            Ok(other) => CompareOutcome { rows: compare_rows_of(b.compare_rows(&other)), error: None },
            Err(e) => CompareOutcome { rows: Vec::new(), error: Some(e) },
        }
    })
}

fn calc_sections_job(xml: String) -> BuildJob<Vec<CalcSection>> {
    Box::new(move |b| {
            b.calc_sections_for(&xml)
                .into_iter()
                .filter(|s| !s.rows.is_empty())
                .map(|s| CalcSection {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    rows: s.rows.iter().map(|r| CalcRow {
                        label: r.label.clone(),
                        cells: r.cells.iter().map(|c| CalcCell { value: c.value.clone(), has_breakdown: false }).collect(),
                        is_header: r.is_header,
                    }).collect(),
                    column: s.column,
                    colour: s.colour.clone(),
                    extra: s.extra.clone(),
                    collapsed_default: s.collapsed_default,
                    section_index: s.section_index as u32,
                    sub_index: s.sub_index as u32,
                })
                .collect()
        })
}

fn rank_job(group: u32) -> BuildJob<Vec<RankedGem>> {
    Box::new(move |b| {
            let ranked = b.rank_gems_for_group(group as usize);
            let cat = b.gem_catalogue();
            ranked
                .into_iter()
                .filter_map(|(id, delta, socketed)| {
                    cat.iter().find(|g| g.id == id).map(|g| RankedGem {
                        id: g.id.clone(),
                        name: g.name.clone(),
                        colour: g.color.map(|c| hex(c.rgb())).unwrap_or_else(|| "#e6ecf3".into()),
                        delta,
                        socketed,
                    })
                })
                .collect()
        })
}

fn trade_job(slot: String, weights: Vec<StatWeight>, buyout: u8) -> BuildJob<TradeResult> {
    let ws: Vec<(String, f64)> = if weights.is_empty() {
        vec![("FullDPS".into(), 1.0), ("TotalEHP".into(), 0.5)]
    } else {
        weights.into_iter().map(|w| (w.stat, w.weight_mult)).collect()
    };
    Box::new(move |b| match b.trade_query(&slot, &ws, buyout) {
        Ok(query) => TradeResult { query: Some(query), error: None },
        Err(error) => TradeResult { query: None, error: Some(error) },
    })
}

/// PoB's colours arrive as RGB triples; SwiftUI wants something it can parse.
fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn compare_rows_of(rows: Vec<pob_engine::tree::CompareRow>) -> Vec<CompareRow> {
    rows.into_iter()
        .map(|r| match r {
            pob_engine::tree::CompareRow::Gap => CompareRow {
                is_gap: true, label: String::new(), key: String::new(), a: String::new(), c: String::new(),
                diff: String::new(), percent: 0.0, better: 0, colour: String::new(),
            },
            pob_engine::tree::CompareRow::Stat(s) => CompareRow {
                is_gap: false,
                label: s.label,
                key: s.key,
                a: s.a,
                c: s.c,
                diff: s.diff.unwrap_or_default(),
                percent: s.percent,
                better: s.better,
                colour: s.colour.map(|[r, g, b]| hex((r, g, b))).unwrap_or_default(),
            },
        })
        .collect()
}

#[cfg(all(test, target_vendor = "apple"))]
mod engine_thread_tests {
    use super::*;

    unsafe extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_qos_class_np(
            thread: *mut std::ffi::c_void,
            qos: *mut u32,
            relative_priority: *mut std::ffi::c_int,
        ) -> std::ffi::c_int;
    }

    /// The main thread blocks on the engine thread (boot, every `with`
    /// read). A waiter at User-interactive on a thread at Default is a
    /// priority inversion, and Xcode's Thread Performance Checker flags it.
    #[test]
    fn the_engine_thread_runs_at_the_qos_of_the_ui_that_waits_on_it() {
        let (tx, rx) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |_| {
            let mut qos = 0u32;
            let mut rel = 0;
            unsafe { pthread_get_qos_class_np(pthread_self(), &mut qos, &mut rel) };
            let _ = tx.send(qos);
        });
        engine().tx.send(job).unwrap();
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        assert_eq!(rx.recv().unwrap(), QOS_CLASS_USER_INTERACTIVE);
    }
}
