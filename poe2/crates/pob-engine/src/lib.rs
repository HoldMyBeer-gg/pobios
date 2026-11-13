//! PoB's calculation engine, embedded via mlua.
//!
//! We run upstream's own Lua rather than porting the math to Rust. That is
//! what guarantees 1:1 numbers with PoB and lets a new league be absorbed by
//! re-vendoring their `Modules/` and `Data/` instead of hand-porting a diff.
//!
//! PoB ships `src/HeadlessWrapper.lua`, which their own test suite uses to run
//! without SimpleGraphic. It `dofile`s `_SimpleGraphic.def.lua` — upstream's
//! stub of the host API — so we do not write shims ourselves. Staying on their
//! wrapper means their headless path is maintained for us.
//!
//! LuaJIT is required, not preferred: `Launch.lua` calls `jit.opt.start`, and
//! PoB targets 5.1 semantics (`collectgarbage("setpause")` is 5.1-only).

pub mod account;
pub mod calcs;
pub mod config;
pub mod items;
pub mod skills;
pub mod stats;
pub mod timeless;
pub mod tree;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, VmState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Decode PoB's `^xRRGGBB` colour escape into an RGB triple.
///
/// PoB tags stat labels with their own colour — life red, mana blue — and the
/// Compare tab uses those on the label column. Anything else (a `^N` palette
/// index, or no colour at all) yields None and the caller uses its own.
/// Remove PoB's inline colour escapes from a formatted value.
///
/// `FormatStat` embeds them — a negative value carries a red `^x` prefix, an
/// over-cap resistance a grey suffix. We colour by our own rules, so the
/// codes would otherwise render as literal text.
fn strip_pob_colours(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // ^xRRGGBB
            Some('x') | Some('X') => {
                chars.next();
                for _ in 0..6 {
                    if chars.next_if(|c| c.is_ascii_hexdigit()).is_none() {
                        break;
                    }
                }
            }
            // ^N palette index
            Some(d) if d.is_ascii_digit() => {
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

fn parse_pob_colour(code: &str) -> Option<[u8; 3]> {
    let hex = code.strip_prefix("^x")?;
    if hex.len() < 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A class and the ascendancies available to it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassChoice {
    pub id: u8,
    pub name: String,
    /// `(ascendClassId, name)`, in PoB's own order. Index 1 is "None".
    pub ascendancies: Vec<(u8, String)>,
}

/// Header facts about the loaded build, for the masthead and titles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildInfo {
    pub class: String,
    pub ascendancy: String,
    pub level: i64,
    /// Allocated passive count, as PoB counts it.
    pub allocated_nodes: usize,
    pub tree_version: String,
    /// Passive points spent and available, as PoB's own progress estimate
    /// computes them. Read rather than recomputed: the maximum folds in quest
    /// points and any `ExtraPoints` a build's items or config grant, so
    /// "99 + 23" would be wrong for a good number of real builds.
    pub points_used: u32,
    pub points_max: u32,
    /// The same, for ascendancy points.
    pub ascendancy_points_used: u32,
    pub ascendancy_points_max: u32,
    /// Whether the level tracks allocated points rather than being pinned.
    pub level_is_auto: bool,
}

impl BuildInfo {
    /// "Witch Occultist — level 100", matching the masthead in the design.
    pub fn summary(&self) -> String {
        let class = if self.ascendancy.is_empty() || self.ascendancy == "None" {
            self.class.clone()
        } else {
            format!("{} {}", self.class, self.ascendancy)
        };
        format!("{class} — level {}", self.level)
    }
}

#[derive(Debug)]
pub enum Error {
    Lua(mlua::Error),
    /// `src` was not where we expected it.
    MissingSource(PathBuf),
    /// PoB reported a failure through `promptMsg` instead of raising.
    PobStartup(String),
    /// Another engine is already alive in this process. PoB's module loading
    /// runs through a process-global working directory, so a second one
    /// corrupts the first rather than coexisting with it.
    AlreadyRunning,
    /// The XML failed the pre-load checks in `pob_import::check_xml`: too
    /// large, or shaped so that PoB's own parser would spin on it.
    Rejected(pob_import::Error),
    /// A load ran past its time or memory budget and was stopped.
    Budget { what: &'static str, reason: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lua(e) => write!(f, "lua: {e}"),
            Self::AlreadyRunning => write!(
                f,
                "an engine is already running in this process; PoB can only \
                 support one at a time. In tests, run with --test-threads=1."
            ),
            Self::MissingSource(p) => {
                write!(f, "PoB source not found at {}", p.display())
            }
            Self::PobStartup(m) => write!(f, "PoB reported: {m}"),
            Self::Rejected(e) => write!(f, "{e}"),
            Self::Budget { what, reason } => write!(f, "stopped {what}: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

/// How long a single build load may run before it is stopped. A real
/// build settles in well under a second on a Mac and a few seconds on an
/// iPad; the launch watchdog is around twenty. This catches a build that
/// PoB's own loaders would chew on for minutes.
const LOAD_BUDGET: Duration = Duration::from_secs(15);

/// How much live data the VM may hold during a load before it is stopped.
/// Jetsam on an iPad takes the app somewhere past this, without an error
/// to show. Live, not allocated: a big build allocates two to three times
/// this in garbage on the way in (a large level-85 build peaks at 530 MB and settles at
/// 226 MB), so the hook collects before it judges.
const LOAD_MEMORY_CAP: usize = 1024 * 1024 * 1024;

/// Run `f` with an instruction hook that stops the VM once `budget` has
/// elapsed or the heap passes `memory_cap`. The hook fires between Lua
/// instructions, so it bounds Lua loops (ItemsTab:Load over a million
/// items) but not a single long C call; those are kept out by
/// `pob_import::check_xml`. The hook is removed afterwards either way.
static PEAK_MEMORY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run_with_budget<T>(
    lua: &Lua,
    what: &'static str,
    budget: Duration,
    memory_cap: usize,
    f: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    // Instruction hooks only fire in the interpreter. On a Mac LuaJIT
    // compiles a hot loop into a trace within milliseconds and the hook
    // never sees it again, so the JIT is switched off for the duration.
    // On iOS it is already off (no runtime code generation), so this is a
    // no-op there and `jit.status()` reports false.
    let jit_was_on: bool = lua
        .load("if jit and jit.status() then jit.off() return true end return false")
        .eval()?;
    PEAK_MEMORY.store(0, std::sync::atomic::Ordering::Relaxed);
    let tripped: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let started = Instant::now();
    let trip = Rc::clone(&tripped);
    // Ten thousand instructions is a few microseconds; the check itself is
    // two loads and a clock read, so the overhead is below one percent.
    let last_gc: std::cell::Cell<Option<Instant>> = std::cell::Cell::new(None);
    lua.set_hook(HookTriggers::new().every_nth_instruction(10_000), move |lua, _| {
        let mut used = lua.used_memory();
        PEAK_MEMORY.fetch_max(used, std::sync::atomic::Ordering::Relaxed);
        if used > memory_cap && last_gc.get().is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
            // Garbage counts until it is collected. Collect, then judge
            // what is actually alive; at most once a second, since a full
            // collection of a heap this size costs a few hundred ms.
            let _ = lua.gc_collect();
            last_gc.set(Some(Instant::now()));
            used = lua.used_memory();
        }
        let reason = if started.elapsed() > budget {
            Some(format!("took longer than {} seconds", budget.as_secs()))
        } else if used > memory_cap {
            Some(format!("needed more than {} MB", memory_cap / (1024 * 1024)))
        } else {
            None
        };
        match reason {
            Some(r) => {
                *trip.borrow_mut() = Some(r.clone());
                Err(mlua::Error::runtime(r))
            }
            None => Ok(VmState::Continue),
        }
    })?;
    let result = f();
    lua.remove_hook();
    if jit_was_on {
        let _ = lua.load("jit.on()").exec();
    }
    match tripped.borrow_mut().take() {
        Some(reason) => Err(Error::Budget { what, reason }),
        None => result,
    }
}

/// Where the Lua shims live, written on first use.
///
/// `lua-utf8.lua` stands in for a module PoB ships only as a Windows binary.
/// It is compiled into the binary so there is nothing to bundle and no build
/// path to go stale.
fn shim_dir() -> Result<PathBuf, Error> {
    const LUA_UTF8: &str = include_str!("../lua/lua-utf8.lua");

    let dir = std::env::temp_dir().join("pobng-lua-shims");
    std::fs::create_dir_all(&dir).map_err(|_| Error::MissingSource(dir.clone()))?;
    let file = dir.join("lua-utf8.lua");
    // Rewritten every boot rather than checked: it is a few kilobytes, and a
    // stale copy from an older build would be a maddening thing to diagnose.
    std::fs::write(&file, LUA_UTF8).map_err(|_| Error::MissingSource(file))?;
    Ok(dir)
}

pub struct Engine {
    lua: Lua,
    /// Every Lua snippet this crate runs, compiled once. Keyed by the
    /// literal's address, which is stable for the life of the program and
    /// unique per literal; a `String` cannot get in here (the key needs
    /// `'static`), so a dynamic chunk is never mistaken for another.
    chunks: RefCell<HashMap<usize, mlua::Function>>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Serialises engine construction across the whole process.
///
/// `Engine::new` chdirs to PoB's `src/` and leaves it there, because PoB
/// resolves modules lazily with relative `dofile` (see the comment in `new`).
/// That directory is process-global, so two threads booting engines at once
/// interleave a chdir with another engine's module loading — which does not
/// fail cleanly. It corrupts the loading state and surfaces much later as
/// `LuaJIT ASSERT lj_opt_mem.c:61: aa_table: bad usage`, an abort with no
/// stack pointing anywhere near the cause.
///
/// This bit the test suite rather than the app: the app holds one engine, but
/// cargo runs tests on parallel threads, and each test builds its own. Making
/// the engine defend its own invariant is better than requiring every caller
/// to remember `--test-threads=1`, which would not have covered it anyway —
/// cargo also runs separate test binaries concurrently.
static BOOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set while an engine is alive, cleared when it drops.
///
/// The boot lock alone was not enough. It serialises construction, but the
/// invariant is stronger than that: PoB loads modules lazily for as long as
/// it runs, and it finds them through a process-global working directory. So
/// two engines *alive at once* corrupt each other even if neither is booting,
/// and the symptom is `LuaJIT ASSERT lj_opt_mem.c:61: aa_table: bad usage` —
/// an abort with no stack anywhere near the cause.
///
/// The app has always held exactly one. It is the test suite that breaks the
/// rule, because cargo runs tests on parallel threads and each builds its own
/// engine; adding seven tests at once was enough to make a latent race show
/// up reliably. Refusing the second engine turns silent corruption into an
/// error naming the problem.
static LIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl Engine {
    /// Boot PoB headless. `pob_src` is the upstream checkout's `src/`.
    ///
    /// PoB resolves its own modules with `dofile` on relative paths, so the
    /// process working directory has to be `src/` for the duration of the
    /// load. We restore it afterwards rather than leaving it changed under
    /// the rest of the app.
    pub fn new(pob_src: &Path) -> Result<Self, Error> {
        if !pob_src.join("HeadlessWrapper.lua").is_file() {
            return Err(Error::MissingSource(pob_src.to_path_buf()));
        }

        // Held for the whole of boot: the chdir below and the module loading
        // that depends on it must not interleave with another engine's.
        // Poisoning is irrelevant here — the lock guards a sequence, not data,
        // so a panicking booter leaves nothing for us to observe.
        let _boot = BOOT.lock().unwrap_or_else(|e| e.into_inner());

        // One at a time, for the whole lifetime rather than just for boot.
        if LIVE.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::AlreadyRunning);
        }

        // Everything but LuaJIT's `ffi`. Build XML from a paste ends up in
        // PoB's Lua, and `ffi.C` would turn any future string-eval path
        // there into native code execution; nothing PoB runs in this app
        // needs it (sha2 has a pure-Lua fallback and checks for it). `debug`
        // is kept for `debug.traceback`, which ItemsTab hands to xpcall, and
        // is pruned to that one function after boot.
        let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL_SAFE | StdLib::DEBUG, LuaOptions::default()) };

        // PoB resolves its modules with relative `dofile`/`LoadModule` calls,
        // and does so *lazily* — Modules/BuildDisplayStats.lua, for one, is
        // not loaded until a frame runs after a build is applied. So the
        // working directory has to stay at `src/` for the engine's whole
        // lifetime, not just across boot; restoring it afterwards makes those
        // later loads fail, and PoB swallows the failure into `promptMsg`
        // rather than raising, leaving a build that parses its header but has
        // an empty tree (1 node, 0 socket groups, TotalDPS 0.06).
        //
        // The working directory is process-global, which makes this the one
        // genuinely invasive thing the engine does. It is why `Engine` is not
        // `Send`/`Sync` and why callers should hold exactly one.
        std::env::set_current_dir(pob_src)
            .map_err(|_| Error::MissingSource(pob_src.to_path_buf()))?;

        // PoB vendors its pure-Lua deps (xml, dkjson, sha2, …) in
        // `runtime/lua/`, a sibling of `src/`. Upstream's CI points LUA_PATH
        // there (.github/workflows/test.yml); we set package.path in-process
        // instead so we do not depend on the caller's environment.
        //
        // Our own `lua/` dir goes first, supplying stand-ins for modules PoB
        // ships only as Windows binaries (see lua/lua-utf8.lua).
        // The shim is embedded rather than read from disk: it is one small
        // file, and a path baked in at build time does not survive leaving
        // the build machine — the iPad has no `~/code` checkout. Written
        // into the app's own writable area so `package.path` can find it.
        let shims = shim_dir()?;
        let result = lua
            .load(
                r#"
                local shims = ...
                package.path = shims .. "/?.lua;"
                    .. "../runtime/lua/?.lua;../runtime/lua/?/init.lua;"
                    .. package.path

                -- Main.lua reads arg[1] to import a build passed on the
                -- command line. A standalone interpreter defines `arg`; an
                -- embedded VM does not. Empty means "no CLI import", which
                -- is what we want.
                arg = arg or {}

                -- LuaJIT is interpreter-only on iOS: Apple forbids runtime
                -- code generation, so `LJ_HASJIT` is 0 and the `jit.opt`
                -- table does not exist. PoB calls `jit.opt.start` at
                -- Launch.lua:18 without checking, which on device fails with
                -- "attempt to index field 'opt' (a nil value)" and takes the
                -- whole engine with it.
                --
                -- Stubbed rather than patched into the vendored checkout, so
                -- upstream stays untouched. Tuning a JIT that is not running
                -- has nothing to do anyway.
                if jit and not jit.opt then
                    jit.opt = { start = function() end }
                end

                dofile("HeadlessWrapper.lua")

                -- The wrapper on `dev` keeps the launch object in the global
                -- `__mainObject__`, which everything here reads. Released
                -- versions keep it in a local; Launch.lua sets the same
                -- object as the global `launch` in both, so a release boots
                -- through that.
                __mainObject__ = __mainObject__ or launch

                -- The def stub's GetTime returns 0, which reads as "never"
                -- to every time-based yield in PoB: the node-power
                -- coroutine checks `GetTime() - start > 100` and so ran to
                -- completion in one step, blocking for the whole ranking.
                -- A real millisecond clock lets it yield as designed.
                GetTime = function() return os.clock() * 1000 end

                -- What PoB does not need on a device, taken away now that
                -- boot is done. Stubs return nil plus a message rather than
                -- vanishing, because PoB checks `res, msg` on the file ops
                -- (Main.lua's MoveFolder) and would otherwise hit "attempt
                -- to call a nil value". `loadstring` stays: PassiveTree.lua
                -- reads its tree data files through it, and those come from
                -- the bundle, not from a build.
                local function disabled(what)
                    return function() return nil, what .. " is disabled in this app" end
                end
                io.popen = disabled("io.popen")
                os.execute = disabled("os.execute")
                os.remove = disabled("os.remove")
                os.rename = disabled("os.rename")
                os.exit = function() error("os.exit is disabled in this app") end
                os.getenv = disabled("os.getenv")
                string.dump = nil
                package.loadlib = nil
                debug = { traceback = debug.traceback }
                "#,
            )
            .call::<()>(shims.to_string_lossy().as_ref());

        result?;

        Self::install_zlib(&lua)?;

        // PoB reports startup failures by setting promptMsg rather than
        // raising, so a silent boot is not proof of a good boot.
        if let Some(msg) = Self::prompt_msg(&lua)? {
            return Err(Error::PobStartup(msg));
        }

        Ok(Self { lua, chunks: RefCell::new(HashMap::new()) })
    }

    /// Real `Inflate`/`Deflate` in place of the headless stubs.
    ///
    /// PoB's SimpleGraphic runtime provides these in C, and the headless
    /// definition file replaces both with `return ""`. The timeless jewel
    /// seed tables (`Data/TimelessJewelData/*.zip`) are zlib streams read
    /// through `Inflate`, so with the stub every lookup came back empty,
    /// PoB logged "Missing LUT", and the jewel-finder had nothing to search.
    /// Worse, the loader caches what it inflated to a `.bin` beside the
    /// zip, so one run left a zero-byte file that later runs preferred.
    fn install_zlib(lua: &Lua) -> Result<(), Error> {
        use std::io::{Read, Write};

        let inflate = lua.create_function(|lua, data: mlua::LuaString| {
            // PoB inflates its own seed tables (tens of MB) and, through
            // its importer, whatever a user pastes. Cap it: a bomb must
            // fail like a corrupt stream, not take the process down.
            const MAX_INFLATED: u64 = 256 * 1024 * 1024;
            let bytes = data.as_bytes();
            let mut out = Vec::new();
            let mut dec = flate2::read::ZlibDecoder::new(&bytes[..]).take(MAX_INFLATED + 1);
            match dec.read_to_end(&mut out) {
                Ok(_) if out.len() as u64 > MAX_INFLATED => {
                    Ok((None, Some("inflated data exceeds the size limit".to_owned())))
                }
                Ok(_) => Ok((Some(lua.create_string(&out)?), None::<String>)),
                Err(e) => Ok((None, Some(e.to_string()))),
            }
        })?;
        let deflate = lua.create_function(|lua, data: mlua::LuaString| {
            let bytes = data.as_bytes();
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            let result = enc
                .write_all(&bytes)
                .and_then(|()| enc.finish());
            match result {
                Ok(out) => Ok((Some(lua.create_string(&out)?), None::<String>)),
                Err(e) => Ok((None, Some(e.to_string()))),
            }
        })?;
        lua.globals().set("Inflate", inflate)?;
        lua.globals().set("Deflate", deflate)?;
        Ok(())
    }

    /// Load a build from PoB build XML and run calculation to completion.
    ///
    /// `loadBuildFromXML` runs a single frame, which is enough to parse the
    /// header but not to populate the tree spec or socket groups: PoB defers
    /// that work across frames (`buildFlag`/`modFlag` are cleared lazily in
    /// `OnFrame`). Stopping after one frame yields a bare character —
    /// level parsed, 1 allocated node, 0 socket groups, TotalDPS 0.06.
    ///
    /// So pump frames until the build stops changing, then one more to let
    /// the final calc pass land.
    /// A snippet compiled once per engine. Around a hundred of them run
    /// per refresh, some hundreds of lines long; parsing each on every call
    /// was a measurable slice of every edit's cost.
    pub(crate) fn chunk(&self, src: &'static str) -> Result<mlua::Function, Error> {
        let key = src.as_ptr() as usize;
        if let Some(f) = self.chunks.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self.lua.load(src).into_function()?;
        self.chunks.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// The Lua heap in megabytes, for the status bar and for tuning the
    /// load budget: the budget is only meaningful relative to this.
    pub fn used_memory_mb(&self) -> usize {
        self.lua.used_memory() / (1024 * 1024)
    }

    /// The highest heap the budget hook saw during the last load.
    pub fn peak_memory_mb(&self) -> usize {
        PEAK_MEMORY.load(std::sync::atomic::Ordering::Relaxed) / (1024 * 1024)
    }

    /// Run a full garbage collection.
    pub fn gc_collect(&self) {
        let _ = self.lua.gc_collect();
    }

    pub fn load_build_xml(&self, xml: &str) -> Result<(), Error> {
        // Every path that loads XML lands here: a paste, a library file, a
        // compare load. PoB's xml.lua is quadratic on an unterminated
        // comment or attribute quote and runs inside a single C call to
        // gsub, which no VM hook can interrupt, so it is refused up front.
        pob_import::check_xml(xml).map_err(Error::Rejected)?;
        let result = run_with_budget(&self.lua, "loading the build", LOAD_BUDGET, LOAD_MEMORY_CAP, || {
            self.load_build_xml_unbounded(xml)
        });
        // A load leaves two to three times its live data behind as garbage
        // (a large build: 530 MB allocated, 226 MB alive). The incremental collector
        // would get to it eventually; on a device where memory is the
        // thing that kills apps, eventually is now. A few hundred ms
        // against a load that took seconds.
        let _ = self.lua.gc_collect();
        result
    }

    fn load_build_xml_unbounded(&self, xml: &str) -> Result<(), Error> {
        // The build being replaced must not autosave on its way out: PoB's
        // Shutdown writes a dev-mode autosave unless abortSave is set, and
        // on a build whose load failed halfway (no savers) that write is
        // the crash that made every load after a bad one fail.
        self.chunk(
                r#"
                local m = __mainObject__.main
                local b = m.modes and m.modes["BUILD"]
                if b then b.abortSave = true end
                "#,
            )?.call::<()>(())?;
        let f: mlua::Function = self.lua.globals().get("loadBuildFromXML")?;
        f.call::<()>(xml)?;

        if let Some(msg) = Self::prompt_msg(&self.lua)? {
            // A failed LoadDB calls CloseBuild, which only queues the
            // switch back to the list; finish it so the half-built mode is
            // gone before the next load, and drop whatever it reports.
            let _ = self.chunk(
                    r#"
                    local m = __mainObject__.main
                    local b = m.modes and m.modes["BUILD"]
                    if b then b.abortSave = true end
                    runCallback("OnFrame")
                    "#,
                )?.call::<()>(());
            let _ = Self::prompt_msg(&self.lua);
            return Err(Error::PobStartup(msg));
        }

        let run_frame: mlua::Function = self.lua.globals().get("runCallback")?;

        // Re-baseline the items undo history now that the whole build is in.
        //
        // ItemsTab:Load ends with ResetUndo, but the jewel sockets are slots
        // whose selection comes from spec.jewels, and the tree loads after
        // the items. So PoB's first undo state records every jewel socket as
        // empty, and undoing any item change back to it writes those zeros
        // into spec.jewels: every socketed jewel gone, and saved that way.
        // Populate the slots from the spec, then reset with them filled.
        self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                if tab and tab.PopulateSlots and tab.ResetUndo then
                    tab:PopulateSlots()
                    tab:ResetUndo()
                end
                "#,
            )?.call::<()>(())?;

        // Bounded so a build that never settles fails loudly rather than
        // hanging. PoB settles in a handful of frames in practice.
        const MAX_FRAMES: usize = 32;
        let mut last = self.stat("TotalDPS").ok().flatten();
        for _ in 0..MAX_FRAMES {
            run_frame.call::<()>("OnFrame")?;
            let now = self.stat("TotalDPS").ok().flatten();
            if now.is_some() && now == last {
                return Ok(());
            }
            last = now;
        }
        Ok(())
    }

    /// PoB's swallowed-error channel. It writes failures here and carries on
    /// with a broken build rather than raising, so we check it explicitly
    /// after boot and after every load.
    /// Fail if PoB swallowed an error into its prompt channel.
    ///
    /// PoB catches its own errors and stashes them rather than propagating,
    /// so a write that "succeeded" can have done nothing at all. Every writer
    /// checks this; skipping it is how a silent no-op reaches the UI as a
    /// stale but plausible number.
    fn check_prompt(lua: &Lua) -> Result<(), Error> {
        match Self::prompt_msg(lua)? {
            Some(msg) => Err(Error::PobStartup(msg)),
            None => Ok(()),
        }
    }

    fn prompt_msg(lua: &Lua) -> Result<Option<String>, Error> {
        let main_object: Table = lua.globals().get("__mainObject__")?;
        let msg: Option<String> = main_object.get("promptMsg")?;
        // Take it, don't peek. PoB clears promptMsg only on a key press in
        // its own popup, which never comes headless, and ShowErrMsg sets it
        // only while it is nil, so an unread message would be reported by
        // every later call as its own failure and hide the next real one.
        if msg.is_some() {
            main_object.set("promptMsg", mlua::Nil)?;
        }
        Ok(msg.filter(|m| !m.is_empty()).map(|m| {
            // Strip PoB's ^N colour escapes and collapse to one line.
            let cleaned = m
                .split('\n')
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            cleaned
                .replace("^1", "")
                .replace("^0", "")
                .replace("^8", "")
        }))
    }

    /// Evaluate a Lua chunk against the live VM. Diagnostics only.
    pub fn eval<T: mlua::FromLuaMulti>(&self, chunk: &str) -> Result<T, Error> {
        Ok(self.lua.load(chunk).eval::<T>()?)
    }

    /// The build PoB currently has active.
    ///
    /// Not the `build` global: HeadlessWrapper sets that once, at load time,
    /// bound to the empty build created during `OnInit`. `loadBuildFromXML`
    /// goes through `SetMode`, which installs a *new* build object, so the
    /// global is stale from then on and reading it yields a bare character's
    /// numbers (TotalDPS 0.06) rather than the loaded build's. Resolve
    /// through `main.modes` on every read instead.
    fn build(&self) -> Result<Table, Error> {
        let main_object: Table = self.lua.globals().get("__mainObject__")?;
        let main: Table = main_object.get("main")?;
        let modes: Table = main.get("modes")?;
        Ok(modes.get::<Table>("BUILD")?)
    }

    /// Read a computed stat off the active build's output.
    ///
    /// PoB stores the current pass under `build.calcsTab.mainOutput`.
    pub fn stat(&self, name: &str) -> Result<Option<f64>, Error> {
        let calcs: Table = self.build()?.get("calcsTab")?;
        let output: Table = calcs.get("mainOutput")?;
        Ok(output.get::<Option<f64>>(name)?)
    }

    /// Read several stats in one pass over the output table.
    pub fn stats(&self, names: &[&str]) -> Result<Vec<(String, Option<f64>)>, Error> {
        let calcs: Table = self.build()?.get("calcsTab")?;
        let output: Table = calcs.get("mainOutput")?;
        names
            .iter()
            .map(|n| {
                // Some of PoB's display stats are tables or strings (lists
                // it formats itself). They are not numbers to compare, and
                // must not fail the whole read.
                let v = match output.get::<mlua::Value>(*n)? {
                    mlua::Value::Number(x) => Some(x),
                    mlua::Value::Integer(x) => Some(x as f64),
                    _ => None,
                };
                Ok(((*n).to_owned(), v))
            })
            .collect()
    }

    /// Socket groups with their gems, in PoB's own order.
    pub fn socket_groups(&self) -> Result<Vec<skills::SocketGroup>, Error> {
        let build = self.build()?;
        let tab: Table = build.get("skillsTab")?;
        let list: Table = tab.get("socketGroupList")?;

        let mut groups = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let g = entry?;

            let mut gems = Vec::new();
            if let Some(gem_list) = g.get::<Option<Table>>("gemList")? {
                for gem_entry in gem_list.sequence_values::<Table>() {
                    let gem = gem_entry?;
                    gems.push(skills::Gem {
                        name: gem.get::<Option<String>>("nameSpec")?.unwrap_or_default(),
                        skill_id: gem.get::<Option<String>>("skillId")?.unwrap_or_default(),
                        level: gem.get::<Option<i64>>("level")?.unwrap_or(1),
                        quality: gem.get::<Option<i64>>("quality")?.unwrap_or(0),
                        effective_quality: {
                            // displayEffect carries the socket-boosted value
                            // after ProcessSocketGroup; fall back to base.
                            let base = gem.get::<Option<i64>>("quality")?.unwrap_or(0);
                            gem.get::<Option<Table>>("displayEffect")?
                                .and_then(|d| d.get::<Option<i64>>("quality").ok().flatten())
                                .unwrap_or(base)
                        },
                        enabled: gem.get::<Option<bool>>("enabled")?.unwrap_or(true),
                        // Granted skills carry no count; treat them as one.
                        count: gem.get::<Option<i64>>("count")?.unwrap_or(1),
                        color: gem
                            .get::<Option<String>>("color")?
                            .and_then(|c| skills::SocketColor::parse(&c)),
                    });
                }
            }

            // The colours of the item's sockets this group sits in
            // (R/G/B/A/W-...), read off the equipped item, not the gems.
            let slot: Option<String> = g.get("slot")?;
            let item_sockets: String = if let Some(sn) = &slot {
                self.lua.globals().set("__pobios_slot", sn.clone())?;
                let out: String = self.chunk(
                        r#"
                        local b = __mainObject__.main.modes["BUILD"]
                        local slot = b.itemsTab and b.itemsTab.slots[__pobios_slot]
                        local item = slot and slot.selItemId and b.itemsTab.items[slot.selItemId]
                        if not item or not item.sockets then return "" end
                        local out = {}
                        for i, sock in ipairs(item.sockets) do
                            if i > 1 then out[#out + 1] = (item.sockets[i-1].group == sock.group) and "-" or " " end
                            out[#out + 1] = tostring(sock.color)
                        end
                        return table.concat(out)
                        "#,
                    )?.call(())?;
                out
            } else {
                String::new()
            };

            groups.push(skills::SocketGroup {
                label: g.get::<Option<String>>("label")?.unwrap_or_default(),
                slot,
                enabled: g.get::<Option<bool>>("enabled")?.unwrap_or(true),
                include_in_full_dps: g
                    .get::<Option<bool>>("includeInFullDPS")?
                    .unwrap_or(false),
                source: g.get::<Option<String>>("source")?,
                item_sockets,
                gems,
            });
        }
        Ok(groups)
    }

    /// Index of the group PoB treats as the main skill, zero-based.
    pub fn main_socket_group(&self) -> Result<usize, Error> {
        let build = self.build()?;
        // PoB's index is 1-based. Clamp to the groups that exist: a saved
        // build can point past the end, and an index that matches nothing
        // would then land on whichever group is added next.
        let idx: Option<i64> = build.get("mainSocketGroup")?;
        let count: i64 = self.chunk(r#"local b = __mainObject__.main.modes["BUILD"]; return b.skillsTab and #b.skillsTab.socketGroupList or 0"#)?.call(())?;
        let idx = idx.unwrap_or(1).min(count.max(1)).saturating_sub(1).max(0);
        Ok(idx as usize)
    }

    /// Equipped items, in PoB's own slot order. Slots with nothing equipped
    /// are included with `item: None` so the list stays stable.
    pub fn equipment(&self) -> Result<Vec<items::Slot>, Error> {
        let build = self.build()?;
        let tab: Table = build.get("itemsTab")?;
        let slots: Table = tab.get("slots")?;
        let all_items: Table = tab.get("items")?;

        let flavour = self.flavour_lookup()?;

        // PoB shows every slot in its order that is not inactive: the base
        // gear, then the abyssal sockets an item provides, the weapon swaps
        // when in use, and the tree's jewel sockets when allocated. The
        // base list comes first regardless, so the layout is stable.
        let extra: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                local out = {}
                -- PoB marks tree sockets inactive in UpdateSockets, which
                -- its draw loop runs; nothing draws here, so run it now.
                if tab.UpdateSockets then tab:UpdateSockets() end
                local swap = tab.activeItemSet and tab.activeItemSet.useSecondWeaponSet
                local base = {}
                for _, n in ipairs({ "Weapon 1", "Weapon 2", "Helmet", "Body Armour", "Gloves", "Boots", "Amulet", "Ring 1", "Ring 2", "Ring 3", "Belt", "Graft 1", "Graft 2", "Flask 1", "Flask 2", "Flask 3", "Flask 4", "Flask 5" }) do base[n] = true end
                for _, slot in ipairs(tab.orderedSlots or {}) do
                    local isSwap = slot.slotName == "Weapon 1 Swap" or slot.slotName == "Weapon 2 Swap"
                    if not base[slot.slotName] and not slot.inactive and (not isSwap or swap) then
                        local label = slot.label or slot.slotName
                        if slot.nodeId then
                            local node = b.spec and b.spec.nodes[slot.nodeId]
                            label = (node and node.dn or "Jewel Socket") .. " (" .. tostring(slot.nodeId) .. ")"
                        end
                        out[#out + 1] = { slot.slotName, label }
                    end
                end
                return out
                "#,
            )?.call(())?;
        let mut names: Vec<(String, String)> = items::EQUIPMENT_SLOTS.iter().map(|n| ((*n).to_owned(), (*n).to_owned())).collect();
        for pair in extra.sequence_values::<Table>() {
            let pair = pair?;
            names.push((pair.get(1)?, pair.get(2)?));
        }

        let mut out = Vec::with_capacity(names.len());
        for (name, label) in &names {
            let name = name.as_str();
            let slot: Option<Table> = slots.get(name)?;
            let item = match &slot {
                Some(s) => {
                    let id: Option<i64> = s.get("selItemId")?;
                    match id.filter(|i| *i != 0) {
                        Some(id) => all_items
                            .get::<Option<Table>>(id)?
                            .map(|t| self.read_item(&t, &flavour))
                            .transpose()?,
                        None => None,
                    }
                }
                None => None,
            };
            // PoB puts the activate checkbox on flask slots only, and reads
            // the state from the item set rather than from the slot control.
            let active = if name.starts_with("Flask") {
                Some(slot.as_ref().and_then(|s| s.get::<Option<bool>>("active").ok().flatten()).unwrap_or(false))
            } else {
                None
            };
            out.push(items::Slot {
                name: name.to_owned(),
                label: label.clone(),
                item,
                active,
            });
        }
        Ok(out)
    }

    /// PoB's flavour text, keyed by item title.
    ///
    /// Read in one pass and handed to `read_item`, rather than looked up per
    /// item: see the note there about what a nested Lua call inside a loop
    /// over Lua tables does to LuaJIT.
    fn flavour_lookup(&self) -> Result<std::collections::HashMap<String, String>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local data = b and b.data
                if not data or not data.flavourText then return {} end
                local out = {}
                for _, entry in pairs(data.flavourText) do
                    if entry.name and entry.text and not out[entry.name] then
                        -- `text` is a table of lines, one per rendered row.
                        if type(entry.text) == "table" then
                            out[entry.name] = table.concat(entry.text, "\n")
                        else
                            out[entry.name] = tostring(entry.text)
                        end
                    end
                end
                return out
                "#,
            )?;

        let table: Table = f.call(())?;
        let mut out = std::collections::HashMap::new();
        for pair in table.pairs::<String, String>() {
            let (k, v) = pair?;
            out.insert(k, v);
        }
        Ok(out)
    }

    /// The headline numbers on PoB's item tooltip, from the weapon or
    /// armour data PoB computed for the item.
    fn item_stats(t: &Table) -> Result<Vec<(String, String)>, Error> {
        let mut out = Vec::new();
        let num = |tbl: &Table, k: &str| -> Result<Option<f64>, Error> {
            Ok(match tbl.get::<mlua::Value>(k)? {
                mlua::Value::Number(x) => Some(x),
                mlua::Value::Integer(x) => Some(x as f64),
                _ => None,
            })
        };
        let fmt = |x: f64| -> String {
            if x == x.round() { format!("{}", x as i64) } else { format!("{x:.1}") }
        };
        if let Some(wd) = t.get::<Option<Table>>("weaponData")?
            && let Some(w) = wd.get::<Option<Table>>(1)? {
                // PoB derives physical DPS from the roll and the rate
                // rather than storing it.
                if let (Some(lo), Some(hi), Some(rate)) = (num(&w, "PhysicalMin")?, num(&w, "PhysicalMax")?, num(&w, "AttackRate")?) {
                    let dps = (lo + hi) / 2.0 * rate;
                    if dps > 0.0 { out.push(("Phys DPS".to_owned(), fmt(dps))); }
                }
                for (k, label) in [("ElementalDPS", "Elemental DPS"), ("ChaosDPS", "Chaos DPS"), ("TotalDPS", "Total DPS")] {
                    if let Some(v) = num(&w, k)?
                        && v > 0.0 { out.push((label.to_owned(), fmt(v))); }
                }
                if let Some(v) = num(&w, "CritChance")? { out.push(("Crit".to_owned(), format!("{v:.2}%"))); }
                if let Some(v) = num(&w, "AttackRate")? { out.push(("APS".to_owned(), format!("{v:.2}"))); }
            }
        if let Some(a) = t.get::<Option<Table>>("armourData")? {
            for (k, label) in [("Armour", "Armour"), ("Evasion", "Evasion"), ("EnergyShield", "ES"), ("Ward", "Ward")] {
                if let Some(v) = num(&a, k)?
                    && v > 0.0 { out.push((label.to_owned(), fmt(v))); }
            }
            if let Some(v) = num(&a, "BlockChance")?
                && v > 0.0 { out.push(("Block".to_owned(), format!("{}%", fmt(v)))); }
        }
        Ok(out)
    }

    /// Read one item, flattening PoB's per-kind mod tables into one list.
    fn read_item(
        &self,
        t: &Table,
        flavour: &std::collections::HashMap<String, String>,
    ) -> Result<items::Item, Error> {
        let mut mods = Vec::new();

        // PoB keeps each kind in its own table rather than tagging one list,
        // so the editor's grouping comes straight from which table a line
        // lives in. Order here is the order the editor shows them.
        for (field, kind) in [
            ("enchantModLines", items::ModKind::Enchant),
            ("implicitModLines", items::ModKind::Implicit),
            ("explicitModLines", items::ModKind::Explicit),
            ("scourgeModLines", items::ModKind::Scourge),
            ("crucibleModLines", items::ModKind::Crucible),
        ] {
            let Some(lines) = t.get::<Option<Table>>(field)? else {
                continue;
            };
            for entry in lines.sequence_values::<Table>() {
                let line = entry?;
                let text: Option<String> = line.get("line")?;
                let Some(text) = text.filter(|s| !s.trim().is_empty()) else {
                    continue;
                };
                // A crafted explicit is tagged on the line itself.
                let crafted: Option<bool> = line.get("crafted")?;
                let kind = if crafted.unwrap_or(false) && kind == items::ModKind::Explicit {
                    items::ModKind::Crafted
                } else {
                    kind
                };
                // `extra` is PoB's flag for a line it parsed but could not
                // fully interpret. Those do not count towards the build, and
                // PoB says so rather than letting a number quietly be wrong.
                let unsupported = line.get::<Option<bool>>("extra")?.unwrap_or(false);
                mods.push(items::ModLine {
                    text,
                    kind,
                    unsupported,
                });
            }
        }

        let reqs: Option<Table> = t.get("requirements")?;

        // Flavour comes from the base type where a base carries one, and
        // otherwise from PoB's `data.flavourText`, keyed by item title.
        //
        // Resolved against a map built once per call rather than by running a
        // Lua lookup per item. The first version did the latter, and it
        // reintroduced the `LuaJIT ASSERT lj_opt_mem.c:61` abort that the
        // engine's boot lock had fixed: a nested `lua.load` inside a loop
        // already iterating Lua tables is exactly the interleaving that
        // assertion catches. Reading the table once is also simply less work.
        //
        // The "Foulborn " prefix is stripped before lookup, as PoB does — the
        // corrupted variant of a unique keeps the original's flavour. Items
        // with several flavour variants (Grand Spectrum picks its line from
        // what is already in the tooltip) take the first, which is a real
        // divergence and cosmetic.
        let flavour = {
            let from_base = t
                .get::<Option<Table>>("base")?
                .and_then(|b| b.get::<Option<String>>("flavourText").ok().flatten());
            match from_base.filter(|s| !s.trim().is_empty()) {
                Some(text) => Some(text),
                None => {
                    // `title` is not always a string: PoB stores a table for
                    // some multi-variant uniques, and asking for a String
                    // there is an error rather than a None.
                    let title = match t.get::<mlua::Value>("title")? {
                        mlua::Value::String(s) => Some(s.to_str()?.to_owned()),
                        _ => None,
                    };
                    title
                        .map(|t| t.trim_start_matches("Foulborn ").to_owned())
                        .and_then(|t| flavour.get(&t).cloned())
                }
            }
        };

        Ok(items::Item {
            name: t.get::<Option<String>>("name")?.unwrap_or_default(),
            base_name: t.get::<Option<String>>("baseName")?.unwrap_or_default(),
            rarity: t
                .get::<Option<String>>("rarity")?
                .map(|r| items::Rarity::parse(&r))
                .unwrap_or_default(),
            item_level: t.get::<Option<i64>>("itemLevel")?.unwrap_or(0),
            quality: t.get::<Option<i64>>("quality")?.unwrap_or(0),
            mods,
            req_level: reqs.as_ref().and_then(|r| r.get("level").ok().flatten()),
            req_str: reqs.as_ref().and_then(|r| r.get("str").ok().flatten()),
            req_dex: reqs.as_ref().and_then(|r| r.get("dex").ok().flatten()),
            req_int: reqs.as_ref().and_then(|r| r.get("int").ok().flatten()),
            flavour,
            stats: Self::item_stats(t)?,
        })
    }

    /// The Calcs tab's sections, with every cell already formatted by PoB.
    ///
    /// Values come from `formatCalcStr`, PoB's own formatter, rather than from
    /// a reimplementation of its `{0:mod:1,2}` format language. That keeps the
    /// numbers identical to PoB's and survives upstream changing the syntax.
    /// The Calcs tab's sections as PoB shows them: only what `CheckFlag`
    /// enables for the selected skill, with PoB's column, colour and the
    /// header value, formatted against the tab's own environment
    /// (calcsEnv, which honours the Calcs inputs), the minion's when
    /// "Show Minion Stats" is on.
    pub fn calc_sections(&self) -> Result<Vec<calcs::Section>, Error> {
        let raw: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.calcsTab
                local env = tab.calcsEnv or tab.mainEnv
                local actor = env and (tab.input.showMinion and env.minion or env.player)
                local out = {}
                if not actor or not formatCalcStr then return out end
                local function hexOf(code)
                    if type(code) ~= "string" then return "" end
                    local h = code:match("^%^x(%x%x%x%x%x%x)")
                    return h and ('#' .. h) or ""
                end
                local function enabled(obj) return tab:CheckFlag(obj, actor) and true or false end
                for si, sec in ipairs(tab.sectionList or {}) do
                    if sec.id ~= "SkillSelect" and sec.subSection then
                        for subi, sub in ipairs(sec.subSection) do
                            local data = sub.data or {}
                            if enabled(data) then
                                local rows = {}
                                for _, row in ipairs(data) do
                                    if type(row) == "table" and enabled(row) then
                                        local cells = {}
                                        local isHeader = (row.label == nil)
                                        for _, cell in ipairs(row) do
                                            local text = ""
                                            local hasBd = false
                                            if type(cell) == "table" then
                                                if cell.format and enabled(cell) then
                                                    if isHeader then
                                                        text = cell.format
                                                    else
                                                        local ok, v = pcall(formatCalcStr, cell.format, actor, cell)
                                                        if ok and v then text = tostring(v) end
                                                    end
                                                end
                                                for _, entry in ipairs(cell) do
                                                    if type(entry) == "table" and (entry.breakdown or entry.modName) then hasBd = true end
                                                end
                                            end
                                            cells[#cells + 1] = { text, hasBd }
                                        end
                                        rows[#rows + 1] = { row.label or "", cells, isHeader }
                                    end
                                end
                                if #rows > 0 then
                                    local extra = ""
                                    if data.extra then
                                        local ok, v = pcall(formatCalcStr, data.extra, actor)
                                        if ok and v then extra = tostring(v) end
                                    end
                                    out[#out + 1] = {
                                        sec.id or "", sub.label or (sec.id or ""), rows,
                                        sec.group or 1, hexOf(sec.colour), extra,
                                        sub.defaultCollapsed and true or false, si, subi
                                    }
                                end
                            end
                        end
                    end
                end
                return out
                "#,
            )?.call(())?;

        let mut sections = Vec::new();
        for section in raw.sequence_values::<Table>() {
            let section = section?;
            let id: String = section.get(1)?;
            let label: String = section.get(2)?;
            let rows_table: Table = section.get(3)?;
            let mut rows = Vec::new();
            for row in rows_table.sequence_values::<Table>() {
                let row = row?;
                let row_label: String = row.get(1)?;
                let cells_table: Table = row.get(2)?;
                let is_header: bool = row.get(3)?;
                let mut cells = Vec::new();
                for cell in cells_table.sequence_values::<Table>() {
                    let cell = cell?;
                    let value: String = cell.get(1)?;
                    let has_breakdown: bool = cell.get(2)?;
                    cells.push(calcs::Cell { value: calcs::strip_color_escapes(&value), has_breakdown });
                }
                rows.push(calcs::Row { label: calcs::strip_color_escapes(&row_label), cells, is_header });
            }
            sections.push(calcs::Section {
                id,
                label: calcs::strip_color_escapes(&label),
                rows,
                column: section.get::<i64>(4)? as u8,
                colour: section.get(5)?,
                extra: calcs::strip_color_escapes(&section.get::<String>(6)?),
                collapsed_default: section.get(7)?,
                section_index: section.get::<i64>(8)? as usize,
                sub_index: section.get::<i64>(9)? as usize,
            });
        }
        Ok(sections)
    }

    /// Another build's Calcs sections, read by loading it in place of the
    /// current one and restoring afterwards, as `stats_for` does.
    pub fn calc_sections_for(&self, other_xml: &str) -> Result<Vec<calcs::Section>, Error> {
        let saved = self.build_xml()?;
        let result = (|| {
            self.load_build_xml(other_xml)?;
            self.calc_sections()
        })();
        self.load_build_xml(&saved)?;
        result
    }

    /// PoB's breakdown for one cell, built by PoB's own breakdown control
    /// so the formatting is PoB's. Indices are 1-based, as PoB's are.
    pub fn calc_breakdown(
        &self,
        section_index: usize,
        sub_index: usize,
        row_index: usize,
        cell_index: usize,
    ) -> Result<Vec<calcs::BreakdownPart>, Error> {
        let raw: Table = self.chunk(
                r#"
                local si, subi, ri, ci = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.calcsTab
                local out = {}
                local sec = tab.sectionList[si]
                local sub = sec and sec.subSection[subi]
                local row = sub and sub.data[ri]
                local cell = row and row[ci]
                if not cell then return out end
                local bd = tab.controls.breakdown
                bd:SetBreakdownData(cell, true)
                for _, part in ipairs(bd.sectionList or {}) do
                    if part.type == "TEXT" then
                        local lines = {}
                        for i, l in ipairs(part.lines) do lines[i] = tostring(l) end
                        out[#out + 1] = { "", "", lines, {}, {} }
                    elseif part.type == "TABLE" then
                        local cols, keys = {}, {}
                        for i, col in ipairs(part.colList) do cols[i] = col.label or ""; keys[i] = col.key end
                        local rows = {}
                        for _, r in ipairs(part.rowList) do
                            local cellsOut = {}
                            for i, k in ipairs(keys) do
                                local v = r[k]
                                cellsOut[i] = v == nil and "" or tostring(v)
                            end
                            rows[#rows + 1] = cellsOut
                        end
                        out[#out + 1] = { part.label or "", part.footer or "", {}, cols, rows }
                    end
                end
                bd:SetBreakdownData(nil)
                return out
                "#,
            )?.call((section_index, sub_index, row_index, cell_index))?;
        let mut parts = Vec::new();
        for part in raw.sequence_values::<Table>() {
            let part = part?;
            let label: String = part.get(1)?;
            let footer: String = part.get(2)?;
            let lines: Vec<String> = part.get::<Table>(3)?.sequence_values::<String>().collect::<Result<_, _>>()?;
            let columns: Vec<String> = part.get::<Table>(4)?.sequence_values::<String>().collect::<Result<_, _>>()?;
            let mut rows = Vec::new();
            for r in part.get::<Table>(5)?.sequence_values::<Table>() {
                let r = r?;
                rows.push(r.sequence_values::<String>().map(|v| v.map(|x| calcs::strip_color_escapes(&x))).collect::<Result<Vec<_>, _>>()?);
            }
            parts.push(calcs::BreakdownPart {
                label: if label.is_empty() { None } else { Some(calcs::strip_color_escapes(&label)) },
                footer: if footer.is_empty() { None } else { Some(calcs::strip_color_escapes(&footer)) },
                lines: lines.iter().map(|l| calcs::strip_color_escapes(l)).collect(),
                columns: columns.iter().map(|c| calcs::strip_color_escapes(c)).collect(),
                rows,
            });
        }
        Ok(parts)
    }

    /// The Calcs tab's header inputs, which select a skill independently
    /// of the sidebar (PoB's `...Calcs` variants).
    pub fn calcs_inputs(&self) -> Result<calcs::CalcsInputs, Error> {
        let t: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.calcsTab
                local out = { groups = {}, groupIndex = 1, buffMode = "EFFECTIVE", hasMinion = false, showMinion = false,
                              active = {}, activeIndex = 1, parts = {}, partIndex = 1,
                              minions = {}, minionValues = {}, minionIndex = 1, byItemSet = false,
                              minionSkills = {}, minionSkillIndex = 1 }
                if not tab then return out end
                for i, g in ipairs(b.skillsTab.socketGroupList) do out.groups[i] = g.displayLabel or "" end
                out.groupIndex = tab.input.skill_number or 1
                out.buffMode = tab.input.misc_buffMode or "EFFECTIVE"
                out.showMinion = tab.input.showMinion and true or false
                local env = tab.calcsEnv
                local player = env and env.player
                out.hasMinion = (player and player.mainSkill and player.mainSkill.skillFlags.haveMinion) and true or false
                local group = b.skillsTab.socketGroupList[out.groupIndex]
                if not group then return out end
                local list = group.displaySkillListCalcs or {}
                local sel = group.mainActiveSkillCalcs or 1
                for i, activeSkill in ipairs(list) do
                    out.active[i] = activeSkill.activeEffect.grantedEffect.name
                end
                out.activeIndex = sel
                local activeSkill = list[sel]
                local effect = activeSkill and activeSkill.activeEffect
                if not effect then return out end
                local ge = effect.grantedEffect
                local src = effect.srcInstance
                if ge.parts and #ge.parts > 1 then
                    for i, part in ipairs(ge.parts) do out.parts[i] = part.name end
                    out.partIndex = src.skillPartCalcs or 1
                    if ge.parts[out.partIndex] and ge.parts[out.partIndex].stages then
                        out.stages = src.skillStageCountCalcs or activeSkill.skillData.stagesMax or ge.parts[out.partIndex].stagesMin or 1
                    end
                end
                if activeSkill.skillFlags.mine then out.mines = src.skillMineCountCalcs or 0 end
                if activeSkill.skillFlags.multiStage and not (ge.parts and #ge.parts > 1) then
                    out.stages = src.skillStageCountCalcs or activeSkill.skillData.stagesMax or activeSkill.skillData.stagesMin or 1
                end
                if not activeSkill.skillFlags.disable and (ge.minionList or activeSkill.minionList[1]) then
                    if ge.minionHasItemSet then
                        out.byItemSet = true
                        local want = src.skillMinionItemSetCalcs or 1
                        for i, itemSetId in ipairs(b.itemsTab.itemSetOrderList) do
                            local itemSet = b.itemsTab.itemSets[itemSetId]
                            out.minions[i] = itemSet.title or "Default Item Set"
                            out.minionValues[i] = tostring(itemSetId)
                            if itemSetId == want then out.minionIndex = i end
                        end
                    else
                        local want = src.skillMinionCalcs
                        for i, minionId in ipairs(activeSkill.minionList) do
                            out.minions[i] = b.data.minions[minionId].name
                            out.minionValues[i] = minionId
                            if minionId == want then out.minionIndex = i end
                        end
                    end
                    if activeSkill.minion then
                        for i, ms in ipairs(activeSkill.minion.activeSkillList) do
                            out.minionSkills[i] = ms.activeEffect.grantedEffect.name
                        end
                        out.minionSkillIndex = src.skillMinionSkillCalcs or 1
                    end
                end
                return out
                "#,
            )?.call(())?;
        let strings = |key: &str| -> Result<Vec<String>, Error> {
            let inner: Table = t.get(key)?;
            Ok(inner.sequence_values::<String>().collect::<Result<_, _>>()?)
        };
        let idx = |key: &str| -> Result<usize, Error> { Ok(t.get::<Option<i64>>(key)?.unwrap_or(1).max(1) as usize) };
        Ok(calcs::CalcsInputs {
            groups: strings("groups")?,
            group_index: idx("groupIndex")?,
            controls: skills::MainSkillControls {
                active_skills: strings("active")?,
                active_index: idx("activeIndex")?,
                parts: strings("parts")?,
                part_index: idx("partIndex")?,
                stages: t.get("stages")?,
                mines: t.get("mines")?,
                minions: strings("minions")?,
                minion_values: strings("minionValues")?,
                minion_index: idx("minionIndex")?,
                minion_by_item_set: t.get::<Option<bool>>("byItemSet")?.unwrap_or(false),
                minion_skills: strings("minionSkills")?,
                minion_skill_index: idx("minionSkillIndex")?,
            },
            buff_mode: t.get::<Option<String>>("buffMode")?.unwrap_or_else(|| "EFFECTIVE".into()),
            has_minion: t.get::<Option<bool>>("hasMinion")?.unwrap_or(false),
            show_minion: t.get::<Option<bool>>("showMinion")?.unwrap_or(false),
        })
    }

    /// Write one Calcs-tab input. `code` runs with `tab`, `group`, `src`
    /// (the selected skill's instance) and `v` in scope, as the tab's own
    /// controls do; the build is then rebuilt.
    fn set_calcs_field(&self, code: &str, value: mlua::Value) -> Result<(), Error> {
        let src = format!(
            r#"
            local v = ...
            local b = __mainObject__.main.modes["BUILD"]
            local tab = b.calcsTab
            if not tab then return end
            local group = b.skillsTab.socketGroupList[tab.input.skill_number or 1]
            local skill = group and group.displaySkillListCalcs and group.displaySkillListCalcs[group.mainActiveSkillCalcs or 1]
            local src = skill and skill.activeEffect and skill.activeEffect.srcInstance
            {code}
            tab.modFlag = true
            b.buildFlag = true
            runCallback("OnFrame")
            "#
        );
        let f: mlua::Function = self.lua.load(&src).into_function()?;
        f.call::<()>(value)?;
        Self::check_prompt(&self.lua)
    }

    pub fn set_calcs_group(&self, index: usize) -> Result<(), Error> {
        self.set_calcs_field("tab.input.skill_number = v", mlua::Value::Integer(index as i64))
    }
    pub fn set_calcs_active_skill(&self, index: usize) -> Result<(), Error> {
        self.set_calcs_field("if group then group.mainActiveSkillCalcs = v end", mlua::Value::Integer(index as i64))
    }
    pub fn set_calcs_part(&self, index: usize) -> Result<(), Error> {
        self.set_calcs_field("if src then src.skillPartCalcs = v end", mlua::Value::Integer(index as i64))
    }
    pub fn set_calcs_stages(&self, count: i64) -> Result<(), Error> {
        self.set_calcs_field("if src then src.skillStageCountCalcs = v end", mlua::Value::Integer(count))
    }
    pub fn set_calcs_mines(&self, count: i64) -> Result<(), Error> {
        self.set_calcs_field("if src then src.skillMineCountCalcs = v end", mlua::Value::Integer(count))
    }
    pub fn set_calcs_minion(&self, value: &str, by_item_set: bool) -> Result<(), Error> {
        let code = if by_item_set { "if src then src.skillMinionItemSetCalcs = tonumber(v) end" } else { "if src then src.skillMinionCalcs = v end" };
        let v = self.lua.create_string(value)?;
        self.set_calcs_field(code, mlua::Value::String(v))
    }
    pub fn set_calcs_minion_skill(&self, index: usize) -> Result<(), Error> {
        self.set_calcs_field("if src then src.skillMinionSkillCalcs = v end", mlua::Value::Integer(index as i64))
    }
    /// UNBUFFED, BUFFED, COMBAT or EFFECTIVE.
    pub fn set_calcs_buff_mode(&self, mode: &str) -> Result<(), Error> {
        let v = self.lua.create_string(mode)?;
        self.set_calcs_field("tab.input.misc_buffMode = v", mlua::Value::String(v))
    }
    pub fn set_calcs_show_minion(&self, show: bool) -> Result<(), Error> {
        self.set_calcs_field("tab.input.showMinion = v", mlua::Value::Boolean(show))
    }

    /// Configuration options with their current values.
    ///
    /// Definitions come from `Modules/ConfigOptions.lua` rather than from the
    /// live `varControls`: those are UI objects whose labels are functions,
    /// while the module returns plain readable data.
    pub fn config_options(&self) -> Result<Vec<config::ConfigOption>, Error> {
        let raw: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local input = b.configTab.input or {}
                local defs = LoadModule("Modules/ConfigOptions")
                local out = {}
                local section = ""

                for _, entry in ipairs(defs or {}) do
                    if entry.section then
                        section = entry.section
                    elseif entry.var and entry.label then
                        -- Only the types that need a control; the rest are
                        -- internal mod-application kinds.
                        local kind = nil
                        if entry.type == "check" then
                            kind = "check"
                        elseif entry.type == "count" or entry.type == "integer" then
                            kind = "count"
                        elseif entry.type == "countAllowZero" then
                            kind = "countZero"
                        elseif entry.type == "float" then
                            kind = "float"
                        elseif entry.type == "list" then
                            kind = "list"
                        end

                        if kind then
                            local listVals = {}
                            local default = ""
                            if kind == "list" then
                                for _, item in ipairs(entry.list or {}) do
                                    listVals[#listVals + 1] = {
                                        tostring(item.val),
                                        tostring(item.label or item.val),
                                    }
                                end
                                -- A list option can default to something other
                                -- than its first entry, and PoB writes that
                                -- default into `input` at load. Clearing the
                                -- key is therefore NOT the inverse of setting
                                -- it; restoring means writing the default back.
                                local di = entry.defaultIndex
                                if di and entry.list and entry.list[di] then
                                    default = tostring(entry.list[di].val)
                                end
                            end

                            local v = input[entry.var]
                            local vType = type(v)
                            local vStr = ""
                            if vType == "boolean" then
                                vStr = v and "true" or "false"
                            elseif v ~= nil then
                                vStr = tostring(v)
                            end

                            -- PoB folds every if-condition into the
                            -- control's shown(); one call answers "would
                            -- PoB show this for the build".
                            local rel = true
                            local ctrl = b.configTab.varControls and b.configTab.varControls[entry.var]
                            if ctrl and ctrl.shown then
                                local ok, v = pcall(ctrl.shown)
                                if ok then rel = v and true or false end
                            end
                            out[#out + 1] = {
                                entry.var,
                                entry.label,
                                section,
                                kind,
                                vType,
                                vStr,
                                listVals,
                                default,
                                rel,
                                type(entry.tooltip) == "string" and entry.tooltip or "",
                            }
                        end
                    end
                end
                return out
                "#,
            )?.call(())?;

        let mut options = Vec::new();
        for entry in raw.sequence_values::<Table>() {
            let entry = entry?;
            let var: String = entry.get(1)?;
            let label: String = entry.get(2)?;
            let section: String = entry.get(3)?;
            let kind: String = entry.get(4)?;
            let value_type: String = entry.get(5)?;
            let value_str: String = entry.get(6)?;
            let list_table: Table = entry.get(7)?;
            let default: String = entry.get(8)?;
            let relevant: bool = entry.get(9)?;
            let tooltip: String = entry.get(10)?;

            let kind = match kind.as_str() {
                "check" => config::OptionKind::Check,
                "count" => config::OptionKind::Number {
                    allow_zero: false,
                    float: false,
                },
                "countZero" => config::OptionKind::Number {
                    allow_zero: true,
                    float: false,
                },
                "float" => config::OptionKind::Number {
                    allow_zero: true,
                    float: true,
                },
                _ => {
                    let mut options = Vec::new();
                    for pair in list_table.sequence_values::<Table>() {
                        let pair = pair?;
                        // The display label carries PoB's ^xRRGGBB colour
                        // escapes (the exposure/element choices); strip them
                        // as the option label already is. The key (index 1)
                        // is kept verbatim: it is what set_config matches on.
                        options.push((pair.get::<String>(1)?, config::clean_label(&pair.get::<String>(2)?)));
                    }
                    config::OptionKind::List { options }
                }
            };

            let value = match value_type.as_str() {
                "boolean" => config::Value::Bool(value_str == "true"),
                "number" => value_str
                    .parse()
                    .map(config::Value::Number)
                    .unwrap_or(config::Value::Unset),
                "string" => config::Value::Text(value_str),
                _ => config::Value::Unset,
            };

            options.push(config::ConfigOption {
                var,
                label: config::clean_label(&label),
                section,
                kind,
                value,
                default: (!default.is_empty()).then_some(default),
                relevant,
                tooltip: (!tooltip.is_empty()).then_some(config::clean_tooltip(&tooltip)),
            });
        }
        Ok(options)
    }

    /// The active config set's custom modifier blocks.
    pub fn custom_mods(&self) -> Result<Vec<config::CustomModBlock>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.configTab
                local set = tab and tab.configSets[tab.activeConfigSetId]
                local out = {}
                for _, blk in ipairs(set and set.customModsList or {}) do
                    out[#out + 1] = { blk.title or "", blk.enabled and true or false, blk.text or "" }
                end
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            out.push(config::CustomModBlock { title: row.get(1)?, enabled: row.get(2)?, text: row.get(3)? });
        }
        Ok(out)
    }

    /// Change the custom modifier blocks, as the Config tab's controls do:
    /// `code` runs with `list` (the active set's blocks) and `v` in scope,
    /// then PoB rebuilds its mod list and the build.
    fn custom_mods_write(&self, code: &str, value: mlua::Value) -> Result<(), Error> {
        let src = format!(
            r#"
            local v = ...
            local b = __mainObject__.main.modes["BUILD"]
            local tab = b.configTab
            local set = tab and tab.configSets[tab.activeConfigSetId]
            if not set then return end
            set.customModsList = set.customModsList or {{}}
            local list = set.customModsList
            {code}
            if #list == 0 then table.insert(list, {{ title = "Default", enabled = true, text = "" }}) end
            tab:AddUndoState()
            tab:BuildModList()
            b.buildFlag = true
            runCallback("OnFrame")
            "#
        );
        let f: mlua::Function = self.lua.load(&src).into_function()?;
        f.call::<()>(value)?;
        Self::check_prompt(&self.lua)
    }

    pub fn add_custom_mod_block(&self) -> Result<(), Error> {
        self.custom_mods_write(r#"table.insert(list, { title = "Group " .. (#list + 1), enabled = true, text = "" })"#, mlua::Value::Nil)
    }

    pub fn remove_custom_mod_block(&self, index: usize) -> Result<(), Error> {
        self.custom_mods_write("if list[v] then table.remove(list, v) end", mlua::Value::Integer(index as i64))
    }

    /// Set a block's title, enabled flag and text. 1-based.
    pub fn set_custom_mod_block(&self, index: usize, title: &str, enabled: bool, text: &str) -> Result<(), Error> {
        let t = self.lua.create_table()?;
        t.set("i", index)?;
        t.set("title", title)?;
        t.set("enabled", enabled)?;
        t.set("text", text)?;
        self.custom_mods_write(
            "local blk = list[v.i]; if blk then blk.title = v.title; blk.enabled = v.enabled; blk.text = v.text end",
            mlua::Value::Table(t),
        )
    }

    /// Set a config input and recalculate.
    ///
    /// This is the write path: PoB needs its mod list rebuilt and a frame run
    /// before the outputs reflect the change, so a caller cannot just poke
    /// `input` and read a stat.
    pub fn set_config(&self, var: &str, value: config::Value) -> Result<Option<String>, Error> {
        let setter: mlua::Function = self.chunk(
                r#"
                local var, kind, text = ...
                local b = __mainObject__.main.modes["BUILD"]
                local cfg = b.configTab

                if kind == "unset" then
                    cfg.input[var] = nil
                elseif kind == "bool" then
                    -- PoB treats false and nil alike here, and storing false
                    -- leaves stale entries in the saved build.
                    if text == "true" then
                        cfg.input[var] = true
                    else
                        cfg.input[var] = nil
                    end
                elseif kind == "number" then
                    cfg.input[var] = tonumber(text)
                else
                    -- A list option's stored value must keep PoB's own type for
                    -- that entry: some lists use numbers (EHPUnluckyWorstOf's
                    -- 1/2/4), and PoB's calcs compare them numerically, so
                    -- storing the string "2" makes CalcDefence do "2" > 1 and
                    -- throw. Look the choice up in the option definition and
                    -- store its native `val`; fall back to the string.
                    local stored = text
                    local varList = LoadModule("Modules/ConfigOptions")
                    for _, varData in ipairs(varList) do
                        if varData.var == var and varData.list then
                            for _, item in ipairs(varData.list) do
                                if tostring(item.val) == text then
                                    stored = item.val
                                    break
                                end
                            end
                            break
                        end
                    end
                    cfg.input[var] = stored
                end

                cfg:BuildModList()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;

        let (kind, text) = match &value {
            config::Value::Bool(b) => ("bool", b.to_string()),
            config::Value::Number(n) => ("number", n.to_string()),
            config::Value::Text(t) => ("text", t.clone()),
            config::Value::Unset => ("unset", String::new()),
        };

        setter.call::<()>((var, kind, text))?;

        // A config toggle can make PoB post an informational promptMsg (for
        // example a note that a combination is unusual). That is not a failure:
        // the write has already been applied to `cfg.input`. Clear the message
        // so it cannot poison the next call, but do not turn it into an error,
        // which would make the caller skip re-reading and leave the control
        // looking stuck. Return it so the caller can surface it if it wants.
        Self::prompt_msg(&self.lua)
    }

    /// The passive tree as PoB has computed it for the current build.
    ///
    /// Node positions, connector paths and group placements all come from
    /// PoB's own processing rather than from a reimplementation of its
    /// geometry — see the module docs on `tree` for why that matters.
    pub fn tree(&self) -> Result<tree::Tree, Error> {
        let raw: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.spec
                local t = spec and spec.tree
                if not t then return { nodes = {}, connectors = {}, groups = {} } end

                local alloc = spec.allocNodes or {}
                local mastery = spec.masterySelections or {}
                -- The jewel in an allocated socket, as GetSocketAndJewelForNodeID
                -- finds it. Nil for any other node, and for empty sockets.
                local function socketJewelAny(node, id)
                    if node.type ~= "Socket" and not node.containJewelSocket then return nil end
                    local tab = b.itemsTab
                    local sock = tab and tab.sockets and tab.sockets[tonumber(id)]
                    return sock and sock.selItemId and tab.items[sock.selItemId] or nil
                end
                local function socketJewel(node, id)
                    if not alloc[id] then return nil end
                    return socketJewelAny(node, id)
                end
                -- The compared spec's jewel in a socket: specs keep their
                -- own socket assignments in spec.jewels.
                local function compareJewel(id)
                    local itemId = cmp and cmp.jewels and cmp.jewels[tonumber(id)]
                    return itemId and itemId > 0 and b.itemsTab and b.itemsTab.items[itemId] or nil
                end
                -- PoB's compareJewelsEqual: the same item text.
                local function sameJewel(a, c)
                    if not a or not c then return a == c end
                    return a:BuildRaw() == c:BuildRaw()
                end
                -- drawJewelRadius, as a list of sprites: art, radius, speed.
                -- Nil when PoB would draw nothing for this jewel.
                -- PoE2 measures every jewel radius in tree units times this.
                local radii = (b.data and b.data.jewelRadius) or data.jewelRadius or {}
                local reach = (data.gameConstants
                    and data.gameConstants["PassiveTreeJewelDistanceMultiplier"]) or 1
                local function shadedRings(outer, inner)
                    local sprites = {
                        { "ShadedOuterRing", outer, -0.7 }, { "ShadedOuterRingFlipped", outer, 0.7 },
                    }
                    -- PoB2 draws the inner pair always, at the inner radius,
                    -- which is 0 for every jewel but the ring-shaped kind.
                    if inner > 0 then
                        sprites[#sprites + 1] = { "ShadedInnerRing", inner, -0.7 }
                        sprites[#sprites + 1] = { "ShadedInnerRingFlipped", inner, 0.7 }
                    end
                    return sprites
                end
                local function ringsFor(jewel, tint)
                    if not jewel or not jewel.jewelRadiusIndex then return nil end
                    local rad = radii[jewel.jewelRadiusIndex]
                    if not rad then return nil end
                    local outer = rad.outer * reach
                    -- From Nothing's rings go on the keystone it names.
                    if jewel.title == "From Nothing" then return nil end
                    local jd = jewel.jewelData
                    local c = jd and jd.conqueredBy and jd.conqueredBy.conqueror
                    if c and c.type then
                        local who = tostring(c.type)
                        if who == "kalguur" then who = "kalguuran" end
                        local ingame = "art/textures/interface/2d/2dart/uiimages/ingame/"
                        local one = ingame .. "passiveskillscreen" .. who .. "jewelcircle1.dds"
                        local two = ingame .. "passiveskillscreen" .. who .. "jewelcircle2.dds"
                        if who == "abyss" then
                            one = ingame .. who .. "/" .. who .. "passiveskillscreenjewelcircle1.dds"
                            two = one
                        end
                        return { { { one, outer, -0.7 }, { two, outer, 0.7 } }, outer, tint }
                    end
                    return { shadedRings(outer, rad.inner * reach * 1.06), outer, tint }
                end
                local NEUTRAL, PRIMARY_ONLY, COMPARE_ONLY = { 1, 1, 1, 0.7 }, { 1, 0, 0, 0.7 }, { 0, 1, 0, 0.7 }
                -- Rings that land on a keystone rather than on a socket, by
                -- the keystone's id. From Nothing rings the keystones it
                -- names, with a fixed inner ring of 150. A keystone that
                -- works like one (spec.intuitiveLeapLikeNodes) puts the
                -- outer pair on every allocated keystone.
                local keystoneRings = {}
                local function ringKeystone(k, set)
                    if not (k and k.id and k.x and k.y) then return end
                    keystoneRings[k.id] = keystoneRings[k.id] or {}
                    table.insert(keystoneRings[k.id], set)
                end
                for sid in pairs(t.sockets or {}) do
                    local node = (spec.nodes or {})[sid]
                    local jewel = node and socketJewel(node, sid)
                    local rad = jewel and jewel.jewelRadiusIndex and radii[jewel.jewelRadiusIndex]
                    local keys = jewel and jewel.title == "From Nothing"
                        and jewel.jewelData and jewel.jewelData.fromNothingKeystones
                    if keys and rad then
                        for name in pairs(keys) do
                            ringKeystone(t.keystoneMap and t.keystoneMap[name],
                                { shadedRings(rad.outer * reach, 150), rad.outer * reach, NEUTRAL })
                        end
                    end
                end
                for _, effect in ipairs(spec.intuitiveLeapLikeNodes or {}) do
                    local rad = radii[effect.radiusIndex]
                    if rad and effect.from == "Keystone" then
                        for name, k in pairs(t.keystoneMap or {}) do
                            -- keystoneMap holds each name twice, once in
                            -- lower case.
                            if k and alloc[k.id] and name == k.name then
                                ringKeystone(k, { shadedRings(rad.outer * reach, 0), rad.outer * reach, NEUTRAL })
                            end
                        end
                    end
                end
                -- spec.nodes is PoB's live node set: the tree's nodes plus
                -- the cluster jewel subgraphs it generates for socketed
                -- cluster jewels, positioned by ProcessNode. The static
                -- tree never has those.
                local allNodes = spec.nodes or t.nodes or {}

                -- The spec being compared against, if compare mode is on.
                -- PoB's whole Compare feature on the tree is this: recolour
                -- against a second spec. `cmp` nil means compare is off and
                -- every node reports no diff.
                local cmp = b.treeTab and b.treeTab.isComparing
                    and b.treeTab.specList
                    and b.treeTab.specList[b.treeTab.activeCompareSpec or 1]
                    or nil
                local cmpAlloc = (cmp and cmp.allocNodes) or {}

                -- Mirrors GetCompareNodeColor. Returns "", "gained", "lost"
                -- or "changed"; the fourth case (a node absent from the
                -- active spec entirely) is handled separately below, since
                -- those have no entry to attach a diff to.
                local function nodeDiff(node)
                    if not cmp then return "" end
                    local id = node.id
                    local cNode = cmp.nodes and cmp.nodes[id]
                    if not cNode then return "" end
                    local a, c = alloc[id] ~= nil, cmpAlloc[id] ~= nil
                    if c and not a then return "gained" end
                    if a and not c then return "lost" end
                    if not (a and c) then return "" end
                    if node.type == "Mastery" then
                        -- PoB compares node.sd table *identity* here, which
                        -- is unreliable across specs. Compare the chosen
                        -- effect instead, which is what it means to ask.
                        local mine = mastery[id] or mastery[tostring(id)]
                        local theirs = cmp.masterySelections
                            and (cmp.masterySelections[id]
                                 or cmp.masterySelections[tostring(id)])
                        if tostring(mine) ~= tostring(theirs) then
                            return "changed"
                        end
                    elseif node.type == "Socket" then
                        local mineId = spec.jewels and spec.jewels[id]
                        local theirsId = cmp.jewels and cmp.jewels[id]
                        if tostring(mineId) ~= tostring(theirsId) then
                            return "changed"
                        end
                    end
                    return ""
                end

                -- A node is "reachable" when it neighbours an allocated one.
                -- PoB draws those with the CanAllocate frame, which is what
                -- shows the frontier of a build.
                local reachable = {}
                for id in pairs(alloc) do
                    local n = allNodes[id]
                    for _, other in ipairs((n and n.linkedId) or {}) do
                        if not alloc[other] then reachable[other] = true end
                    end
                end

                local nodes = {}
                for id, node in pairs(allNodes) do
                    -- Nodes without a position are not drawable (proxies,
                    -- cluster placeholders and the like).
                    if node.x and node.y then
                        local stats = {}
                        for _, line in ipairs(node.sd or {}) do
                            stats[#stats + 1] = tostring(line)
                        end
                        nodes[#nodes + 1] = {
                            tonumber(id) or 0,
                            tostring(node.type or "Normal"),
                            tostring(node.dn or node.name or ""),
                            node.x,
                            node.y,
                            alloc[id] ~= nil,
                            reachable[id] ~= nil,
                            nodeDiff(node),
                            node.ascendancyName and tostring(node.ascendancyName) or "",
                            stats,
                            -- Orbit data, so same-orbit edges can be drawn as
                            -- arcs using PoB's own angles rather than as
                            -- chords across the wheel.
                            node.group and node.group.x or 0,
                            node.group and node.group.y or 0,
                            node.angle or 0,
                            node.o or 0,
                            node.g or 0,
                            -- Ascendant's mirrored notables live under
                            -- .../passives/Ascendants/ in the icon path.
                            (node.icon and tostring(node.icon):find("/Ascendants/")) and 1 or 0,
                            -- Node icon, PoB2 form: the icon *path*
                            -- ("Art/2DArt/SkillIcons/.../x.dds"). The art
                            -- layer resolves it through the tree's ddsCoords
                            -- to a DDS sheet + slice and decodes it; unlike
                            -- PoB1 there is no sub-rect. Sockets carry no
                            -- atlas icon (they draw from nodeOverlay frames),
                            -- and PoB2 has not wired node.sprites at all
                            -- (PassiveTree.lua leaves that resolution TODO),
                            -- so node.icon is the only handle we have.
                            -- The exception is an OnlyImage node, the picture
                            -- in the middle of a group: PoB2 draws its
                            -- activeEffectImage as the base, and its node.icon
                            -- (a MasteryGroup* path) is packed nowhere. On
                            -- every other node activeEffectImage is the glow
                            -- under the node, reported separately below, and
                            -- must not stand in for the icon.
                            (function()
                                if node.type == "Socket" then return "" end
                                if node.type == "OnlyImage" then
                                    return node.activeEffectImage
                                        and tostring(node.activeEffectImage) or ""
                                end
                                return node.icon and tostring(node.icon) or ""
                            end)(),
                            -- Mastery: the chosen effect and its text, plus
                            -- how many options exist. Only the choice is
                            -- carried; the full list would overflow a tooltip.
                            (function()
                                if node.type ~= "Mastery" then
                                    return { 0, {}, 0 }
                                end
                                local picked = mastery[node.id] or mastery[tostring(node.id)]
                                local stats = {}
                                if picked then
                                    for _, e in ipairs(node.masteryEffects or {}) do
                                        if tostring(e.effect) == tostring(picked) then
                                            for _, line in ipairs(e.stats or {}) do
                                                stats[#stats + 1] = tostring(line)
                                            end
                                        end
                                    end
                                end
                                -- A table, not three values: a multi-value
                                -- call only expands in the *last* position of
                                -- a Lua table constructor. This used to be
                                -- last and silently collapsed to one value
                                -- the moment a field was appended after it.
                                return { tonumber(picked) or 0, stats,
                                         #(node.masteryEffects or {}) }
                            end)(),
                            -- The frame ("overlay") asset, resolved the way
                            -- PoB resolves it. This is a lookup into the
                            -- tree's own nodeOverlay table rather than a
                            -- name we compose, because the key is a single
                            -- concatenated string -- state plus Ascend,
                            -- Blighted or Alt -- and the variants are not
                            -- regular: Keystones have Blighted entries that
                            -- are just the plain frames again, Sockets swap
                            -- Alt for Ascend, and Masteries have no variants
                            -- at all. Rebuilding that table in Rust would be
                            -- three near-duplicates of PoB's data that drift
                            -- the moment a league adds a fourth.
                            (function()
                                -- PoB2 gives ascendancy nodes their own fancy
                                -- ring in node.nodeOverlay (e.g.
                                -- PathfinderFrameSmallAllocated), keyed by the
                                -- plain state; fall back to the generic
                                -- node.overlay for everything else.
                                local ov = node.nodeOverlay or node.overlay
                                if not ov then return { "", 0, 0, 0, 0, "" } end
                                -- Masteries (isOnlyImage) draw their pattern
                                -- alone, no ring. Cluster "Nothingness"
                                -- masteries included.
                                if node.type == "Mastery" or node.isOnlyImage then
                                    return { "", 0, 0, 0, 0, "" }
                                end
                                local state = "unalloc"
                                if alloc[node.id] or cmpAlloc[node.id] then
                                    state = "alloc"
                                elseif reachable[node.id] then state = "path" end

                                local key = state
                                if node.type == "Socket" then
                                    key = key .. (node.expansionJewel and "Alt" or "")
                                else
                                    key = key
                                        .. (node.ascendancyName and "Ascend" or "")
                                        .. (node.isBlighted and "Blighted" or "")
                                end
                                local name = ov[key] or ov[state]
                                if not name then return { "", 0, 0, 0, 0, "" } end

                                -- Abyss-conquered notables and keystones get
                                -- their own frames, when the art exists.
                                local conq = node.conqueredBy
                                    and node.conqueredBy.conqueror
                                if conq and conq.type
                                    and tostring(conq.type):match("^abyss_")
                                    and (node.type == "Notable"
                                         or node.type == "Keystone") then
                                    local frameType = node.ascendancyName
                                        and "Ascendancy" or node.type
                                    local frameState = state == "alloc"
                                        and "Allocated"
                                        or (state == "path" and "CanAllocate"
                                            or "Unallocated")
                                    local abyss = "Abyss" .. frameType
                                        .. "Frame" .. frameState
                                    if t.assets[abyss] then name = abyss end
                                end

                                -- Ascendancy frames are per-bloodline where
                                -- that bloodline ships its own art. Those
                                -- live *in* bloodline-3.webp as sprite rects,
                                -- not as files on disk, so the prefixed name
                                -- alone would name nothing loadable. Return
                                -- the rect and its sheet instead, and let the
                                -- plain name stand for the file case.
                                local sheet, rect = "", nil
                                if node.ascendancyName then
                                    local prefix = node.bloodlineOverlayPrefix
                                        or (t.bloodlineSpritePrefixes
                                            and t.bloodlineSpritePrefixes[node.ascendancyName])
                                    local a = prefix and t.assets[prefix .. name]
                                    if a and a[1] then
                                        rect = { a[1], a[2], a[3], a[4] }
                                        -- Every bloodline asset is packed
                                        -- into one sheet. PoB keeps the
                                        -- name->sheet map in a local, so
                                        -- there is nothing to look it up in;
                                        -- the sheet is named directly.
                                        sheet = "bloodline-3.webp"
                                        name = prefix .. name
                                    end
                                end
                                if rect then
                                    return { tostring(name), rect[1], rect[2],
                                             rect[3], rect[4], sheet }
                                end
                                return { tostring(name), 0, 0, 0, 0, "" }
                            end)(),
                            -- Effect art drawn *over* the node: the glow that
                            -- marks a tattoo, or a mastery whose effect has
                            -- been chosen. PoB draws this as a second sprite
                            -- on top of the base one; without it an allocated
                            -- mastery is indistinguishable from an unallocated
                            -- one at a glance, which is the whole point of
                            -- the state. Trees before 3.22 have no
                            -- effectSprites at all.
                            (function()
                                -- PoB2: the image under a notable that lights
                                -- up when it is allocated. A path resolved
                                -- through ddsCoords, carried in the sixth
                                -- slot; there is no rect or sheet.
                                if node.activeEffectImage
                                    and node.type ~= "OnlyImage"
                                    and node.type ~= "Socket" then
                                    return { 0, 0, 0, 0, "",
                                             tostring(node.activeEffectImage) }
                                end
                                local e, kind
                                if node.isTattoo and node.effectSprites then
                                    e = node.effectSprites.tattooActiveEffect
                                    kind = "tattooActiveEffect"
                                elseif node.type == "Mastery"
                                    and alloc[node.id]
                                    and node.masteryEffects
                                    and node.masterySprites
                                    and node.masterySprites.activeEffectImage then
                                    e = node.masterySprites
                                        .activeEffectImage.masteryActiveEffect
                                    kind = "masteryActiveEffect"
                                end
                                if not e or not e[1] then
                                    return { 0, 0, 0, 0, "" }
                                end
                                local sheet = ""
                                local entry = t.skillSprites
                                    and t.skillSprites[kind]
                                if entry then
                                    local z = entry[0.3835] or entry[#entry] or entry
                                    local f = z and z.filename
                                    if f then
                                        sheet = tostring(f):gsub("%?%x+$", "")
                                                           :gsub(".*/", "")
                                    end
                                end
                                return { e[1], e[2], e[3], e[4], sheet }
                            end)(),
                            -- Radius rings, mirroring PassiveTreeView's socket
                            -- loop: sockets allocated in either spec, not
                            -- charm sockets, cluster sockets only at size 2.
                            -- Nor a socket inside another passive, nor one
                            -- marked as having no radius.
                            -- A jewel the compared spec lacks is tinted red,
                            -- one only it has is drawn green.
                            (function()
                                if node.type == "Keystone" then
                                    return keystoneRings[node.id] or {}
                                end
                                if node.type ~= "Socket" then return {} end
                                if node.name == "Charm Socket" then return {} end
                                if node.containJewelSocket == true or node.noRadius then return {} end
                                if node.expansionJewel and node.expansionJewel.size ~= 2 then return {} end
                                if not (alloc[id] or cmpAlloc[id]) then return {} end
                                local jewel = socketJewelAny(node, id)
                                local cJewel = compareJewel(id)
                                local same = sameJewel(jewel, cJewel)
                                local sets = {}
                                local primary = ringsFor(jewel, (not cmp or same) and NEUTRAL or PRIMARY_ONLY)
                                if primary then sets[#sets + 1] = primary end
                                if not same then
                                    local compare = ringsFor(cJewel, COMPARE_ONLY)
                                    if compare then sets[#sets + 1] = compare end
                                end
                                return sets
                            end)(),
                            -- The jewel in the socket: the art PoB lays over
                            -- the socket frame for it (GetJewelSocketOverlay),
                            -- and what its tooltip says, since PoB shows the
                            -- item's tooltip on a filled socket, not the
                            -- socket's name.
                            (function()
                                -- Any socket holding a jewel, allocated or
                                -- not: an unallocated socket's jewel does
                                -- nothing, and PoB draws nothing for it, so
                                -- the tooltip has to say it is there.
                                local jewel = socketJewelAny(node, id)
                                if not jewel then return {} end
                                local active = alloc[id] and true or false
                                local base = jewel.baseName
                                -- PoB2 names the art after the jewel: a
                                -- unique's own title where the tree carries
                                -- art under it, otherwise the base ("Ruby").
                                local overlay = tostring(base or "")
                                if jewel.rarity == "UNIQUE" and jewel.title then
                                    local own = (t.ddsMap and t.ddsMap[jewel.title])
                                        or (t.assets and t.assets[jewel.title])
                                        or (t.spriteMap and t.spriteMap[jewel.title])
                                    if own then overlay = tostring(jewel.title) end
                                end
                                local lines = {}
                                for _, group in ipairs({ jewel.enchantModLines, jewel.implicitModLines, jewel.explicitModLines }) do
                                    for _, m in ipairs(group or {}) do
                                        -- Only the chosen variant's lines: the
                                        -- finder's jewels carry one line per
                                        -- conqueror and pick one.
                                        local shown = not m.variantList
                                            or (jewel.variant and m.variantList[jewel.variant])
                                            or (jewel.variantAlt and m.variantList[jewel.variantAlt])
                                        if m.line and shown then lines[#lines + 1] = tostring(m.line) end
                                    end
                                end
                                local name = tostring(jewel.name or jewel.title or base or "Jewel")
                                if jewel.title and jewel.title ~= "" and base and base ~= "" and jewel.rarity == "UNIQUE" then
                                    name = tostring(jewel.title) .. ", " .. tostring(base)
                                end
                                -- A timeless jewel is its seed; say which.
                                local seed = jewel.jewelData and jewel.jewelData.conqueredBy and jewel.jewelData.conqueredBy.id
                                if seed and not name:find(tostring(seed), 1, true) then
                                    name = name .. " (" .. tostring(seed) .. ")"
                                end
                                -- What the jewel did to the passives in its
                                -- radius. spec.nodes are copies over the tree
                                -- nodes, and ReplaceNode and the additions
                                -- write into the copy, so the base tree still
                                -- says what each passive was.
                                local changes = {}
                                local radiusIdx = jewel.jewelRadiusIndex
                                local socketNode = t.nodes[tonumber(id)]
                                local inRadius = radiusIdx and socketNode and socketNode.nodesInRadius
                                    and socketNode.nodesInRadius[radiusIdx]
                                for otherId in pairs(inRadius or {}) do
                                    local live, base = spec.nodes[otherId], t.nodes[otherId]
                                    if live and base and (base.type == "Notable" or base.type == "Keystone" or base.type == "Normal") then
                                        local liveName, baseName = tostring(live.dn or ""), tostring(base.dn or "")
                                        if liveName ~= baseName and liveName ~= "" then
                                            changes[#changes + 1] = { baseName, liveName, true, alloc[otherId] and true or false, base.type == "Notable" and 1 or (base.type == "Keystone" and 0 or 2) }
                                        else
                                            local have = {}
                                            for _, l in ipairs(base.sd or {}) do have[tostring(l)] = true end
                                            local added = {}
                                            for _, l in ipairs(live.sd or {}) do
                                                local line = tostring(l)
                                                -- PoB inserts a blank line before the addition.
                                                if not have[line] and line:match("%S") then added[#added + 1] = line end
                                            end
                                            if #added > 0 then
                                                changes[#changes + 1] = { baseName, table.concat(added, "; "), false, alloc[otherId] and true or false, base.type == "Notable" and 1 or (base.type == "Keystone" and 0 or 2) }
                                            end
                                        end
                                    end
                                end
                                -- Keystones first, then notables, then small
                                -- passives; allocated before not, by name.
                                table.sort(changes, function(x, y)
                                    if x[5] ~= y[5] then return x[5] < y[5] end
                                    if x[4] ~= y[4] then return x[4] end
                                    return x[1] < y[1]
                                end)
                                -- PoB lays the overlay only on an allocated socket.
                                if not active then overlay = "" end
                                return { overlay, name, lines, tostring(jewel.rarity or "NORMAL"),
                                         jewel.jewelRadiusLabel == "Variable", changes, active }
                            end)(),
                            -- A socket PoB2 shows no radius for, hovered or
                            -- filled: a charm's, one inside another passive,
                            -- one the tree marks as having none.
                            (node.name == "Charm Socket" or node.containJewelSocket == true
                                or node.noRadius) and 1 or 0,
                            -- A socket an item grants, which no point buys.
                            node.sinister and 1 or 0,
                        }
                    end
                end

                local connectors = {}
                local lineFamilies = {}
                -- The tree's own connectors, then each cluster subgraph's.
                local connSets = { t.connectors or {} }
                for _, sg in pairs(spec.subGraphs or {}) do
                    connSets[#connSets + 1] = sg.connectors or {}
                end
                for _, set in ipairs(connSets) do
                for _, conn in pairs(set) do
                    local a, bId = conn.nodeId1, conn.nodeId2
                    local n1, n2 = allNodes[a], allNodes[bId]
                    if n1 and n2 and n1.x and n2.x then
                        -- PoB draws connectors as textured quads whose vertex
                        -- coordinates are filled in at draw time, in screen
                        -- space — before that they are degenerate. So the
                        -- quads are not usable as geometry here. Instead we
                        -- report the endpoints plus enough orbit data for the
                        -- caller to sweep an arc using PoB's own angles.
                        local isArc = conn.type and conn.type ~= "LineConnector"
                        local path = { { n1.x, n1.y }, { n2.x, n2.y } }

                        -- PoB colours a connector from the endpoints' own
                        -- `alloc` flag (see PassiveTreeView getState), not the
                        -- allocNodes map. They usually agree, but a mastery
                        -- whose effect was just set has node.alloc true before
                        -- it reappears in the map, so reading the map dropped
                        -- its lines to "intermediate" (dark). Read the flag,
                        -- and keep the map as a fallback.
                        local aAlloc = n1.alloc == true or alloc[a] ~= nil
                        local bAlloc = n2.alloc == true or alloc[bId] ~= nil
                        -- PoB2's getState with nothing hovered: lit between
                        -- two taken nodes, plain otherwise. Its in-between
                        -- art is for the path under the pointer, not for a
                        -- line with one end taken. Nodes taken for different
                        -- weapon sets do not join.
                        local mode1, mode2 = n1.allocMode or 0, n2.allocMode or 0
                        local state = "normal"
                        if aAlloc and bAlloc and (mode1 == 0 or mode2 == 0 or mode1 == mode2) then
                            state = "active"
                        end
                        -- The colour PoB2 multiplies the art by: a weapon
                        -- set's on a lit line, and 0.75 grey on the lines of
                        -- every ascendancy but the build's own.
                        local tint = ""
                        if state == "active" and not conn.ascendancyName then
                            local mode = (mode1 ~= 0 and mode1) or (mode2 ~= 0 and mode2) or 0
                            if mode == 1 then tint = colorCodes.NEGATIVE
                            elseif mode == 2 then tint = colorCodes.POSITIVE end
                        end
                        if conn.ascendancyName and conn.ascendancyName ~= spec.curAscendClassBaseName then
                            tint = "^xBFBFBF"
                        end
                        local family = tostring(conn.connectionArt
                            or (t.connectionArt and t.connectionArt.default) or "Character")
                        lineFamilies[family] = true

                        -- An arc's quad is a kite in tree space: its first
                        -- corner is the centre the curve turns about and its
                        -- third lies out along the middle of the curve. PoE2
                        -- needs both, because a connection naming its own
                        -- orbit turns about a point that is not the group's,
                        -- and only the middle says which way round it goes.
                        local kite = isArc and conn.vert and conn.vert.Normal
                        local arc = kite and { kite[1], kite[2], kite[5], kite[6] } or nil

                        connectors[#connectors + 1] = {
                            a, bId, state, path,
                            isArc and 1 or 0,
                            tostring(conn.type or "LineConnector"),
                            arc,
                            family,
                            tostring(tint),
                        }
                    end
                end
                end

                local groups = {}
                local groupSets = { t.groups or {} }
                for _, sg in pairs(spec.subGraphs or {}) do
                    if sg.group then groupSets[#groupSets + 1] = { sg.group } end
                end
                for si, gset in ipairs(groupSets) do
                -- Everything after the tree's own set is a cluster subgraph,
                -- which PoB draws with the expansion ("Alt") ring art.
                local expansion = si > 1 and 1 or 0
                for _, g in pairs(gset) do
                    -- Proxy groups exist for layout bookkeeping and get no
                    -- ring; PoB skips them (`if not group.isProxy`). Drawing
                    -- them puts a second ring next to a real one, which reads
                    -- as a doubled circle.
                    if g.x and g.y and not g.isProxy then
                        -- PoB picks the ring by *asking* for orbit 3, then 2,
                        -- then 1 — `group.oo[3]` is a lookup, not a maximum.
                        -- Taking the largest orbit present yields sizes like
                        -- 4 that have no art at all.
                        -- PoB2 draws a group background only where the
                        -- tree data names one (`if group.background`), and
                        -- the PoE2 trees name none: the picture in the
                        -- middle of a group is an OnlyImage node instead.
                        -- Deriving a ring from the orbits, as PoE1 does,
                        -- paints art the game does not have.
                        local bg = 0
                        if g.background and g.oo then
                            if g.oo[3] then bg = 3
                            elseif g.oo[2] then bg = 2
                            elseif g.oo[1] then bg = 1 end
                        end
                        groups[#groups + 1] = { g.x, g.y, bg, expansion }
                    end
                end
                end

                -- Drop rings that coincide with one already emitted. A pair
                -- of groups a few units apart both claiming a large ring
                -- draws the same circle twice, which reads as a doubled
                -- outline rather than one crisp ring.
                local deduped = {}
                for _, g in ipairs(groups) do
                    local dup = false
                    for _, kept in ipairs(deduped) do
                        if kept[3] == g[3] then
                            local dx, dy = kept[1] - g[1], kept[2] - g[2]
                            if dx * dx + dy * dy < 100 then
                                dup = true
                                break
                            end
                        end
                    end
                    if not dup then deduped[#deduped + 1] = g end
                end
                groups = deduped

                -- The class backdrop and the start node's own art. PoB
                -- hardcodes the backdrop positions, noting the tree JSON does
                -- not carry them.
                local startArt, startX, startY = "", 0, 0
                for id, node in pairs(t.nodes or {}) do
                    if node.type == "ClassStart" and node.x
                        and node.startArt and alloc[id] then
                        startArt = tostring(node.startArt)
                        startX, startY = node.x, node.y
                        break
                    end
                end

                -- The pictures under the tree, placed as PassiveTreeView
                -- places them and in its order: the class portrait (the
                -- ascendancy's, once one is chosen), the ring turned to point
                -- at the class's start, the frame over both, then every
                -- ascendancy's plate.
                local backdrops = {}
                local scaleImage = t.scaleImage or 1
                local class = t.classes and t.classes[spec.curClassId]
                if class and class.background then
                    local bg = class.background
                    local image = bg.image
                    local asc = spec.curAscendClassId ~= 0 and class.classes
                        and class.classes[spec.curAscendClassId]
                    if asc and asc.background then image = asc.background.image end
                    local x, y = bg.x * scaleImage, bg.y * scaleImage
                    backdrops[#backdrops + 1] = { tostring(image), x, y, bg.width, 0, 0 }
                    local start = spec.nodes[class.startNodeId]
                    if start and start.x and bg.active then
                        local turn = math.pi / 2 + math.atan2(start.y - y, start.x - x)
                        backdrops[#backdrops + 1] = { "BGTreeActive", x, y, bg.active.width, turn, 0 }
                    end
                    if bg.bg then
                        backdrops[#backdrops + 1] = { "BGTree", x, y, bg.bg.width, 0, 0 }
                    end
                end
                local plates = {}
                for name, entry in pairs(t.ascendNameMap or {}) do
                    local asc = entry.ascendClass
                    local drawn = true
                    if asc.replaceBy and asc.replaceBy == spec.curAscendClassBaseName then
                        drawn = false
                    elseif asc.replace and name ~= spec.curAscendClassBaseName then
                        drawn = false
                    end
                    if asc.background and drawn then
                        local bg = asc.background
                        plates[#plates + 1] = {
                            tostring(bg.image), bg.x * scaleImage, bg.y * scaleImage, bg.width, 0,
                            name == spec.curAscendClassBaseName and 0 or 1,
                        }
                    end
                end
                -- pairs() has no order; the plates do not overlap, but a list
                -- that reshuffles between reads is a nuisance to test.
                table.sort(plates, function(a, b) return a[1] < b[1] end)
                for _, plate in ipairs(plates) do backdrops[#backdrops + 1] = plate end

                return {
                    nodes = nodes,
                    connectors = connectors,
                    groups = groups,
                    backdrops = backdrops,
                    -- The file behind each family of line art, per state.
                    -- An arc's art has the same band across it as the
                    -- straight line's, so the line's serves for both.
                    lineArt = (function()
                        local out = {}
                        for family in pairs(lineFamilies) do
                            local files = {}
                            for i, state in ipairs({ "Normal", "Intermediate", "Active" }) do
                                local asset = t.assets and t.assets[family .. "LineConnector" .. state]
                                files[i] = asset and asset[1] and tostring(asset[1]) or ""
                            end
                            out[#out + 1] = { family, files[1], files[2], files[3] }
                        end
                        return out
                    end)(),
                    bounds = { t.min_x or 0, t.min_y or 0, t.max_x or 0, t.max_y or 0 },
                    -- PoB's radius table for this tree version, with the
                    -- colour it draws each radius in on a hovered socket.
                    radii = (function()
                        local out = {}
                        for _, r in ipairs(radii) do
                            out[#out + 1] = { r.inner * reach, r.outer * reach, tostring(r.col or ""), tostring(r.label or "") }
                        end
                        return out
                    end)(),
                    version = tostring(t.treeVersion or ""),
                    classId = spec.curClassId or 0,
                    startArt = startArt,
                    startX = startX,
                    startY = startY,
                }
                "#,
            )?.call(())?;

        let mut out = tree::Tree {
            version: raw.get::<Option<String>>("version")?.unwrap_or_default(),
            ..Default::default()
        };

        if let Some(families) = raw.get::<Option<Table>>("lineArt")? {
            for entry in families.sequence_values::<Table>() {
                let f = entry?;
                out.line_art.insert(f.get(1)?, [f.get(2)?, f.get(3)?, f.get(4)?]);
            }
        }

        if let Some(backdrops) = raw.get::<Option<Table>>("backdrops")? {
            for entry in backdrops.sequence_values::<Table>() {
                let b = entry?;
                out.backdrops.push(tree::Backdrop {
                    key: b.get(1)?,
                    x: b.get(2)?,
                    y: b.get(3)?,
                    half: b.get(4)?,
                    turn: b.get(5)?,
                    dimmed: b.get::<u8>(6)? == 1,
                });
            }
        }

        if let Some(b) = raw.get::<Option<Table>>("bounds")? {
            out.bounds = (b.get(1)?, b.get(2)?, b.get(3)?, b.get(4)?);
        }
        if let Some(radii) = raw.get::<Option<Table>>("radii")? {
            for entry in radii.sequence_values::<Table>() {
                let r = entry?;
                let col: String = r.get(3)?;
                // "^xRRGGBB", PoB's colour escape.
                let hex = col.trim_start_matches("^x");
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
                out.jewel_radii.push(tree::JewelRadius {
                    inner: r.get(1)?,
                    outer: r.get(2)?,
                    colour: [byte(0), byte(2), byte(4)],
                    label: r.get(4)?,
                });
            }
        }

        let nodes: Table = raw.get("nodes")?;
        for entry in nodes.sequence_values::<Table>() {
            let n = entry?;
            let ascendancy: String = n.get(9)?;
            let stat_table: Table = n.get(10)?;
            out.nodes.push(tree::Node {
                id: n.get(1)?,
                kind: tree::NodeKind::parse(&n.get::<String>(2)?),
                name: n.get(3)?,
                x: n.get(4)?,
                y: n.get(5)?,
                allocated: n.get(6)?,
                reachable: n.get(7)?,
                compare: {
                    let d: String = n.get(8)?;
                    tree::NodeDiff::parse(&d)
                },
                ascendancy: (!ascendancy.is_empty()).then_some(ascendancy),
                stats: stat_table
                    .sequence_values::<String>()
                    .collect::<Result<_, _>>()?,
                group_x: n.get(11)?,
                group_y: n.get(12)?,
                angle: n.get(13)?,
                orbit: n.get(14)?,
                group: n.get(15)?,
                is_ascendant_mirror: n.get::<u8>(16)? == 1,
                icon: {
                    // PoB2: field 17 is the icon path, resolved through
                    // ddsCoords in the art layer. No sub-rect or sheet here.
                    let key: String = n.get(17)?;
                    (!key.is_empty()).then(|| tree::Icon {
                        px: (0.0, 0.0, 0.0, 0.0),
                        sheet: String::new(),
                        key,
                    })
                },
                mastery_choice: {
                    let mastery: Table = n.get(18)?;
                    let effect_id: u64 = mastery.get(1)?;
                    let stat_table: Table = mastery.get(2)?;
                    let stats: Vec<String> = stat_table
                        .sequence_values::<String>()
                        .collect::<Result<_, _>>()?;
                    (effect_id != 0).then_some(tree::MasteryChoice { effect_id, stats })
                },
                mastery_effect_count: {
                    let mastery: Table = n.get(18)?;
                    mastery.get(3)?
                },
                frame: {
                    let f: Table = n.get(19)?;
                    let name: String = f.get(1)?;
                    let (x0, y0, x1, y1): (f32, f32, f32, f32) =
                        (f.get(2)?, f.get(3)?, f.get(4)?, f.get(5)?);
                    let sheet: String = f.get(6)?;
                    (!name.is_empty()).then(|| tree::Frame {
                        name,
                        atlas: (x1 > x0 && y1 > y0 && !sheet.is_empty()).then_some({
                            tree::Icon {
                                px: (x0, y0, x1, y1),
                                sheet,
                                key: String::new(),
                            }
                        }),
                    })
                },
                effect: {
                    let rect: Table = n.get(20)?;
                    let (x0, y0, x1, y1): (f32, f32, f32, f32) =
                        (rect.get(1)?, rect.get(2)?, rect.get(3)?, rect.get(4)?);
                    let sheet: String = rect.get(5)?;
                    // PoB2 reports the glow as a ddsCoords path in slot six.
                    let key: String = rect.get::<Option<String>>(6)?.unwrap_or_default();
                    ((x1 > x0 && y1 > y0 && !sheet.is_empty()) || !key.is_empty()).then_some(
                        tree::Icon {
                            px: (x0, y0, x1, y1),
                            sheet,
                            key,
                        },
                    )
                },
                jewel_rings: {
                    let sets: Table = n.get(21)?;
                    let mut out = Vec::new();
                    for set in sets.sequence_values::<Table>() {
                        let r = set?;
                        let list: Table = r.get(1)?;
                        let mut sprites = Vec::new();
                        for entry in list.sequence_values::<Table>() {
                            let e = entry?;
                            sprites.push(tree::RingSprite {
                                art: e.get(1)?,
                                radius: e.get(2)?,
                                speed: e.get(3)?,
                            });
                        }
                        let t: Table = r.get(3)?;
                        let c = |i: i64| -> Result<u8, Error> {
                            let v: f32 = t.get(i)?;
                            Ok((v * 255.0).round() as u8)
                        };
                        // Premultiplied, as SetDrawColor's alpha applies to
                        // the whole sprite.
                        let a = c(4)?;
                        let pre = |v: u8| ((v as u32 * a as u32) / 255) as u8;
                        out.push(tree::JewelRings {
                            sprites,
                            outer: r.get(2)?,
                            tint: [pre(c(1)?), pre(c(2)?), pre(c(3)?), a],
                        });
                    }
                    out
                },
                no_radius: n.get::<Option<u8>>(23)?.unwrap_or(0) == 1,
                granted_only: n.get::<Option<u8>>(24)?.unwrap_or(0) == 1,
                socket_jewel: {
                    let j: Table = n.get(22)?;
                    match j.get::<Option<String>>(2)? {
                        Some(name) => {
                            let overlay: String = j.get(1)?;
                            let lines: Table = j.get(3)?;
                            Some(tree::SocketJewel {
                                overlay: (!overlay.is_empty()).then_some(overlay),
                                name,
                                lines: lines.sequence_values::<String>().collect::<Result<_, _>>()?,
                                rarity: j.get(4)?,
                                variable_radius: j.get::<Option<bool>>(5)?.unwrap_or(false),
                                active: j.get::<Option<bool>>(7)?.unwrap_or(true),
                                changes: {
                                    let mut out = Vec::new();
                                    if let Some(list) = j.get::<Option<Table>>(6)? {
                                        for entry in list.sequence_values::<Table>() {
                                            let c = entry?;
                                            out.push(tree::PassiveChange {
                                                from: c.get(1)?,
                                                to: c.get(2)?,
                                                replaced: c.get(3)?,
                                                allocated: c.get(4)?,
                                                small: c.get::<i64>(5)? == 2,
                                            });
                                        }
                                    }
                                    out
                                },
                            })
                        }
                        None => None,
                    }
                },
            });
        }

        out.class_art = raw
            .get::<Option<u8>>("classId")?
            .and_then(tree::ClassArt::from_class_id);
        out.start_art = raw
            .get::<Option<String>>("startArt")?
            .filter(|s| !s.is_empty());
        if out.start_art.is_some() {
            out.start_pos = Some((raw.get("startX")?, raw.get("startY")?));
        }

        out.wheels = tree::Tree::compute_wheels(&out.nodes);

        // Index nodes so arcs can look up their endpoints' orbit data.
        let by_id: std::collections::HashMap<u64, usize> = out
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();

        let mut swept = std::collections::HashSet::new();
        let connectors: Table = raw.get("connectors")?;
        for entry in connectors.sequence_values::<Table>() {
            let c = entry?;
            let state = match c.get::<String>(3)?.as_str() {
                "active" => tree::ConnectorState::Active,
                "intermediate" => tree::ConnectorState::Intermediate,
                _ => tree::ConnectorState::Normal,
            };
            let from: u64 = c.get(1)?;
            let to: u64 = c.get(2)?;
            let is_arc: u8 = c.get(5)?;
            let is_arc = is_arc == 1;

            let path_table: Table = c.get(4)?;
            let mut path = Vec::new();
            for point in path_table.sequence_values::<Table>() {
                let pt = point?;
                path.push((pt.get(1)?, pt.get(2)?));
            }

            // Sweep arcs from one node to the other about the centre PoB2
            // itself turns them on. Left as a chord, these would cut visibly
            // across every wheel in the tree, and arcs are the majority of
            // edges.
            if is_arc
                && path.len() == 2
                && let Some(kite) = c.get::<Option<Table>>(7)?
            {
                let centre = (kite.get(1)?, kite.get(2)?);
                let through = (kite.get(3)?, kite.get(4)?);
                // PoB2 draws a sweep past 90 degrees as two quads. Each one
                // names the whole curve, so the second adds nothing.
                let key = (from.min(to), to.max(from), centre.0 as i64, centre.1 as i64);
                if !swept.insert(key) {
                    continue;
                }
                // Enough segments that the curve reads as smooth at the zoom
                // levels the tree is actually viewed at.
                path = tree::arc_between(centre, path[0], path[1], through, 16);
            }

            // Bounds now, once, rather than per frame in the renderer.
            let bounds = path.iter().fold(
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                |(x0, y0, x1, y1), (x, y)| (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)),
            );

            // Whether this line's active-ness differs between the specs,
            // derived from the endpoints rather than asked of Lua again.
            //
            // PoB's rule: a connector active in the compared spec but not the
            // active one is forced to its Active art and tinted green, so a
            // gained path is *visible* rather than simply absent. The reverse
            // is tinted red. Everything else is untouched.
            let compare = match (by_id.get(&from), by_id.get(&to)) {
                (Some(&i), Some(&j)) => {
                    let (a, b) = (&out.nodes[i], &out.nodes[j]);
                    let active_now = a.allocated && b.allocated;
                    let alloc_in_compare = |n: &tree::Node| match n.compare {
                        Some(tree::NodeDiff::Gained) => true,
                        Some(tree::NodeDiff::Lost) => false,
                        _ => n.allocated,
                    };
                    let active_there = alloc_in_compare(a) && alloc_in_compare(b);
                    match (active_now, active_there) {
                        (false, true) => Some(tree::NodeDiff::Gained),
                        (true, false) => Some(tree::NodeDiff::Lost),
                        _ => None,
                    }
                }
                _ => None,
            };

            let tint: String = c.get::<Option<String>>(9)?.unwrap_or_default();
            let tint = tint.strip_prefix("^x").and_then(|hex| {
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                Some([byte(0)?, byte(2)?, byte(4)?])
            });
            out.connectors.push(tree::Connector {
                art: c.get::<Option<String>>(8)?.unwrap_or_default(),
                tint,
                compare,
                from,
                to,
                state,
                path,
                is_arc,
                bounds,
            });
        }

        let groups: Table = raw.get("groups")?;
        for entry in groups.sequence_values::<Table>() {
            let g = entry?;
            let bg: u8 = g.get(3)?;
            out.groups.push(tree::Group {
                x: g.get(1)?,
                y: g.get(2)?,
                background: (bg > 0).then_some(bg),
                expansion: g.get::<u8>(4)? == 1,
            });
        }

        Ok(out)
    }

    /// Read every displayable stat for a build without disturbing the one
    /// currently loaded.
    ///
    /// Comparison needs two builds' numbers at once, and PoB holds exactly
    /// one. So this saves the current build, loads the other, reads it, and
    /// restores — the caller never sees the swap.
    pub fn stats_for(
        &self,
        other_xml: &str,
        keys: &[&str],
    ) -> Result<Vec<(String, Option<f64>)>, Error> {
        let saved = self.build_xml()?;

        let result = (|| {
            self.load_build_xml(other_xml)?;
            self.stats(keys)
        })();

        // Restore even if the other build failed to load, so a bad paste
        // cannot cost the user the build they were working on.
        self.load_build_xml(&saved)?;
        result
    }

    /// Serialise the current build back to PoB's build XML.
    ///
    /// `SaveDB("code")` is what PoB itself calls when producing an export
    /// code, so the XML matches what upstream would write.
    pub fn build_xml(&self) -> Result<String, Error> {
        Ok(self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                -- SaveDB writes the <PlayerStat> block from calcsTab.mainOutput.
                -- After an import the build can be saved before a calc pass
                -- has populated that output, so the file lands with no stats
                -- and the library has no DPS/EHP to show. Settle one pass if
                -- the headline number is missing before exporting.
                local out = b.calcsTab and b.calcsTab.mainOutput
                if not out or not out.TotalDPS then
                    b.buildFlag = true
                    runCallback("OnFrame")
                end
                return b:SaveDB("code") or ""
                "#,
            )?.call::<String>(())?)
    }

    /// The build's notes.
    ///
    /// PoB keeps the text in its editor control's buffer rather than in a
    /// plain field, so this reads `notesTab.controls.edit.buf`.
    pub fn notes(&self) -> Result<String, Error> {
        Ok(self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local edit = b.notesTab and b.notesTab.controls and b.notesTab.controls.edit
                return (edit and edit.buf) or ""
                "#,
            )?.call::<String>(())?)
    }

    /// Replace the build's notes.
    ///
    /// Notes do not feed the calculations, so unlike a config change this
    /// needs no rebuild — only the modification flag PoB uses to know the
    /// build is unsaved.
    pub fn set_notes(&self, text: &str) -> Result<(), Error> {
        self.chunk(
                r#"
                local text = ...
                local b = __mainObject__.main.modes["BUILD"]
                local edit = b.notesTab and b.notesTab.controls and b.notesTab.controls.edit
                if edit then
                    edit.buf = text
                    b.notesTab.modFlag = (b.notesTab.lastContent ~= text)
                end
                "#,
            )?.call::<()>(text)?;
        Ok(())
    }

    /// Header facts about the loaded build.
    /// The current tree as a pathofexile.com passive-skill-tree link.
    ///
    /// Encoded by PoB's own `EncodeURL`, so the link is the same one upstream
    /// would produce and is understood by the official site, poeplanner and
    /// every build guide.
    /// Undo or redo on one of PoB's undo handlers: the tree (`spec`), or
    /// the skills, items or config tab. Each keeps its own history, as PoB
    /// does. Returns (can_undo, can_redo) afterwards.
    pub fn undo_redo(&self, scope: &str, redo: bool) -> Result<(bool, bool), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local scope, redo = ...
                local b = __mainObject__.main.modes["BUILD"]
                local h = (scope == "tree" and b.spec) or (scope == "skills" and b.skillsTab)
                    or (scope == "items" and b.itemsTab) or (scope == "config" and b.configTab)
                if not h then return false, false end
                if redo then h:Redo() else h:Undo() end
                b.buildFlag = true
                runCallback("OnFrame")
                return h.undo[2] ~= nil, h.redo[1] ~= nil
                "#,
            )?;
        let (u, r): (bool, bool) = f.call((scope, redo))?;
        Self::check_prompt(&self.lua)?;
        Ok((u, r))
    }

    /// Whether there is anything to undo or redo in a scope.
    pub fn undo_state(&self, scope: &str) -> Result<(bool, bool), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local scope = ...
                local b = __mainObject__.main.modes["BUILD"]
                local h = (scope == "tree" and b.spec) or (scope == "skills" and b.skillsTab)
                    or (scope == "items" and b.itemsTab) or (scope == "config" and b.configTab)
                if not h or not h.undo then return false, false end
                return h.undo[2] ~= nil, h.redo[1] ~= nil
                "#,
            )?;
        Ok(f.call(scope)?)
    }

    /// Read one of PoB's application options (`main.showThousandsSeparators`
    /// and friends), as a string: "true"/"false", a number, or text.
    pub fn main_option(&self, name: &str) -> Result<Option<String>, Error> {
        let f: mlua::Function = self.chunk(r#"local name = ...; local v = main[name]; if v == nil then return nil end; if type(v) == "boolean" then return v and "true" or "false" end; return tostring(v)"#)?;
        Ok(f.call(name)?)
    }

    /// Set one of PoB's application options. Booleans, numbers and strings,
    /// typed from the text as PoB's own settings reader does; then the
    /// build is rebuilt so formatting follows.
    pub fn set_main_option(&self, name: &str, value: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local name, text = ...
                local v
                if text == "true" then v = true
                elseif text == "false" then v = false
                elseif tonumber(text) then v = tonumber(text)
                else v = text end
                main[name] = v
                local b = main.modes["BUILD"]
                if b then b.buildFlag = true end
                runCallback("OnFrame")
                if main.SaveSettings then pcall(main.SaveSettings, main) end
                "#,
            )?;
        f.call::<()>((name, value))?;
        Self::check_prompt(&self.lua)
    }

    /// The Skills tab's per-build gem defaults, as (name, value) text.
    pub fn skills_options(&self) -> Result<Vec<(String, String)>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local t = b.skillsTab
                local out = {}
                for _, k in ipairs({ "defaultGemLevel", "defaultGemQuality", "sortGemsByDPS", "sortGemsByDPSField", "showSupportGemTypes", "showLegacyGems" }) do
                    local v = t and t[k]
                    if type(v) == "boolean" then v = v and "true" or "false" end
                    out[#out + 1] = { k, v == nil and "" or tostring(v) }
                end
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            out.push((row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    pub fn set_skills_option(&self, name: &str, value: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local name, text = ...
                local b = __mainObject__.main.modes["BUILD"]
                local t = b.skillsTab
                if not t then return end
                local v
                if text == "true" then v = true
                elseif text == "false" then v = false
                elseif name == "defaultGemQuality" then v = tonumber(text) or 0
                else v = text end
                t[name] = v
                t.modFlag = true
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((name, value))?;
        Self::check_prompt(&self.lua)
    }

    /// Unallocate every node in the active spec, as PoB's Reset Tree does.
    pub fn reset_tree(&self) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return end
                b.spec:ResetNodes()
                b.spec:AddUndoState()
                b.spec:BuildAllDependsAndPaths()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(())?;
        Self::check_prompt(&self.lua)
    }

    pub fn export_tree_link(&self) -> Result<String, Error> {
        let link: String = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return "" end
                local v = treeVersions[b.spec.treeVersion]
                return b.spec:EncodeURL(v and v.url or "")
                "#,
            )?.call(())?;
        Ok(link)
    }

    /// Replace the current tree from a passive-skill-tree link.
    ///
    /// Returns PoB's own error text when the link is unusable, rather than a
    /// message of our own: upstream distinguishes "unrecognised format" from
    /// a version mismatch, and paraphrasing loses that.
    pub fn import_tree_link(&self, link: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local link = ...
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return "No build loaded" end
                local err = b.spec:DecodeURL(link)
                if err then return err end
                b.spec:AddUndoState()
                b.spec:BuildAllDependsAndPaths()
                b.buildFlag = true
                runCallback("OnFrame")
                return nil
                "#,
            )?;
        if let Some(err) = f.call::<Option<String>>(link)? {
            return Err(Error::PobStartup(err));
        }
        Self::check_prompt(&self.lua)
    }

    /// Every tree version PoB supports: `(id, display name)`, oldest first.
    pub fn tree_versions(&self) -> Result<Vec<(String, String)>, Error> {
        let list: Table = self.chunk(
                r#"
                local out = {}
                for _, id in ipairs(treeVersionList or {}) do
                    local v = treeVersions[id]
                    out[#out + 1] = { id, v and v.display or id }
                end
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let entry = entry?;
            let id: String = entry.get(1)?;
            // Only offer versions whose tree data is actually present. PoB2's
            // list names every EA patch, but a build ships a trimmed set (the
            // iOS app bundles just the latest), and offering a version with no
            // data on disk would let the user pick one that renders blank. The
            // engine runs with its working directory at `src`, so this is the
            // path the tree loader itself would read.
            if std::path::Path::new("TreeData").join(&id).join("tree.lua").is_file() {
                out.push((id, entry.get(2)?));
            }
        }
        Ok(out)
    }

    /// Convert the tree to another version.
    ///
    /// Lossy by nature: nodes that no longer exist are dropped, which is why
    /// PoB warns before doing it. The caller is expected to have asked.
    pub fn convert_tree_version(&self, version: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local version = ...
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return "No build loaded" end
                if not treeVersions[version] then
                    return "Unknown tree version: " .. tostring(version)
                end
                local ok, err = pcall(function()
                    b.treeTab:ConvertToVersion(version, false)
                end)
                if not ok then return tostring(err) end
                b.buildFlag = true
                runCallback("OnFrame")
                return nil
                "#,
            )?;
        if let Some(err) = f.call::<Option<String>>(version)? {
            return Err(Error::PobStartup(err));
        }
        Self::check_prompt(&self.lua)
    }

    /// Turn tree compare mode on against a saved spec, or off.
    ///
    /// This is the whole of PoB's tree comparison: set a second spec, and the
    /// tree read reports per-node and per-connector differences which the
    /// renderer draws as colour. Nothing else changes — not the stats, not
    /// the point counts, not the active spec.
    pub fn set_compare_spec(&self, index: Option<usize>) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab then return end
                if index then
                    tab.isComparing = true
                    tab.activeCompareSpec =
                        math.min(index, #tab.specList)
                else
                    tab.isComparing = false
                end
                "#,
            )?;
        f.call::<()>(index)?;
        Ok(())
    }

    /// PoB's build warnings: over-reserved mana, unmet requirements, too many
    /// passives allocated, and the rest.
    ///
    /// These are the engine's own findings about the build, not our error
    /// messages. PoB puts a warning count under its sidebar and a PoB user
    /// relies on it to notice a build is broken, so not reading them means
    /// silently dropping the thing that catches mistakes.
    pub fn build_warnings(&self) -> Result<Vec<String>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local out = {}
                local lines = b.controls and b.controls.warnings
                    and b.controls.warnings.lines
                for _, line in pairs(lines or {}) do
                    out[#out + 1] = tostring(line)
                end
                return out
                "#,
            )?.call(())?;

        Ok(list
            .sequence_values::<String>()
            .map(|line| line.map(|l| strip_pob_colours(&l)))
            .collect::<Result<_, _>>()?)
    }

    /// The sidebar stat list, exactly as PoB builds it.
    ///
    /// Same `displayStats` table the Compare tab uses, but for one build:
    /// PoB's own comment calls it "the stats in the side bar, and also which
    /// stats show in node/item comparisons". Walking it rather than keeping a
    /// list of our own is what makes the panel show the ~40 stats a PoB user
    /// expects — effective hit pool, per-element max hits, resistances,
    /// required attributes — instead of a hand-picked dozen.
    ///
    /// Values arrive pre-formatted because the format belongs to PoB: each
    /// entry carries its own spec, and its own rules for percentages,
    /// multipliers and over-cap suffixes.
    pub fn sidebar_stats(&self) -> Result<Vec<tree::SidebarRow>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local out = {}
                local o = b.calcsTab and b.calcsTab.mainOutput or {}
                local flags = b.calcsTab and b.calcsTab.mainEnv
                    and b.calcsTab.mainEnv.player
                    and b.calcsTab.mainEnv.player.mainSkill
                    and b.calcsTab.mainEnv.player.mainSkill.skillFlags or {}

                local function matches(req, notReq)
                    if type(req) == "string" then req = { req } end
                    if req then
                        for _, fl in ipairs(req) do
                            if not flags[fl] then return false end
                        end
                    end
                    if type(notReq) == "string" then notReq = { notReq } end
                    if notReq then
                        for _, fl in ipairs(notReq) do
                            if flags[fl] then return false end
                        end
                    end
                    return true
                end

                for _, sd in ipairs(b.displayStats or {}) do
                    if not sd.stat and not sd.label then
                        out[#out + 1] = { kind = "gap" }
                    elseif sd.stat and not sd.hideStat
                        and sd.stat ~= "SkillDPS"
                        and matches(sd.flag, sd.notFlag) then
                        local v = o[sd.stat]
                        if sd.childStat then
                            v = type(v) == "table" and v[sd.childStat] or nil
                        end
                        if type(v) == "table" then v = nil end
                        -- PoB's own visibility rule for the sidebar: a stat
                        -- shows when it has a value and its condition holds.
                        -- Some stats (resistances) force themselves visible
                        -- with `condFunc = function() return true end`.
                        if v and (v ~= 0 or (sd.condFunc and sd.condFunc(v, o)))
                            and (not sd.condFunc or sd.condFunc(v, o)) then
                            local ok, text = pcall(function()
                                return b:FormatStat(sd, v)
                            end)
                            if ok and text then
                                out[#out + 1] = {
                                    kind = "stat",
                                    label = tostring(sd.label or sd.stat),
                                    key = tostring(sd.stat),
                                    value = tostring(text),
                                    colour = tostring(sd.color or ""),
                                }
                            end
                        end
                    end
                end
                return out
                "#,
            )?.call(())?;

        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let e = entry?;
            let kind: String = e.get("kind")?;
            if kind == "gap" {
                out.push(tree::SidebarRow::Gap);
                continue;
            }
            out.push(tree::SidebarRow::Stat(tree::SidebarStat {
                label: e.get("label")?,
                key: e.get("key")?,
                value: strip_pob_colours(&e.get::<String>("value")?),
                colour: parse_pob_colour(&e.get::<String>("colour")?),
            }));
        }
        Ok(out)
    }

    /// Every stat key PoB's `displayStats` table names.
    ///
    /// A comparison must read all of them: any key the compared build does
    /// not supply falls back to the current build's value and silently shows
    /// no difference, which looks like "these are the same" rather than
    /// "this was never measured".
    pub fn display_stat_keys(&self) -> Result<Vec<String>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local seen, out = {}, {}
                for _, sd in ipairs(b.displayStats or {}) do
                    if sd.stat and not seen[sd.stat] then
                        seen[sd.stat] = true
                        out[#out + 1] = sd.stat
                    end
                end
                return out
                "#,
            )?.call(())?;
        Ok(list
            .sequence_values::<String>()
            .collect::<Result<_, _>>()?)
    }

    /// A side-by-side comparison of two builds' stats, as PoB's Compare tab
    /// renders it: label, both absolute values, and the difference.
    ///
    /// Driven by PoB's own `displayStats` table — 204 entries carrying the
    /// label, number format, colour, section breaks and lower-is-better flag
    /// — rather than a list of our own. That table is what PoB's sidebar and
    /// every comparison already use, so borrowing it means our rows match
    /// upstream's and keep matching when a league adds a stat.
    ///
    /// `compare` is the output of the build being compared against, obtained
    /// from [`Engine::stats_for_spec`] or [`Engine::stats_of`].
    pub fn compare_rows(
        &self,
        compare: &std::collections::HashMap<String, f64>,
    ) -> Result<Vec<tree::CompareRow>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local given = ...
                local b = __mainObject__.main.modes["BUILD"]

                -- condFunc reads *other* stats off the output it is handed
                -- (e.g. "show Strength Required when it exceeds o.Str"), so a
                -- sparse map of the keys we happen to care about makes it
                -- compare nil with a number and throw. Fall back to the
                -- primary output for anything the compared side did not
                -- supply, which keeps those conditions answerable.
                local primaryOut = b.calcsTab and b.calcsTab.mainOutput or {}
                local other = setmetatable({}, {
                    __index = function(_, k) return primaryOut[k] end,
                })
                for k, v in pairs(given) do other[k] = v end
                local out = {}
                local primary = primaryOut
                local flags = b.calcsTab and b.calcsTab.mainEnv
                    and b.calcsTab.mainEnv.player
                    and b.calcsTab.mainEnv.player.mainSkill
                    and b.calcsTab.mainEnv.player.mainSkill.skillFlags or {}

                local function matches(req, notReq)
                    if type(req) == "string" then req = { req } end
                    if req then
                        for _, fl in ipairs(req) do
                            if not flags[fl] then return false end
                        end
                    end
                    if type(notReq) == "string" then notReq = { notReq } end
                    if notReq then
                        for _, fl in ipairs(notReq) do
                            if flags[fl] then return false end
                        end
                    end
                    return true
                end

                for _, sd in ipairs(b.displayStats or {}) do
                    if not sd.stat and not sd.label then
                        -- A blank entry is a section break. PoB's Compare tab
                        -- honours these (its tooltips do not), which is what
                        -- keeps offence, defence and resistances visually
                        -- grouped rather than running together.
                        out[#out + 1] = { kind = "gap" }
                    elseif sd.stat and sd.stat ~= "SkillDPS" and not sd.hideStat
                        and matches(sd.flag, sd.notFlag) then
                        local a = primary[sd.stat]
                        local c = other[sd.stat]
                        if sd.childStat then
                            a = type(a) == "table" and a[sd.childStat] or nil
                            c = type(c) == "table" and c[sd.childStat] or nil
                        end
                        if type(a) == "table" then a = nil end
                        if type(c) == "table" then c = nil end
                        a, c = a or 0, c or 0

                        -- Shown when *either* side has a value: unlike a
                        -- tooltip diff, an unchanged stat is still worth a
                        -- row in a side-by-side table.
                        local show = (a ~= 0 or c ~= 0)
                            and (not sd.condFunc
                                 or sd.condFunc(a, primary)
                                 or sd.condFunc(c, other))
                        if show then
                            local fmt = sd.fmt or "d"
                            local mul = (sd.pc or sd.mod) and 100 or 1
                            -- Thousands separators as PoB's own tables draw
                            -- them. An infinite side (a max hit with no
                            -- damage taken) compares as unchanged rather
                            -- than as "nan".
                            local function finite(x) return x == x and x ~= math.huge and x ~= -math.huge end
                            local d = c - a
                            if not finite(d) then d = 0 end
                            local same = d <= 0.001 and d >= -0.001
                            out[#out + 1] = {
                                kind = "stat",
                                label = tostring(sd.label or sd.stat),
                                key = tostring(sd.stat),
                                a = formatNumSep(string.format("%" .. fmt, a * mul)),
                                c = formatNumSep(string.format("%" .. fmt, c * mul)),
                                diff = same and "" or formatNumSep(string.format("%+" .. fmt, d * mul)),
                                pc = (function()
                                    if same or a == 0 or not finite(a) or not finite(c) then return 0 end
                                    return c / a * 100 - 100
                                end)(),
                                better = (function()
                                    if same then return 0 end
                                    local good = (sd.lowerIsBetter and d < 0)
                                        or (not sd.lowerIsBetter and d > 0)
                                    return good and 1 or -1
                                end)(),
                                colour = tostring(sd.color or ""),
                            }
                        end
                    end
                end
                return out
                "#,
            )?;

        let list: Table = f.call(compare.clone())?;
        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let e = entry?;
            let kind: String = e.get("kind")?;
            if kind == "gap" {
                out.push(tree::CompareRow::Gap);
                continue;
            }
            out.push(tree::CompareRow::Stat(tree::CompareStat {
                label: e.get("label")?,
                key: e.get("key")?,
                a: e.get("a")?,
                c: e.get("c")?,
                diff: e.get::<Option<String>>("diff")?.filter(|d| !d.is_empty()),
                percent: e.get::<f64>("pc")?,
                better: e.get::<i8>("better")?,
                colour: parse_pob_colour(&e.get::<String>("colour")?),
            }));
        }
        Ok(out)
    }

    /// Evaluate a Lua snippet against the live engine and return a string.
    ///
    /// For diagnostics only — a way to ask the running PoB what it thinks
    /// without adding a typed accessor for a question asked once.
    #[doc(hidden)]
    pub fn probe(&self, source: &str) -> Result<String, Error> {
        Ok(self.lua.load(source).eval()?)
    }

    /// Change a gem's level, quality, enabled state or count.
    ///
    /// `group` and `gem` are 1-based, matching PoB's own indexing. Only the
    /// fields given are changed; `None` leaves a field alone, so a caller
    /// nudging quality cannot accidentally reset the level.
    ///
    /// Every edit ends in `ProcessSocketGroup`, which is what re-resolves the
    /// gem against its data — supports, tags, and whether the level is even
    /// legal for that gem. Setting the field alone would leave the build
    /// showing a number that the calculation never saw.
    pub fn set_gem(
        &self,
        group: usize,
        gem: usize,
        level: Option<i64>,
        quality: Option<i64>,
        enabled: Option<bool>,
        count: Option<i64>,
    ) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, gj, level, quality, enabled, count = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                if not tab then return end
                local group = tab.socketGroupList[gi]
                if not group then return end
                local gem = group.gemList and group.gemList[gj]
                if not gem then return end

                if level then
                    -- PoB clamps to what the gem actually supports. A level
                    -- 40 gem is not a stronger gem, it is a broken build.
                    gem.level = math.max(1, math.min(level, 40))
                end
                if quality then
                    -- Base quality: 0..23 for normal/awakened; higher only
                    -- comes from item/socket bonuses, applied by the engine,
                    -- not typed. Cap the typed base at 23.
                    gem.quality = math.max(0, math.min(quality, 23))
                end
                if enabled ~= nil then gem.enabled = enabled end
                if count then gem.count = math.max(1, count) end

                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((group, gem, level, quality, enabled, count))?;
        Self::check_prompt(&self.lua)
    }

    /// Enable or disable a whole socket group.
    pub fn set_group_enabled(&self, group: usize, enabled: bool) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, enabled = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group then return end
                group.enabled = enabled
                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((group, enabled))?;
        Self::check_prompt(&self.lua)
    }

    /// Make a socket group the main skill. 1-based, as PoB's own index is.
    pub fn set_main_group(&self, group: usize) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                if not (tab and tab.socketGroupList[gi]) then return end
                b.mainSocketGroup = gi
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(group)?;
        Self::check_prompt(&self.lua)
    }

    /// Count a socket group in the Full DPS total, or stop counting it.
    pub fn set_group_full_dps(&self, group: usize, include: bool) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, include = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group then return end
                group.includeInFullDPS = include
                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((group, include))?;
        Self::check_prompt(&self.lua)
    }

    /// The main-skill controls, filled the way PoB's sidebar fills them.
    pub fn main_skill_controls(&self) -> Result<skills::MainSkillControls, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local out = { active = {}, activeIndex = 1, parts = {}, partIndex = 1,
                              minions = {}, minionValues = {}, minionIndex = 1, byItemSet = false,
                              minionSkills = {}, minionSkillIndex = 1 }
                local group = tab and tab.socketGroupList[b.mainSocketGroup]
                if not group then return out end
                local list = group.displaySkillList or {}
                local sel = group.mainActiveSkill or 1
                for i, activeSkill in ipairs(list) do
                    local src = activeSkill.activeEffect.srcInstance
                    local ex = src.explodeSource
                    local exName = ex and (ex.name or ex.dn)
                    out.active[i] = exName and ("From " .. exName) or activeSkill.activeEffect.grantedEffect.name
                end
                out.activeIndex = sel
                local activeSkill = list[sel]
                if not activeSkill then return out end
                local effect = activeSkill.activeEffect
                if not effect then return out end
                local ge = effect.grantedEffect
                local src = effect.srcInstance
                if ge.parts and #ge.parts > 1 then
                    for i, part in ipairs(ge.parts) do out.parts[i] = part.name end
                    out.partIndex = src.skillPart or 1
                    if ge.parts[out.partIndex] and ge.parts[out.partIndex].stages then
                        out.stages = src.skillStageCount or activeSkill.skillData.stagesMax
                            or ge.parts[out.partIndex].stagesMin or 1
                    end
                end
                if activeSkill.skillFlags.mine then
                    out.mines = src.skillMineCount or 0
                end
                if activeSkill.skillFlags.multiStage and not (ge.parts and #ge.parts > 1) then
                    out.stages = src.skillStageCount or activeSkill.skillData.stagesMax
                        or activeSkill.skillData.stagesMin or 1
                end
                if not activeSkill.skillFlags.disable and (ge.minionList or activeSkill.minionList[1]) then
                    if ge.minionHasItemSet then
                        out.byItemSet = true
                        local want = src.skillMinionItemSet or 1
                        for i, itemSetId in ipairs(b.itemsTab.itemSetOrderList) do
                            local itemSet = b.itemsTab.itemSets[itemSetId]
                            out.minions[i] = itemSet.title or "Default Item Set"
                            out.minionValues[i] = tostring(itemSetId)
                            if itemSetId == want then out.minionIndex = i end
                        end
                    else
                        local want = src.skillMinion
                        for i, minionId in ipairs(activeSkill.minionList) do
                            out.minions[i] = b.data.minions[minionId].name
                            out.minionValues[i] = minionId
                            if minionId == want then out.minionIndex = i end
                        end
                    end
                    if activeSkill.minion then
                        for i, ms in ipairs(activeSkill.minion.activeSkillList) do
                            out.minionSkills[i] = ms.activeEffect.grantedEffect.name
                        end
                        out.minionSkillIndex = src.skillMinionSkill or 1
                    end
                end
                return out
                "#,
            )?;
        let t: mlua::Table = f.call(())?;
        let strings = |key: &str| -> Result<Vec<String>, Error> {
            let inner: mlua::Table = t.get(key)?;
            Ok(inner.sequence_values::<String>().collect::<Result<_, _>>()?)
        };
        let idx = |key: &str| -> Result<usize, Error> {
            Ok(t.get::<Option<i64>>(key)?.unwrap_or(1).max(1) as usize)
        };
        Ok(skills::MainSkillControls {
            active_skills: strings("active")?,
            active_index: idx("activeIndex")?,
            parts: strings("parts")?,
            part_index: idx("partIndex")?,
            stages: t.get("stages")?,
            mines: t.get("mines")?,
            minions: strings("minions")?,
            minion_values: strings("minionValues")?,
            minion_index: idx("minionIndex")?,
            minion_by_item_set: t.get::<Option<bool>>("byItemSet")?.unwrap_or(false),
            minion_skills: strings("minionSkills")?,
            minion_skill_index: idx("minionSkillIndex")?,
        })
    }

    /// Write one field of the main skill's source instance, as PoB's
    /// sidebar controls do, then recalculate.
    fn set_main_skill_field(&self, code: &str, value: mlua::Value) -> Result<(), Error> {
        let src = format!(
            r#"
            local v = ...
            local b = __mainObject__.main.modes["BUILD"]
            local tab = b.skillsTab
            local group = tab and tab.socketGroupList[b.mainSocketGroup]
            if not group then return end
            local skill = group.displaySkillList and group.displaySkillList[group.mainActiveSkill or 1]
            local src = skill and skill.activeEffect and skill.activeEffect.srcInstance
            {code}
            b.modFlag = true
            b.buildFlag = true
            runCallback("OnFrame")
            "#
        );
        let f: mlua::Function = self.lua.load(&src).into_function()?;
        f.call::<()>(value)?;
        Self::check_prompt(&self.lua)
    }

    pub fn set_main_active_skill(&self, index: usize) -> Result<(), Error> {
        self.set_main_skill_field("group.mainActiveSkill = v", mlua::Value::Integer(index as i64))
    }

    pub fn set_main_skill_part(&self, index: usize) -> Result<(), Error> {
        self.set_main_skill_field("if src then src.skillPart = v end", mlua::Value::Integer(index as i64))
    }

    pub fn set_main_skill_stages(&self, count: i64) -> Result<(), Error> {
        self.set_main_skill_field("if src then src.skillStageCount = v end", mlua::Value::Integer(count))
    }

    pub fn set_main_skill_mines(&self, count: i64) -> Result<(), Error> {
        self.set_main_skill_field("if src then src.skillMineCount = v end", mlua::Value::Integer(count))
    }

    /// `value` is one of `MainSkillControls::minion_values`.
    pub fn set_main_skill_minion(&self, value: &str, by_item_set: bool) -> Result<(), Error> {
        let code = if by_item_set {
            "if src then src.skillMinionItemSet = tonumber(v) end"
        } else {
            "if src then src.skillMinion = v end"
        };
        let v = self.lua.create_string(value)?;
        self.set_main_skill_field(code, mlua::Value::String(v))
    }

    pub fn set_main_skill_minion_skill(&self, index: usize) -> Result<(), Error> {
        self.set_main_skill_field("if src then src.skillMinionSkill = v end", mlua::Value::Integer(index as i64))
    }

    /// Add an empty socket group, as the Skills tab's New button does.
    /// Returns its 1-based index.
    pub fn new_socket_group(&self) -> Result<usize, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                if not tab then return 0 end
                local group = { label = "", enabled = true, gemList = { } }
                table.insert(tab.socketGroupList, group)
                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return #tab.socketGroupList
                "#,
            )?;
        let i: usize = f.call(())?;
        Self::check_prompt(&self.lua)?;
        Ok(i)
    }

    /// Delete a socket group. Groups granted by an item are refused, as
    /// PoB refuses them. The main-skill index follows, as PoB's list does.
    pub fn delete_socket_group(&self, group: usize) -> Result<bool, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group or group.source then return false end
                table.remove(tab.socketGroupList, gi)
                if b.mainSocketGroup > gi then b.mainSocketGroup = b.mainSocketGroup - 1 end
                if b.mainSocketGroup > #tab.socketGroupList then b.mainSocketGroup = math.max(1, #tab.socketGroupList) end
                local calcs = b.calcsTab
                if calcs and calcs.input and calcs.input.skill_number and calcs.input.skill_number > gi then
                    calcs.input.skill_number = calcs.input.skill_number - 1
                end
                if tab.RebuildImbuedSupportBySlot then tab:RebuildImbuedSupportBySlot() end
                if tab.displayGroup == group then tab.displayGroup = nil end
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;
        let ok: bool = f.call(group)?;
        Self::check_prompt(&self.lua)?;
        Ok(ok)
    }

    /// Socket a group in a slot (PoB's slot name), or none.
    pub fn set_group_slot(&self, group: usize, slot: Option<&str>) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, slot = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group then return end
                group.slot = slot
                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((group, slot))?;
        Self::check_prompt(&self.lua)
    }

    /// Remove a gem from a socket group.
    pub fn remove_gem(&self, group: usize, gem: usize) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, gj = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group or not group.gemList then return end
                if not group.gemList[gj] then return end
                table.remove(group.gemList, gj)
                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>((group, gem))?;
        Self::check_prompt(&self.lua)
    }

    /// What equipping `item_id` in `slot` would do to the build.
    ///
    /// The right half of PoB's item tooltip. Uses the same
    /// `GetMiscCalculator` the tree's node preview uses, with an item
    /// override rather than a node one — `repSlotName`/`repItem` is how PoB
    /// asks "what if this were worn here" without actually wearing it. That
    /// matters beyond tidiness: the calculator handles the two-handed case,
    /// where trying a staff in Weapon 1 has to empty Weapon 2 to be honest
    /// about the result.
    ///
    /// Hovering the item already in the slot returns its removal deltas with
    /// `is_equipped` set, because that is the only sensible reading of "what
    /// would this change" for something already worn.
    pub fn item_preview(
        &self,
        slot: &str,
        item_id: i64,
    ) -> Result<Option<items::ItemPreview>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local slotName, itemId = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab, calcs = b and b.itemsTab, b and b.calcsTab
                if not tab or not calcs then return nil end
                local slot = tab.slots[slotName]
                local item = tab.items[itemId]
                if not slot or not item then return nil end

                local calcFunc, calcBase = calcs:GetMiscCalculator()
                if not calcFunc then return nil end

                local worn = slot.selItemId ~= 0 and tab.items[slot.selItemId]
                local isEquipped = worn and worn.id == item.id

                -- Removing what is worn means asking for the slot empty;
                -- trying something else means asking for it in place.
                local out
                if isEquipped then
                    out = calcFunc({ repSlotName = slotName, repItem = nil })
                else
                    out = calcFunc({ repSlotName = slotName, repItem = item })
                end
                if not out then return nil end

                -- PoB's own display stat list, so labels, number formats and
                -- which direction counts as better are all upstream's.
                local rows = {}
                for _, sd in ipairs(b.displayStats or {}) do
                    if sd.stat and not sd.childStat and sd.stat ~= "SkillDPS" then
                        local a = out[sd.stat] or 0
                        local base = calcBase[sd.stat] or 0
                        local diff = a - base
                        if diff > 0.001 or diff < -0.001 then
                            local mul = (sd.pc or sd.mod) and 100 or 1
                            local val = string.format("%+" .. sd.fmt, diff * mul)
                            local pc = nil
                            if sd.compPercent and a ~= 0 and base ~= 0 then
                                pc = a / base * 100 - 100
                            end
                            local better = sd.lowerIsBetter and diff < 0
                                or (not sd.lowerIsBetter and diff > 0)
                            rows[#rows + 1] = {
                                tostring(sd.label or sd.stat),
                                val,
                                pc or false,
                                better and 1 or 0,
                            }
                        end
                    end
                end

                return {
                    slot = slot.label or slotName,
                    replacing = (worn and not isEquipped) and worn.name or false,
                    equipped = isEquipped and 1 or 0,
                    rows = rows,
                }
                "#,
            )?;

        let Some(raw): Option<Table> = f.call((slot, item_id))? else {
            return Ok(None);
        };

        let mut deltas = Vec::new();
        let rows: Table = raw.get("rows")?;
        for entry in rows.sequence_values::<Table>() {
            let e = entry?;
            // Absent values arrive as `false`, not nil: a nil inside a Lua
            // table constructor truncates the sequence and takes every field
            // after it. Decode loosely and read a boolean as "not present".
            let percent = match e.get::<mlua::Value>(3)? {
                mlua::Value::Number(n) => Some(n),
                mlua::Value::Integer(i) => Some(i as f64),
                _ => None,
            };
            deltas.push(tree::StatDelta {
                label: e.get::<String>(1)?,
                value: e.get::<String>(2)?,
                percent,
                per_point: None,
                per_point_percent: None,
                is_improvement: e.get::<i64>(4)? == 1,
            });
        }

        let replacing = match raw.get::<mlua::Value>("replacing")? {
            mlua::Value::String(s) => Some(s.to_str()?.to_owned()),
            _ => None,
        };

        Ok(Some(items::ItemPreview {
            slot: raw.get::<String>("slot")?,
            replacing,
            is_equipped: raw.get::<i64>("equipped")? == 1,
            deltas,
        }))
    }

    /// What equipping an item given as text in `slot` would do: the same
    /// calculation as `item_preview`, for items the build does not own
    /// (another build's, a database unique).
    pub fn item_preview_text(
        &self,
        slot: &str,
        text: &str,
    ) -> Result<Option<items::ItemPreview>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local slotName, itemText = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab, calcs = b and b.itemsTab, b and b.calcsTab
                if not tab or not calcs then return nil end
                local slot = tab.slots[slotName]
                local item = new("Item"):Item(itemText)
                if not slot or not item or not item.base then return nil end
                item:NormaliseQuality()
                item:BuildModList()

                local calcFunc, calcBase = calcs:GetMiscCalculator()
                if not calcFunc then return nil end

                local worn = slot.selItemId ~= 0 and tab.items[slot.selItemId]
                local isEquipped = false

                -- Removing what is worn means asking for the slot empty;
                -- trying something else means asking for it in place.
                local out
                if isEquipped then
                    out = calcFunc({ repSlotName = slotName, repItem = nil })
                else
                    out = calcFunc({ repSlotName = slotName, repItem = item })
                end
                if not out then return nil end

                -- PoB's own display stat list, so labels, number formats and
                -- which direction counts as better are all upstream's.
                local rows = {}
                for _, sd in ipairs(b.displayStats or {}) do
                    if sd.stat and not sd.childStat and sd.stat ~= "SkillDPS" then
                        local a = out[sd.stat] or 0
                        local base = calcBase[sd.stat] or 0
                        local diff = a - base
                        if diff > 0.001 or diff < -0.001 then
                            local mul = (sd.pc or sd.mod) and 100 or 1
                            local val = string.format("%+" .. sd.fmt, diff * mul)
                            local pc = nil
                            if sd.compPercent and a ~= 0 and base ~= 0 then
                                pc = a / base * 100 - 100
                            end
                            local better = sd.lowerIsBetter and diff < 0
                                or (not sd.lowerIsBetter and diff > 0)
                            rows[#rows + 1] = {
                                tostring(sd.label or sd.stat),
                                val,
                                pc or false,
                                better and 1 or 0,
                            }
                        end
                    end
                end

                return {
                    slot = slot.label or slotName,
                    replacing = (worn and not isEquipped) and worn.name or false,
                    equipped = isEquipped and 1 or 0,
                    rows = rows,
                }
                "#,
            )?;

        let Some(raw): Option<Table> = f.call((slot, text))? else {
            return Ok(None);
        };

        let mut deltas = Vec::new();
        let rows: Table = raw.get("rows")?;
        for entry in rows.sequence_values::<Table>() {
            let e = entry?;
            // Absent values arrive as `false`, not nil: a nil inside a Lua
            // table constructor truncates the sequence and takes every field
            // after it. Decode loosely and read a boolean as "not present".
            let percent = match e.get::<mlua::Value>(3)? {
                mlua::Value::Number(n) => Some(n),
                mlua::Value::Integer(i) => Some(i as f64),
                _ => None,
            };
            deltas.push(tree::StatDelta {
                label: e.get::<String>(1)?,
                value: e.get::<String>(2)?,
                percent,
                per_point: None,
                per_point_percent: None,
                is_improvement: e.get::<i64>(4)? == 1,
            });
        }

        let replacing = match raw.get::<mlua::Value>("replacing")? {
            mlua::Value::String(s) => Some(s.to_str()?.to_owned()),
            _ => None,
        };

        Ok(Some(items::ItemPreview {
            slot: raw.get::<String>("slot")?,
            replacing,
            is_equipped: raw.get::<i64>("equipped")? == 1,
            deltas,
        }))
    }

    /// The items this build owns, in PoB's own order.
    ///
    /// This is what the slot dropdowns choose from and what the item list
    /// panel shows. `equipped_in` is filled from the active item set, since
    /// that is what "unused" means to Del Unused.
    pub fn item_list(&self) -> Result<Vec<items::ItemEntry>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                if not tab then return {} end

                -- Which slots each item currently occupies. Read from the
                -- live slots rather than the item, because an item carries
                -- no back-reference to where it is worn.
                local wornIn = {}
                for slotName, slot in pairs(tab.slots or {}) do
                    local id = slot.selItemId
                    if id and id ~= 0 then
                        wornIn[id] = wornIn[id] or {}
                        table.insert(wornIn[id], slotName)
                    end
                end

                local out = {}
                for _, id in ipairs(tab.itemOrderList or {}) do
                    local item = tab.items[id]
                    if item then
                        local slotsFor = wornIn[id] or {}
                        table.sort(slotsFor)
                        table.insert(out, {
                            id = id,
                            -- A timeless jewel is its seed: PoB's finder
                            -- names the jewels it makes with one, and an
                            -- imported one is just "Elegant Hubris", so a
                            -- list of them is a guessing game without it.
                            name = (function()
                                local n = item.name or ""
                                local seed = item.jewelData and item.jewelData.conqueredBy and item.jewelData.conqueredBy.id
                                if seed and not n:find(tostring(seed), 1, true) then
                                    n = n .. " " .. tostring(seed)
                                end
                                return n
                            end)(),
                            rarity = item.rarity or "NORMAL",
                            equipped = slotsFor,
                        })
                    end
                end
                return out
                "#,
            )?;

        let list: Table = f.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            let worn: Table = row.get("equipped")?;
            let mut equipped_in = Vec::new();
            for slot in worn.sequence_values::<String>() {
                equipped_in.push(slot?);
            }
            out.push(items::ItemEntry {
                id: row.get::<i64>("id")?,
                name: row.get::<String>("name")?,
                rarity: items::Rarity::parse(&row.get::<String>("rarity")?),
                equipped_in,
            });
        }
        Ok(out)
    }

    /// The items that may go in a slot, as PoB's own dropdown lists them.
    ///
    /// Validity is `IsItemValidForSlot`, called rather than reimplemented: it
    /// covers jewel socket sizes, cluster jewels, charms, tinctures and the
    /// weapon-type rules, and a copy of it here would drift from upstream
    /// the first time GGG adds a base type.
    pub fn slot_candidates(&self, slot: &str) -> Result<Vec<items::ItemEntry>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local slotName = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                if not tab or not tab.slots[slotName] then return {} end

                local out = {}
                for _, id in ipairs(tab.itemOrderList or {}) do
                    local item = tab.items[id]
                    if item and tab:IsItemValidForSlot(item, slotName) then
                        table.insert(out, {
                            id = id,
                            name = item.name or "",
                            rarity = item.rarity or "NORMAL",
                        })
                    end
                end
                return out
                "#,
            )?;

        let list: Table = f.call(slot)?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            out.push(items::ItemEntry {
                id: row.get::<i64>("id")?,
                name: row.get::<String>("name")?,
                rarity: items::Rarity::parse(&row.get::<String>("rarity")?),
                equipped_in: Vec::new(),
            });
        }
        Ok(out)
    }

    /// Put an item in a slot, or empty it with `item_id` of 0.
    ///
    /// Goes through the slot control's own `SetSelItemId` rather than
    /// assigning the field: jewel slots re-socket into the spec and rebuild
    /// cluster jewel graphs from there, so a plain assignment would leave a
    /// build that looks right and calculates wrong.
    pub fn equip_item(&self, slot: &str, item_id: i64) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local slotName, itemId = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                local slot = tab and tab.slots[slotName]
                if not slot then return false end
                -- 0 clears the slot; anything else must be an item we hold.
                if itemId ~= 0 and not tab.items[itemId] then return false end

                slot:SetSelItemId(itemId)
                tab:PopulateSlots()
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call((slot, item_id))?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!(
                "could not equip item {item_id} in '{slot}'"
            )));
        }
        Ok(())
    }

    /// Turn a flask on or off. PoB stores this on the item set, not the item.
    pub fn set_flask_active(&self, slot: &str, active: bool) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local slotName, active = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                local slot = tab and tab.slots[slotName]
                if not slot or not tab.activeItemSet[slotName] then return false end
                -- Every slot has an item-set entry, so its presence proves
                -- nothing. PoB puts the activate checkbox on flask slots
                -- alone, and that control is what makes a slot toggleable.
                if not slot.controls or not slot.controls.activate then return false end

                slot.active = active
                tab.activeItemSet[slotName].active = active
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call((slot, active))?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!("'{slot}' is not a flask slot")));
        }
        Ok(())
    }

    /// Delete an item from the build entirely, unequipping it first.
    /// Add an item from its text, as pasted from the game or a trade site.
    ///
    /// PoB's own parser (`Item:Item(raw)`) does the reading; an item it
    /// cannot place on a base is rejected rather than added blank. Not
    /// auto-equipped: the user chooses the slot. Returns the new item's id.
    pub fn add_item_from_text(&self, text: &str) -> Result<Option<i64>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local raw = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                if not tab then return nil end
                local item = new("Item"):Item(raw)
                if not item.base then return nil end
                item:NormaliseQuality()
                item:BuildModList()
                tab:AddItem(item, true)
                tab:PopulateSlots()
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return item.id
                "#,
            )?;
        let id: Option<i64> = f.call(text)?;
        Self::check_prompt(&self.lua)?;
        Ok(id)
    }

    /// Whether the active item set is on its second weapon set.
    pub fn weapon_set_second(&self) -> Result<bool, Error> {
        let v: Option<bool> = self.chunk(r#"local b = __mainObject__.main.modes["BUILD"]; local t = b.itemsTab; return t and t.activeItemSet and t.activeItemSet.useSecondWeaponSet or false"#)?.call(())?;
        Ok(v.unwrap_or(false))
    }

    /// Switch weapon sets, as PoB's I / II buttons do: the main socket group
    /// follows to a group socketed in the other set when it has to.
    pub fn set_weapon_set(&self, second: bool) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local second = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                if not (tab and tab.activeItemSet) then return end
                if (tab.activeItemSet.useSecondWeaponSet or false) == second then return end
                tab.activeItemSet.useSecondWeaponSet = second
                tab:AddUndoState()
                b.buildFlag = true
                local from, to = second and 1 or 2, second and 2 or 1
                local mainGroup = b.skillsTab.socketGroupList[b.mainSocketGroup]
                if mainGroup and mainGroup.slot and tab.slots[mainGroup.slot] and tab.slots[mainGroup.slot].weaponSet == from then
                    for index, group in ipairs(b.skillsTab.socketGroupList) do
                        if group.slot and tab.slots[group.slot] and tab.slots[group.slot].weaponSet == to then
                            b.mainSocketGroup = index
                            break
                        end
                    end
                end
                tab:PopulateSlots()
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(second)?;
        Self::check_prompt(&self.lua)
    }

    /// PoB's unique database, sorted by name.
    pub fn unique_catalogue(&self) -> Result<Vec<items::DbItem>, Error> {
        let list: Table = self.chunk(
                r#"
                local out = {}
                for name, item in pairs(main.uniqueDB.list) do
                    out[#out + 1] = {
                        item.name or name, item.baseName or "", item.type or "",
                        item.rarity or "UNIQUE", item.league,
                        item.requirements and item.requirements.level or nil
                    }
                end
                table.sort(out, function(a, b) return a[1]:gsub("^The ", "") < b[1]:gsub("^The ", "") end)
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            let rarity: String = row.get(4)?;
            out.push(items::DbItem {
                name: row.get(1)?,
                base_name: row.get(2)?,
                item_type: row.get(3)?,
                rarity: items::Rarity::parse(&rarity),
                league: row.get(5)?,
                req_level: row.get(6)?,
            });
        }
        Ok(out)
    }

    /// A database item's text, for the detail card.
    pub fn unique_raw(&self, name: &str) -> Result<Option<String>, Error> {
        let f: mlua::Function = self.chunk(r#"local name = ...; local item = main.uniqueDB.list[name]; return item and item.raw or nil"#)?;
        Ok(f.call(name)?)
    }

    /// Add a database unique to the build, as PoB's Ctrl-click does;
    /// optionally into its primary slot. Returns the new item's id.
    pub fn add_unique(&self, name: &str, equip: bool) -> Result<Option<i64>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local name, equip = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                local src = main.uniqueDB.list[name]
                if not (tab and src) then return nil end
                local item = new("Item"):Item(src.raw)
                item:NormaliseQuality()
                tab:AddItem(item, true)
                if equip then
                    local slotName = item:GetPrimarySlot()
                    if slotName and tab.slots[slotName] then
                        if tab.slots[slotName].weaponSet == 1 and tab.activeItemSet.useSecondWeaponSet then
                            slotName = slotName .. " Swap"
                        end
                        if tab.slots[slotName] then tab.slots[slotName]:SetSelItemId(item.id) end
                    end
                end
                tab:PopulateSlots()
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return item.id
                "#,
            )?;
        let id: Option<i64> = f.call((name, equip))?;
        Self::check_prompt(&self.lua)?;
        Ok(id)
    }

    /// PoB's craftable bases, by type, in PoB's order.
    pub fn craft_bases(&self) -> Result<Vec<items::CraftBase>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local d = b.data
                local out = {}
                for _, t in ipairs(d.itemBaseTypeList or {}) do
                    for _, base in ipairs(d.itemBaseLists[t] or {}) do
                        out[#out + 1] = { t, base.name or base.label or "" }
                    end
                end
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            out.push(items::CraftBase { item_type: row.get(1)?, name: row.get(2)? });
        }
        Ok(out)
    }

    /// Create an item on a base, as PoB's Craft item popup does. `rarity` is
    /// NORMAL, MAGIC or RARE; `title` names a rare. Returns the item's id.
    pub fn craft_item(&self, item_type: &str, base_name: &str, rarity: &str, title: &str) -> Result<Option<i64>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local itemType, baseName, rarity, title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                if not tab then return nil end
                local baseEntry
                for _, base in ipairs(b.data.itemBaseLists[itemType] or {}) do
                    if (base.name or base.label) == baseName then baseEntry = base break end
                end
                if not baseEntry then return nil end
                local base = baseEntry.base
                local item = new("Item"):Item()
                item.name = baseEntry.name or baseName
                item.base = base
                item.baseName = item.name
                item.buffModLines = { }
                item.enchantModLines = { }
                item.classRequirementModLines = { }
                item.scourgeModLines = { }
                item.implicitModLines = { }
                item.explicitModLines = { }
                item.crucibleModLines = { }
                if base.type == "Amulet" or base.type == "Belt" or base.type == "Jewel" or base.type == "Quiver" or base.type == "Ring" or base.type == "Graft" then
                    item.quality = nil
                else
                    item.quality = 0
                end
                if (base.flask or (base.type == "Jewel" and base.subType == "Charm") or base.type == "Tincture") and rarity == "RARE" then
                    rarity = "MAGIC"
                end
                if rarity == "MAGIC" or rarity == "RARE" then item.crafted = true end
                item.rarity = rarity
                if rarity == "RARE" then
                    item.title = (title and title:match("%S")) and title or "New Item"
                end
                if base.implicit then
                    local i = 1
                    for line in base.implicit:gmatch("[^\n]+") do
                        local modList, extra = modLib.parseMod(line)
                        table.insert(item.implicitModLines, { line = line, extra = extra, modList = modList or { }, modTags = base.implicitModTypes and base.implicitModTypes[i] or { } })
                        i = i + 1
                    end
                end
                item:NormaliseQuality()
                item:BuildAndParseRaw()
                tab:AddItem(item, true)
                tab:PopulateSlots()
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return item.id
                "#,
            )?;
        let id: Option<i64> = f.call((item_type, base_name, rarity, title))?;
        Self::check_prompt(&self.lua)?;
        Ok(id)
    }

    /// Parse item text with PoB's own parser, without adding it to the
    /// build. `None` when PoB cannot place it on a base.
    pub fn parse_item_text(&self, raw: &str) -> Result<Option<items::Item>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local raw = ...
                local item = new("Item"):Item(raw)
                if not item.base then return nil end
                item:NormaliseQuality()
                item:BuildModList()
                return item
                "#,
            )?;
        let t: Option<Table> = f.call(raw)?;
        let Some(t) = t else { return Ok(None) };
        let flavour = self.flavour_lookup()?;
        Ok(Some(self.read_item(&t, &flavour)?))
    }

    /// The slot PoB would put an item given as text into (its primary
    /// slot), or `None` when it fits nowhere.
    pub fn item_primary_slot(&self, text: &str) -> Result<Option<String>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local raw = ...
                local item = new("Item"):Item(raw)
                if not item.base then return nil end
                return item:GetPrimarySlot()
                "#,
            )?;
        Ok(f.call(text)?)
    }

    /// An item's text as PoB writes it, for editing.
    pub fn item_raw(&self, item_id: i64) -> Result<Option<String>, Error> {
        let f: mlua::Function = self.chunk(r#"local id = ...; local b = __mainObject__.main.modes["BUILD"]; local item = b.itemsTab and b.itemsTab.items[id]; return item and item:BuildRaw() or nil"#)?;
        Ok(f.call(item_id)?)
    }

    /// Replace an item with one parsed from text, keeping its id and slots,
    /// which is what PoB's edit-as-text does when saved.
    pub fn replace_item_from_text(&self, item_id: i64, text: &str) -> Result<bool, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local id, raw = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                if not (tab and tab.items[id]) then return false end
                local item = new("Item"):Item(raw)
                if not item.base then return false end
                item.id = id
                item:NormaliseQuality()
                item:BuildModList()
                tab:AddItem(item, true)
                tab:PopulateSlots()
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;
        let ok: bool = f.call((item_id, text))?;
        Self::check_prompt(&self.lua)?;
        Ok(ok)
    }

    pub fn delete_item(&self, item_id: i64) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local itemId = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.itemsTab
                if not tab or not tab.items[itemId] then return false end
                tab:DeleteItem(tab.items[itemId])
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call(item_id)?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!("no item with id {item_id}")));
        }
        Ok(())
    }

    /// Every gem that can be socketed, for the gem picker.
    ///
    /// PoB's own list, filtered the way `PopulateGemList` filters it: gems
    /// flagged `hideFromGemList` are skipped, and so are legacy ones unless
    /// the build asks for them. Ordered by name here rather than by DPS —
    /// PoB's DPS ordering trial-runs the build once per candidate gem, which
    /// is a separate feature and a much more expensive one.
    pub fn gem_catalogue(&self) -> Result<Vec<skills::GemChoice>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local data = b and b.data
                if not data or not data.gems then return {} end

                local out = {}
                for gemId, gemData in pairs(data.gems) do
                    local ge = gemData.grantedEffect
                    -- `hideFromGemList` covers gems that exist in the data
                    -- but are not socketable; showing them offers a choice
                    -- that cannot be made.
                    if ge and not ge.hideFromGemList and not ge.legacy then
                        -- PoB stores colour as 1/2/3 on the granted effect
                        -- (str/dex/int) and paints the dropdown from it.
                        local letter = "W"
                        if ge.color == 1 then letter = "R"
                        elseif ge.color == 2 then letter = "G"
                        elseif ge.color == 3 then letter = "B" end

                        table.insert(out, {
                            id = gemId,
                            name = gemData.name or "",
                            color = letter,
                            support = ge.support and true or false,
                        })
                    end
                end

                table.sort(out, function(x, y) return x.name < y.name end)
                return out
                "#,
            )?;

        let list: mlua::Table = f.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<mlua::Table>() {
            let row = row?;
            out.push(skills::GemChoice {
                id: row.get::<String>("id")?,
                name: row.get::<String>("name")?,
                color: skills::SocketColor::parse(&row.get::<String>("color")?),
                support: row.get::<bool>("support")?,
            });
        }
        Ok(out)
    }

    /// Point a gem slot at a different gem, by PoB's gem id.
    ///
    /// `gem` may be one past the end of the group, which is how a gem is
    /// added: PoB's editor shows a blank trailing row and typing into it
    /// appends. Passing an id for that index appends rather than failing.
    ///
    /// Only `gemId` is written. `ProcessSocketGroup` derives the rest —
    /// display name, skill id, socket colour, which supports apply — from
    /// the id alone, so setting those here would just be stale copies of
    /// what it is about to compute.
    /// A gem's own description text (what the skill or support does), for
    /// the socketed gem the user tapped. `group`/`gem` are 1-based, as
    /// `set_gem` uses them. Empty when the gem has no description.
    pub fn gem_description(&self, group: usize, gem: usize) -> Result<String, Error> {
        self.lua.globals().set("__pobios_desc_g", group)?;
        self.lua.globals().set("__pobios_desc_i", gem)?;
        let text: Option<String> = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local grp = b.skillsTab and b.skillsTab.socketGroupList[__pobios_desc_g]
                local gem = grp and grp.gemList and grp.gemList[__pobios_desc_i]
                local ge = gem and gem.gemData and gem.gemData.grantedEffect
                return ge and ge.description or nil
                "#,
            )?.call(())?;
        self.lua.globals().set("__pobios_desc_g", mlua::Nil)?;
        self.lua.globals().set("__pobios_desc_i", mlua::Nil)?;
        Ok(text.unwrap_or_default())
    }

    pub fn set_gem_id(&self, group: usize, gem: usize, gem_id: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local gi, gj, gemId = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.skillsTab
                if not tab then return false end
                local group = tab.socketGroupList[gi]
                if not group then return false end
                group.gemList = group.gemList or {}

                local gemData = b.data and b.data.gems and b.data.gems[gemId]
                if not gemData then return false end

                local gem = group.gemList[gj]
                if not gem then
                    -- Appending: PoB's defaults for a fresh gem, so a gem
                    -- added here matches one added in PoB rather than
                    -- arriving at level 0 quality 0.
                    gem = { level = 20, quality = 0, enabled = true, count = 1 }
                    group.gemList[gj] = gem
                end

                -- skillId and nameSpec are cleared rather than set: with a
                -- gemId present ProcessSocketGroup resolves from that and
                -- overwrites both. Leaving the old ones would mean a gem
                -- briefly carrying the previous gem's name.
                gem.gemId = gemId
                gem.skillId = nil
                gem.nameSpec = nil
                gem.new = nil

                tab:ProcessSocketGroup(group)
                tab:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call((group, gem, gem_id))?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!("unknown gem id '{gem_id}'")));
        }
        Ok(())
    }

    /// Resolve typed text to a gem id, using PoB's own matcher.
    ///
    /// Worth calling into rather than reimplementing: `FindSkillGem` accepts
    /// abbreviations ("CtF" finds Cold to Fire) and reports ambiguity as an
    /// error rather than silently picking one. `Ok(None)` means no match.
    /// Score every support gem by the DPS it would add to a socket group,
    /// as PoB's gem dropdown does when "Sort gems by DPS" is on. Returns
    /// (gem id, delta) sorted best first; `delta` is the DPS change against
    /// the group as it stands, and only supports that can actually support
    /// the group's active skill are scored (the rest add nothing).
    ///
    /// This is a real calc per candidate, so the caller runs it on demand
    /// for the one group being edited, not the whole catalogue.
    /// Returns (gem id, delta, socketed). For a socketed support `delta`
    /// is its current contribution; for a candidate it is the DPS it would
    /// add if socketed.
    /// A trade-site query for the best upgrade to a slot, weighted by one
    /// stat, built by PoB's own TradeQueryGenerator driven headless.
    ///
    /// `slot` is a PoB slot name ("Body Armour", "Ring 1", "Amulet", ...);
    /// `stat` is a powerStatList key ("TotalDPS", "TotalEHP", "FullDPS").
    /// Returns the query JSON PoB would send to the trade site, or an error
    /// string PoB produced (e.g. "found no mods to search for").
    ///
    /// The generator evaluates each candidate mod through a full calc in a
    /// coroutine that PoB drains over frames; here we pump frames until it
    /// finishes and the callback has the query.
    /// PoB's weighted "Find best" search for one slot. `weights` is the sort
    /// stat list as PoB models it (a `data.powerStatList` key and a multiplier
    /// each, e.g. `[("FullDPS", 1.0), ("TotalEHP", 0.5)]`, which is PoB's own
    /// default). `buyout` is the 0-based index into PoB's trade-status list
    /// (0 = securable / "Instant buyout" .. 4 = any). Returns the query JSON,
    /// or PoB's own message when there is nothing worth searching for.
    pub fn trade_query(
        &self,
        slot: &str,
        weights: &[(String, f64)],
        buyout: u8,
    ) -> Result<Result<String, String>, Error> {
        self.lua.globals().set("__pobios_trade_slot", slot)?;
        self.lua.globals().set("__pobios_trade_buyout", buyout as i64)?;
        // Hand the weight list over as a Lua table, so an arbitrary number of
        // weighted stats round-trips (PoB's default is two: Full DPS + EHP).
        let wt = self.lua.create_table()?;
        for (i, (stat, mult)) in weights.iter().enumerate() {
            let row = self.lua.create_table()?;
            row.set("stat", stat.as_str())?;
            row.set("weightMult", *mult)?;
            wt.set(i + 1, row)?;
        }
        self.lua.globals().set("__pobios_trade_weights", wt)?;
        // Kick off the generator with the weighted stat list and our callback.
        self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.itemsTab
                -- The Trade tab is a UI construct the headless build never
                -- makes; create the TradeQuery (and its generator) on demand.
                if not tab.tradeQuery then
                    tab.tradeQuery = new("TradeQuery"):TradeQuery(tab)
                end
                local tq = tab.tradeQuery
                -- The generator is created in PriceItem (a UI popup); make it
                -- directly, as that function does.
                if not tq.tradeQueryGenerator then
                    tq.tradeQueryGenerator = new("TradeQueryGenerator"):TradeQueryGenerator(tq)
                end
                local gen = tq.tradeQueryGenerator
                local slotName = __pobios_trade_slot
                local slot = b.itemsTab.slots[slotName]
                if not slot then error("No such slot: " .. tostring(slotName), 0) end
                local slotTbl = { slotName = slotName }
                do
                    local item = slot.selItemId and b.itemsTab.items[slot.selItemId]
                    -- PoB's generator only special-cases two uniques
                    -- (Megalomaniac, Watcher's Eye); for any other unique it
                    -- passes special = { itemName = slotName }, which leaves
                    -- the item category nil and matches no mods, so the search
                    -- comes back empty. We instead search for rare upgrades of
                    -- the same base, which is what "find an upgrade" means, so
                    -- only flag `unique` for the slots PoB actually handles.
                    local specialUnique = slotName == "Watcher's Eye"
                    if item and item.rarity == "UNIQUE" and specialUnique then slotTbl.unique = true end
                    if item and item.corrupted then slotTbl.alreadyCorrupted = true end
                end
                local statWeights = __pobios_trade_weights or { { stat = "FullDPS", weightMult = 1 } }
                -- PoB's status list is index-matched between the UI labels and
                -- the API options (securable/available/onlineleague/online/any);
                -- FinishQuery reads self.tradeTypeIndex.
                gen.tradeTypeIndex = (__pobios_trade_buyout or 0) + 1
                __pobios_trade_result = nil
                __pobios_trade_error = nil
                __pobios_trade_done = false
                -- StartQuery is normally invoked by RequestQuery, which sets
                -- the callback FinishQuery calls. Set it before StartQuery,
                -- as RequestQuery does.
                gen.requesterCallback = function(_, query, errMsg)
                    __pobios_trade_result = query
                    __pobios_trade_error = errMsg
                    __pobios_trade_done = true
                end
                gen.requesterContext = { slotTbl = slotTbl }
                -- The pricing popup fills these; supply the same defaults so
                -- StartQuery has every field it reads.
                gen:StartQuery(slot, {
                    statWeights = statWeights,
                    requiredMods = {},
                    influence1 = 1, influence2 = 1,
                    sockets = 0, links = 0, maxLevel = 0,
                    maxPrice = 0, maxPriceType = nil,
                    jewelType = 1,
                    includeCorrupted = not slotTbl.alreadyCorrupted,
                    includeMirrored = false,
                    includeEldritch = false,
                    includeScourge = false,
                    includeTalisman = false,
                    includeAllWEMods = false,
                    account = nil,
                    special = slotTbl.unique and { itemName = slotName } or nil,
                })
                "#,
            )?.call::<()>(())?;

        // Drain the generator's coroutine by resuming it directly, the way
        // OnFrame does. We must NOT also pump the global app frame
        // (`runCallback("OnFrame")`): that re-runs PoB's build calc and
        // rebuilds the calc environment the coroutine's `calcFunc` closure
        // captured, so every test item's output collapses back to the base
        // and every mod weighs 0. Body Armour happened to survive it; every
        // other slot came back with no stat filters.
        for _ in 0..2000 {
            let done: bool = self.chunk(r#"
                    local b = __mainObject__.main.modes["BUILD"]
                    b.itemsTab.tradeQuery.tradeQueryGenerator:OnFrame()
                    return __pobios_trade_done == true
                    "#)?.call(())?;
            if done {
                break;
            }
        }

        let (query, err): (Option<String>, Option<String>) = self.chunk(r#"return __pobios_trade_result, __pobios_trade_error"#)?.call(())?;
        self.lua.globals().set("__pobios_trade_slot", mlua::Nil)?;
        self.lua.globals().set("__pobios_trade_weights", mlua::Nil)?;
        self.lua.globals().set("__pobios_trade_buyout", mlua::Nil)?;
        Self::check_prompt(&self.lua)?;
        match (query, err) {
            (Some(q), _) if !q.is_empty() => Ok(Ok(q)),
            (_, Some(e)) if !e.is_empty() => Ok(Err(e)),
            _ => Ok(Err("The trade query generator returned nothing.".to_owned())),
        }
    }

    pub fn rank_gems_for_group(&self, group: usize) -> Result<Vec<(String, f64, bool)>, Error> {
        self.lua.globals().set("__pobios_rank_group", group)?;
        let t: Option<Table> = self.chunk(
                r#"
                local gi = __pobios_rank_group
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.skillsTab
                local group = tab and tab.socketGroupList[gi]
                if not group or not group.displaySkillList or not group.displaySkillList[1] then
                    return nil
                end
                tab:SetDisplayGroup(group)

                -- PoB's GetMiscCalculator: one base env, re-run per gem.
                local calcFunc, calcBase = b.calcsTab:GetMiscCalculator(b)
                if not calcFunc then return nil end
                local field = tab.sortGemsByDPSField or "CombinedDPS"
                local useFull = field == "FullDPS"
                local function dpsOf(out)
                    return (useFull and out.FullDPS)
                        or (out.Minion and out.Minion.CombinedDPS)
                        or out[field] or 0
                end
                local base = dpsOf(calcBase)

                -- Which supports can support this group's active skill.
                local skills = group.displaySkillList
                local index = #group.gemList + 1
                local out = {}

                -- Supports already in the group are marked socketed=true and
                -- scored by their own contribution: disable one, recalc, and
                -- the DPS lost is what it is worth. That is the number to
                -- weigh a candidate's "added" DPS against when the group is
                -- full and one must come out.
                local socketed = {}
                for _, gem in ipairs(group.gemList) do
                    if gem.gemId then socketed[gem.gemId] = gem end
                end
                for gemId, gem in pairs(socketed) do
                    local ge = gem.gemData and gem.gemData.grantedEffect
                    if ge and ge.support then
                        local wasEnabled = gem.enabled
                        gem.enabled = false
                        local ok, res = pcall(calcFunc, nil, useFull)
                        gem.enabled = wasEnabled
                        if ok and res then
                            -- Worth: how much this support is currently adding
                            -- (base minus the build without it).
                            out[#out + 1] = { gemId, base - dpsOf(res), true }
                        end
                    end
                end

                for gemId, gemData in pairs(b.data.gems) do
                    local ge = gemData.grantedEffect
                    if ge and ge.support and not ge.hideFromGemList and not socketed[gemId] then
                        local can = false
                        for _, sk in ipairs(skills) do
                            if calcLib.canGrantedEffectSupportActiveSkill(ge, sk) then can = true break end
                        end
                        if can then
                            -- Slot the gem at the end of the list, calc, remove.
                            group.gemList[index] = {
                                level = gemData.naturalMaxLevel or 20,
                                quality = tab.defaultGemQuality or 0,
                                count = 1, enabled = true, enableGlobal1 = true, enableGlobal2 = true,
                                gemId = gemId, nameSpec = gemData.name, skillId = ge.id or ge.grantedEffectId,
                                gemData = gemData,
                            }
                            local ok, res = pcall(calcFunc, nil, useFull)
                            group.gemList[index] = nil
                            if ok and res then
                                out[#out + 1] = { gemId, dpsOf(res) - base, false }
                            end
                        end
                    end
                end
                -- Put the group back the way it was.
                tab:ProcessSocketGroup(group)
                runCallback("OnFrame")
                return out
                "#,
            )?.call(())?;
        self.lua.globals().set("__pobios_rank_group", mlua::Nil)?;
        Self::check_prompt(&self.lua)?;
        let Some(t) = t else { return Ok(Vec::new()) };
        let mut out: Vec<(String, f64, bool)> = Vec::new();
        for row in t.sequence_values::<Table>() {
            let row = row?;
            out.push((row.get(1)?, row.get(2)?, row.get(3)?));
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        Ok(out)
    }

    pub fn find_gem(&self, name: &str) -> Result<Option<skills::GemChoice>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local name = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.skillsTab
                if not tab then return nil end
                local err, gemData = tab:FindSkillGem(name)
                if err or not gemData then return nil end

                local ge = gemData.grantedEffect
                local letter = "W"
                if ge.color == 1 then letter = "R"
                elseif ge.color == 2 then letter = "G"
                elseif ge.color == 3 then letter = "B" end

                return {
                    id = gemData.id,
                    name = gemData.name or "",
                    color = letter,
                    support = ge.support and true or false,
                }
                "#,
            )?;

        let found: Option<mlua::Table> = f.call(name)?;
        Self::check_prompt(&self.lua)?;
        let Some(row) = found else { return Ok(None) };
        Ok(Some(skills::GemChoice {
            id: row.get::<String>("id")?,
            name: row.get::<String>("name")?,
            color: skills::SocketColor::parse(&row.get::<String>("color")?),
            support: row.get::<bool>("support")?,
        }))
    }

    /// The build's loadouts, as PoB's own dropdown lists them.
    ///
    /// A loadout is not a record. It is a tree spec, item set, skill set and
    /// config set that share a title — PoB derives the list by matching
    /// names, which is why renaming a set moves it between loadouts.
    ///
    /// Two matching modes, both upstream's. An exact title match, and a
    /// `{N}` link id in braces so sets can be grouped without identical
    /// names ("Early Mapping $ {2}" links to any set carrying `{2}`). A kind
    /// that holds only one set does not constrain the match at all, which is
    /// what makes loadouts work on a build that has never split its skills.
    ///
    /// Computed in Lua against the live tabs rather than reimplemented here:
    /// the rules are fiddly, they are upstream's to change, and getting them
    /// subtly wrong would produce a list that looks plausible and groups the
    /// wrong things.
    pub fn loadouts(&self) -> Result<Vec<String>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local trees, items, skills, config =
                    b.treeTab, b.itemsTab, b.skillsTab, b.configTab
                if not (trees and items and skills and config) then return {} end

                -- A kind with a single set does not constrain the match.
                local oneItem = #items.itemSetOrderList == 1
                local oneSkill = #skills.skillSetOrderList == 1
                local oneConfig = #config.configSetOrderList == 1

                -- Titles present for each kind, and the link ids in braces.
                local function titles(orderList, sets)
                    local byTitle, byLink = {}, {}
                    for _, id in ipairs(orderList) do
                        local set = sets[id]
                        local title = set and set.title or "Default"
                        byTitle[title] = true
                        for link in string.gmatch(
                            string.match(title, "%{([%w,]+)%}") or "", "[^%,]+"
                        ) do
                            byLink[link] = true
                        end
                    end
                    return byTitle, byLink
                end

                local itemTitles, itemLinks =
                    titles(items.itemSetOrderList, items.itemSets)
                local skillTitles, skillLinks =
                    titles(skills.skillSetOrderList, skills.skillSets)
                local configTitles, configLinks =
                    titles(config.configSetOrderList, config.configSets)

                local out, seen = {}, {}
                for _, spec in ipairs(trees.specList or {}) do
                    local title = spec.title or "Default"
                    local link = string.match(title, "%{([%w,]+)%}")

                    local matched
                    if link then
                        -- Link mode: every other kind must carry this id.
                        matched = (oneItem or itemLinks[link])
                            and (oneSkill or skillLinks[link])
                            and (oneConfig or configLinks[link])
                    else
                        -- Exact mode: every other kind must have this title.
                        matched = (oneItem or itemTitles[title])
                            and (oneSkill or skillTitles[title])
                            and (oneConfig or configTitles[title])
                    end

                    if matched and not seen[title] then
                        seen[title] = true
                        out[#out + 1] = title
                    end
                end
                return out
                "#,
            )?.call(())?;

        let mut out = Vec::new();
        for entry in list.sequence_values::<String>() {
            out.push(entry?);
        }
        Ok(out)
    }

    /// Switch every set to the one belonging to `loadout`.
    ///
    /// PoB's "New Loadout" creates four sets sharing a title, so switching
    /// means finding that title — or that `{N}` link — in each kind and
    /// activating it. A kind holding only one set is left alone: it is shared
    /// by every loadout rather than being wrong for all but one.
    pub fn set_active_loadout(&self, loadout: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local wanted = ...
                local b = __mainObject__.main.modes["BUILD"]
                local trees, items, skills, config =
                    b.treeTab, b.itemsTab, b.skillsTab, b.configTab
                if not (trees and items and skills and config) then return false end

                local link = string.match(wanted, "%{([%w,]+)%}")

                -- Does this set title belong to the loadout being switched to?
                local function belongs(title)
                    title = title or "Default"
                    if link then
                        for candidate in string.gmatch(
                            string.match(title, "%{([%w,]+)%}") or "", "[^%,]+"
                        ) do
                            if candidate == link then return true end
                        end
                        return false
                    end
                    return title == wanted
                end

                local function activate(orderList, sets, setter, tab)
                    -- One set means it is shared, not mismatched.
                    if #orderList <= 1 then return end
                    for _, id in ipairs(orderList) do
                        local set = sets[id]
                        if set and belongs(set.title) then
                            tab[setter](tab, id)
                            return
                        end
                    end
                end

                for i, spec in ipairs(trees.specList or {}) do
                    if belongs(spec.title) then
                        trees:SetActiveSpec(i)
                        break
                    end
                end
                activate(items.itemSetOrderList, items.itemSets,
                         "SetActiveItemSet", items)
                activate(skills.skillSetOrderList, skills.skillSets,
                         "SetActiveSkillSet", skills)
                activate(config.configSetOrderList, config.configSets,
                         "SetActiveConfigSet", config)

                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call(loadout)?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup("could not switch loadout".into()));
        }
        Ok(())
    }

    /// Create a loadout: one set of each kind, all sharing `title`.
    ///
    /// This is all "New Loadout" is in PoB — four sets with the same name.
    /// The tree spec is copied from the active one rather than started blank,
    /// because a loadout with no passive tree is not a usable starting point.
    pub fn new_loadout(&self, title: &str) -> Result<(), Error> {
        // The tree spec first, through the path that already handles copying
        // class and ascendancy across.
        self.new_spec(title)?;
        self.new_set(items::SetKind::Item, title)?;
        self.new_set(items::SetKind::Skill, title)?;
        self.new_set(items::SetKind::Config, title)?;
        Ok(())
    }

    /// The kinds of set a build scopes its data into.
    ///
    /// Trees are deliberately absent: PoB keeps those in `specList` with its
    /// own management dialogue, which this app already has.
    fn set_tab(kind: items::SetKind) -> (&'static str, &'static str, &'static str) {
        match kind {
            // (tab field, sets table, order list)
            items::SetKind::Item => ("itemsTab", "itemSets", "itemSetOrderList"),
            items::SetKind::Skill => ("skillsTab", "skillSets", "skillSetOrderList"),
            items::SetKind::Config => ("configTab", "configSets", "configSetOrderList"),
        }
    }

    /// The sets of one kind, in PoB's own order, with which is active.
    pub fn sets(&self, kind: items::SetKind) -> Result<items::SetList, Error> {
        let (tab, sets, order) = Self::set_tab(kind);
        let f: mlua::Function = self.chunk(
                r#"
                local tabName, setsField, orderField = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b[tabName]
                if not tab then return { sets = {}, active = 0 } end

                local out = {}
                for _, id in ipairs(tab[orderField] or {}) do
                    local set = tab[setsField] and tab[setsField][id]
                    if set then
                        out[#out + 1] = {
                            id = id,
                            -- PoB shows "Default" for an untitled set rather
                            -- than an empty row.
                            title = (set.title and set.title ~= "" )
                                and set.title or "Default",
                        }
                    end
                end

                -- The active id is named differently per tab, so try each.
                local active = tab.activeItemSetId or tab.activeSkillSetId
                    or tab.activeConfigSetId or 0
                return { sets = out, active = active }
                "#,
            )?;

        let raw: Table = f.call((tab, sets, order))?;
        let list: Table = raw.get("sets")?;
        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let e = entry?;
            out.push(items::Set {
                id: e.get::<i64>("id")?,
                title: e.get::<String>("title")?,
            });
        }
        Ok(items::SetList {
            sets: out,
            active: raw.get::<i64>("active")?,
        })
    }

    /// Switch which set of this kind is active.
    ///
    /// Goes through PoB's own `SetActive*Set`, which saves the outgoing set's
    /// state before loading the incoming one. Assigning the id alone would
    /// lose whatever was changed since the set was last switched.
    pub fn set_active_set(&self, kind: items::SetKind, id: i64) -> Result<(), Error> {
        let (tab, _, _) = Self::set_tab(kind);
        let func = match kind {
            items::SetKind::Item => "SetActiveItemSet",
            items::SetKind::Skill => "SetActiveSkillSet",
            items::SetKind::Config => "SetActiveConfigSet",
        };
        let f: mlua::Function = self.chunk(
                r#"
                local tabName, funcName, id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b[tabName]
                if not tab or not tab[funcName] then return false end
                tab[funcName](tab, id)
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call((tab, func, id))?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!("could not switch {tab} set")));
        }
        Ok(())
    }

    /// Create a set of this kind and return its id. Does not activate it.
    pub fn new_set(&self, kind: items::SetKind, title: &str) -> Result<i64, Error> {
        let (tab, _, order) = Self::set_tab(kind);
        let func = match kind {
            items::SetKind::Item => "NewItemSet",
            items::SetKind::Skill => "NewSkillSet",
            items::SetKind::Config => "NewConfigSet",
        };
        let f: mlua::Function = self.chunk(
                r#"
                local tabName, funcName, orderField, title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b[tabName]
                if not tab or not tab[funcName] then return 0 end

                local set = tab[funcName](tab)
                if not set then return 0 end
                set.title = title
                table.insert(tab[orderField], set.id)
                b.buildFlag = true
                runCallback("OnFrame")
                return set.id
                "#,
            )?;

        let id: i64 = f.call((tab, func, order, title))?;
        Self::check_prompt(&self.lua)?;
        if id == 0 {
            return Err(Error::PobStartup(format!("could not create {tab} set")));
        }
        Ok(id)
    }

    /// Rename a set. The title is what binds sets into a loadout, so this is
    /// not cosmetic: renaming can move a set between loadouts.
    pub fn rename_set(
        &self,
        kind: items::SetKind,
        id: i64,
        title: &str,
    ) -> Result<(), Error> {
        let (tab, sets, _) = Self::set_tab(kind);
        let f: mlua::Function = self.chunk(
                r#"
                local tabName, setsField, id, title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b[tabName]
                local set = tab and tab[setsField] and tab[setsField][id]
                if not set then return false end
                set.title = title
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?;

        let ok: bool = f.call((tab, sets, id, title))?;
        Self::check_prompt(&self.lua)?;
        if !ok {
            return Err(Error::PobStartup(format!("no {tab} set with id {id}")));
        }
        Ok(())
    }

    /// Delete a set. The last one cannot be deleted — a build always has at
    /// least one of each, and PoB's own dialogue greys Delete out rather than
    /// letting you reach a state with none.
    pub fn delete_set(&self, kind: items::SetKind, id: i64) -> Result<(), Error> {
        let (tab, sets, order) = Self::set_tab(kind);
        let f: mlua::Function = self.chunk(
                r#"
                local tabName, setsField, orderField, id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b[tabName]
                if not tab or not tab[setsField] or not tab[setsField][id] then
                    return "missing"
                end
                if #tab[orderField] <= 1 then return "last" end

                for i, existing in ipairs(tab[orderField]) do
                    if existing == id then
                        table.remove(tab[orderField], i)
                        break
                    end
                end
                tab[setsField][id] = nil

                -- Deleting the active set leaves nothing selected, so fall
                -- back to the first remaining one rather than to an id that
                -- no longer resolves.
                local activeField = tabName == "itemsTab" and "activeItemSetId"
                    or tabName == "skillsTab" and "activeSkillSetId"
                    or "activeConfigSetId"
                if tab[activeField] == id then
                    local setter = tabName == "itemsTab" and "SetActiveItemSet"
                        or tabName == "skillsTab" and "SetActiveSkillSet"
                        or "SetActiveConfigSet"
                    tab[setter](tab, tab[orderField][1])
                end

                b.buildFlag = true
                runCallback("OnFrame")
                return "ok"
                "#,
            )?;

        let result: String = f.call((tab, sets, order, id))?;
        Self::check_prompt(&self.lua)?;
        match result.as_str() {
            "ok" => Ok(()),
            "last" => Err(Error::PobStartup(
                "a build must keep at least one set of each kind".into(),
            )),
            _ => Err(Error::PobStartup(format!("no {tab} set with id {id}"))),
        }
    }

    /// The tree specs saved in this build, as PoB lists them.
    ///
    /// This is what PoB's own Compare works against: a build holds several
    /// passive trees, each with a `title` you set, and comparing means
    /// picking one from a dropdown. Returns `(index, label)` with the label
    /// formatted the way TreeTab formats it — an older tree version is
    /// prefixed in brackets, and an untitled spec is "Default".
    pub fn tree_specs(&self) -> Result<Vec<(usize, String)>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local out = {}
                for i, spec in ipairs(b.treeTab and b.treeTab.specList or {}) do
                    local prefix = ""
                    if spec.treeVersion ~= latestTreeVersion then
                        local v = treeVersions[spec.treeVersion]
                        prefix = "[" .. (v and v.display or spec.treeVersion) .. "] "
                    end
                    out[#out + 1] = { i, prefix .. (spec.title or "Default") }
                end
                return out
                "#,
            )?.call(())?;

        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let e = entry?;
            out.push((e.get(1)?, e.get(2)?));
        }
        Ok(out)
    }

    /// Create a new tree spec, inheriting the current class and ascendancy.
    ///
    /// Mirrors PoB's New: latest tree version, class and ascendancy copied
    /// from the *active* spec, and nothing else — no allocated nodes, no
    /// jewels, no masteries. Returns the new spec's index.
    pub fn new_spec(&self, title: &str) -> Result<usize, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab then return 0 end
                local spec = new("PassiveSpec")
                    :PassiveSpec(b, latestTreeVersion)
                spec:SelectClass(b.spec.curClassId)
                spec:SelectAscendClass(b.spec.curAscendClassId)
                spec:SelectSecondaryAscendClass(b.spec.curSecondaryAscendClassId)
                spec.title = title
                table.insert(tab.specList, spec)
                tab.modFlag = true
                return #tab.specList
                "#,
            )?;
        let index: usize = f.call(title)?;
        Self::check_prompt(&self.lua)?;
        Ok(index)
    }

    /// Duplicate a spec, including its tree, jewels and masteries.
    pub fn copy_spec(&self, index: usize, title: &str) -> Result<usize, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index, title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                local src = tab and tab.specList[index]
                if not src then return 0 end

                local spec = new("PassiveSpec"):PassiveSpec(b, src.treeVersion)
                spec.title = title
                spec.jewels = copyTable(src.jewels)
                spec:RestoreUndoState(src:CreateUndoState())
                -- PoB's CreateUndoState reads `secondaryAscendClassId` where
                -- the field is `curSecondaryAscendClassId`, so a copy loses
                -- the secondary ascendancy. Carry it across explicitly rather
                -- than reproducing the bug.
                if src.curSecondaryAscendClassId then
                    spec:SelectSecondaryAscendClass(src.curSecondaryAscendClassId)
                end
                spec:BuildClusterJewelGraphs()
                table.insert(tab.specList, spec)
                tab.modFlag = true
                return #tab.specList
                "#,
            )?;
        let new_index: usize = f.call((index, title))?;
        Self::check_prompt(&self.lua)?;
        Ok(new_index)
    }

    /// Rename a spec in place.
    pub fn rename_spec(&self, index: usize, title: &str) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index, title = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.treeTab and b.treeTab.specList[index]
                if spec then
                    spec.title = title
                    b.treeTab.modFlag = true
                end
                "#,
            )?;
        f.call::<()>((index, title))?;
        Self::check_prompt(&self.lua)
    }

    /// Delete a spec. Refuses to remove the last one, as PoB does.
    ///
    /// If the active spec is deleted, the previous one becomes active; if a
    /// spec above it goes, the active index shifts to follow the same tree.
    pub fn delete_spec(&self, index: usize) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab or #tab.specList <= 1 then return end
                if not tab.specList[index] then return end

                local wasActive = (index == tab.activeSpec)
                local current = b.spec
                table.remove(tab.specList, index)
                if wasActive then
                    tab:SetActiveSpec(math.max(1, index - 1))
                else
                    -- Re-derive by identity so the same tree stays active.
                    for i, s in ipairs(tab.specList) do
                        if s == current then tab.activeSpec = i break end
                    end
                end
                -- PoB leaves the compare index dangling here, so it can end
                -- up pointing at a different tree than the one chosen. Clamp
                -- it rather than reproduce that.
                if tab.activeCompareSpec then
                    tab.activeCompareSpec =
                        math.min(tab.activeCompareSpec, #tab.specList)
                end
                tab.modFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(index)?;
        Self::check_prompt(&self.lua)
    }

    /// Move a spec to a new position, as PoB's drag-reorder does.
    pub fn move_spec(&self, from: usize, to: usize) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local from, to = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab then return end
                local n = #tab.specList
                if from < 1 or from > n or to < 1 or to > n or from == to then
                    return
                end
                local current = b.spec
                local spec = table.remove(tab.specList, from)
                table.insert(tab.specList, to, spec)
                -- Active spec follows the tree, not the index.
                for i, s in ipairs(tab.specList) do
                    if s == current then tab.activeSpec = i break end
                end
                tab.modFlag = true
                "#,
            )?;
        f.call::<()>((from, to))?;
        Self::check_prompt(&self.lua)
    }

    /// Switch which spec is being edited.
    pub fn set_active_spec(&self, index: usize) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab or not tab.specList[index] then return end
                tab:SetActiveSpec(index)
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(index)?;
        Self::check_prompt(&self.lua)
    }

    /// Which spec is currently active, 1-based to match PoB's own indexing.
    /// The active tree spec's bare title, without the version and class
    /// decoration `tree_specs` adds for display.
    ///
    /// The title is what binds a spec into a loadout, so it is wanted raw.
    pub fn active_spec_title(&self) -> Result<String, Error> {
        Ok(self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b and b.treeTab
                if not tab then return "Default" end
                local spec = tab.specList and tab.specList[tab.activeSpec]
                local title = spec and spec.title
                return (title and title ~= "") and title or "Default"
                "#,
            )?.call(())?)
    }

    pub fn active_spec(&self) -> Result<usize, Error> {
        Ok(self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                return (b.treeTab and b.treeTab.activeSpec) or 1
                "#,
            )?.call(())?)
    }

    /// Stats for one of the build's saved specs, without switching to it.
    ///
    /// Reads by swapping the spec, calculating, and swapping back — the same
    /// approach the existing `stats_for` uses for a pasted build. Anything
    /// that leaves the wrong spec active would silently rewrite the user's
    /// tree, so the restore is unconditional.
    pub fn stats_for_spec(
        &self,
        index: usize,
        keys: &[&str],
    ) -> Result<std::collections::HashMap<String, f64>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local index, keys = ...
                local b = __mainObject__.main.modes["BUILD"]
                local tab = b.treeTab
                if not tab or not tab.specList[index] then return nil end

                local previous = tab.activeSpec
                local out = {}
                local ok, err = pcall(function()
                    tab:SetActiveSpec(index)
                    b.buildFlag = true
                    runCallback("OnFrame")
                    for _, k in ipairs(keys) do
                        local v = b.calcsTab and b.calcsTab.mainOutput
                            and b.calcsTab.mainOutput[k]
                        if type(v) == "number" then out[k] = v end
                    end
                end)

                -- Restore unconditionally: leaving another spec active would
                -- silently replace the tree the user is editing.
                tab:SetActiveSpec(previous)
                b.buildFlag = true
                runCallback("OnFrame")

                if not ok then error(err) end
                return out
                "#,
            )?;

        let table: Option<Table> = f.call((index, keys.to_vec()))?;
        let mut out = std::collections::HashMap::new();
        if let Some(t) = table {
            for pair in t.pairs::<String, f64>() {
                let (k, v) = pair?;
                out.insert(k, v);
            }
        }
        Self::check_prompt(&self.lua)?;
        Ok(out)
    }

    /// Node ids matching a search string, using PoB's own matcher.
    ///
    /// Deliberately not reimplemented. PoB's search is Lua pattern matching
    /// with quoted phrases, an `oil:` prefix for anoint recipes, and matches
    /// against a node's name, its stat lines *and* its reminder text. Writing
    /// a lookalike in Rust would silently disagree with the tooltip a user
    /// just read; borrowing the matcher cannot.
    pub fn search_nodes(&self, query: &str) -> Result<Vec<u64>, Error> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let f: mlua::Function = self.chunk(
                r#"
                local query = ...
                local b = __mainObject__.main.modes["BUILD"]
                local view = b.treeTab and b.treeTab.viewer
                if not view or not b.spec then return {} end

                -- Drive the viewer's own cached search rather than calling the
                -- matcher directly: the parameter list it matches against is
                -- built in the draw path, and a stale one would match the
                -- previous query.
                view.searchStr = query
                view.searchStrCached = nil
                local words = {}
                local rest = query:lower()
                for phrase in rest:gmatch('"([^"]*)"') do
                    words[#words + 1] = phrase
                    rest = rest:gsub('"' .. phrase:gsub("([%(%)])", "%%%1") .. '"', "")
                end
                for word in rest:gmatch("(%S+)") do
                    words[#words + 1] = word
                end
                view.searchParams = words

                local out = {}
                for id, node in pairs(b.spec.nodes or {}) do
                    local ok, matched = pcall(function()
                        return view:DoesNodeMatchSearchParams(node)
                    end)
                    if ok and matched then out[#out + 1] = id end
                end
                return out
                "#,
            )?;

        let list: Table = f.call(query)?;
        let mut out: Vec<u64> = list
            .sequence_values::<u64>()
            .collect::<Result<_, _>>()?;
        // Lua tables have no order; sort so results are stable frame to frame.
        out.sort_unstable();
        Ok(out)
    }

    /// What allocating (or unallocating) a node would do to the build.
    ///
    /// This is PoB's own hypothetical calculation: `GetMiscCalculator` returns
    /// a function that recomputes the build with extra nodes added, and the
    /// deltas are that result against the current one. Both the node alone and
    /// the whole path to it are reported, because a node three points away is
    /// a different proposition from one adjacent.
    ///
    /// Costs two full recalculations, so it is for a hovered node, not for
    /// every node on screen.
    pub fn node_preview(&self, node_id: u64) -> Result<Option<tree::NodePreview>, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec, calcs = b.spec, b.calcsTab
                if not spec or not calcs then return nil end
                local node = spec.nodes[id]
                if not node then return nil end
                -- Structural nodes are not choices, so there is nothing to
                -- preview: they cannot be allocated or removed by clicking.
                if node.type == "ClassStart"
                    or node.type == "AscendClassStart"
                    or node.type == "Mastery" then
                    return nil
                end

                local calcFunc, calcBase = calcs:GetMiscCalculator()
                if not calcFunc then return nil end

                -- The node on its own, then everything on the way to it.
                local nodeOutput, pathOutput
                local pathLength = (node.path and #node.path) or 0
                if node.alloc then
                    -- Removing a node removes everything that depended on it.
                    nodeOutput = calcFunc({ removeNodes = { [node] = true } })
                    if node.depends and #node.depends > 1 then
                        local deps = {}
                        for _, d in ipairs(node.depends) do deps[d] = true end
                        pathOutput = calcFunc({ removeNodes = deps })
                        pathLength = #node.depends
                    end
                else
                    nodeOutput = calcFunc({ addNodes = { [node] = true } })
                    if pathLength > 1 then
                        local pathNodes = {}
                        for _, p in ipairs(node.path) do pathNodes[p] = true end
                        pathOutput = calcFunc({ addNodes = pathNodes })
                    end
                end

                -- Walk PoB's own display stat list so the labels, formats and
                -- lower-is-better flags are its, not ours.
                local function compare(out, count)
                    local rows = {}
                    if not out then return rows end
                    local actor = calcs.mainEnv.player
                    for _, sd in ipairs(b.displayStats or {}) do
                        if sd.stat and not sd.childStat and sd.stat ~= "SkillDPS" then
                            local a = out[sd.stat] or 0
                            local base = calcBase[sd.stat] or 0
                            local diff = a - base
                            if diff > 0.001 or diff < -0.001 then
                                local mul = (sd.pc or sd.mod) and 100 or 1
                                local val = string.format("%+" .. sd.fmt, diff * mul)
                                local pc, pcPer = nil, nil
                                if sd.compPercent and a ~= 0 and base ~= 0 then
                                    pc = a / base * 100 - 100
                                    if count and count > 0 then pcPer = pc / count end
                                end
                                local per = nil
                                if count and count > 0 then
                                    per = string.format("%+" .. sd.fmt,
                                                        diff * mul / count)
                                end
                                local better = sd.lowerIsBetter and diff < 0
                                    or (not sd.lowerIsBetter and diff > 0)
                                rows[#rows + 1] = {
                                    tostring(sd.label or sd.stat),
                                    val,
                                    pc or false,
                                    per or false,
                                    pcPer or false,
                                    better and 1 or 0,
                                }
                            end
                        end
                    end
                    return rows
                end

                return {
                    node = compare(nodeOutput, nil),
                    path = compare(pathOutput, pathLength),
                    points = node.pathDist or pathLength,
                    alloc = node.alloc and 1 or 0,
                }
                "#,
            )?;

        let Some(raw): Option<Table> = f.call(node_id)? else {
            return Ok(None);
        };

        fn rows(t: Table) -> Result<Vec<tree::StatDelta>, Error> {
            let mut out = Vec::new();
            for entry in t.sequence_values::<Table>() {
                let e = entry?;
                // Absent values arrive as `false`, not nil: a nil inside a
                // Lua table constructor truncates the sequence, taking every
                // field after it with it. So decode loosely and treat a
                // boolean as "not present".
                let num = |v: mlua::Value| match v {
                    mlua::Value::Number(n) => Some(n),
                    mlua::Value::Integer(n) => Some(n as f64),
                    _ => None,
                };
                let text = |v: mlua::Value| match v {
                    mlua::Value::String(s) => s.to_str().ok().map(|s| s.to_string()),
                    _ => None,
                };
                out.push(tree::StatDelta {
                    label: e.get(1)?,
                    value: e.get(2)?,
                    percent: num(e.get(3)?),
                    per_point: text(e.get(4)?),
                    per_point_percent: num(e.get(5)?),
                    is_improvement: e.get::<u8>(6)? == 1,
                });
            }
            Ok(out)
        }

        Ok(Some(tree::NodePreview {
            node: rows(raw.get("node")?)?,
            path: rows(raw.get("path")?)?,
            path_points: raw.get::<Option<u32>>("points")?.unwrap_or(0),
            allocated: raw.get::<u8>("alloc")? == 1,
        }))
    }

    /// Toggle a node: allocate it, or deallocate it if already taken.
    ///
    /// Allocation is never just "set a flag". PoB allocates the whole
    /// shortest path to the node, and deallocation removes everything that
    /// depended on it — which is why this calls `AllocNode`/`DeallocNode`
    /// rather than touching `allocNodes` directly. A node that cannot be
    /// reached is left alone and reported, instead of being silently
    /// allocated into an island disconnected from the tree.
    ///
    /// Returns whether anything changed.
    pub fn toggle_node(&self, node_id: u64) -> Result<bool, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.spec
                if not spec then return false end
                local node = spec.nodes[id]
                if not node then return false end

                -- Class and ascendancy start nodes are structural: they are
                -- chosen through the class dropdowns, and clicking one should
                -- do nothing rather than corrupt the spec.
                if node.type == "ClassStart"
                    or node.type == "AscendClassStart" then
                    return false
                end

                local before = spec:CountAllocNodes()
                if node.alloc then
                    spec:DeallocNode(node)
                else
                    -- Unreachable nodes have no path; allocating one would
                    -- create an island the build cannot actually take.
                    if not node.path then return false end
                    spec:AllocNode(node)
                end
                spec:AddUndoState()
                spec:BuildAllDependsAndPaths()
                b.buildFlag = true
                runCallback("OnFrame")
                return spec:CountAllocNodes() ~= before
                "#,
            )?;
        let changed: bool = f.call(node_id)?;
        Self::check_prompt(&self.lua)?;
        Ok(changed)
    }

    /// A mastery node's selectable effects: `(effect id, label)`. Effects
    /// already chosen on another mastery of the same group are filtered out,
    /// as PoB's own popup does, so the list is only what can still be picked
    /// here. Empty for a non-mastery node or one with nothing left to choose.
    pub fn mastery_effects(&self, node_id: u64) -> Result<Vec<(u32, String)>, Error> {
        let raw: Table = self.chunk(
                r#"
                local id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.spec
                local out = {}
                if not spec then return out end
                local node = spec.nodes[id]
                if not node or node.type ~= "Mastery" or not node.masteryEffects then return out end
                for _, effect in pairs(node.masteryEffects) do
                    -- Skip an effect already taken by a different mastery.
                    local usedBy
                    for mastery, eff in pairs(spec.masterySelections) do
                        if eff == effect.effect then usedBy = mastery break end
                    end
                    if not usedBy or usedBy == node.id then
                        local ed = spec.tree.masteryEffects[effect.effect]
                        local label = ed and ed.sd and table.concat(ed.sd, " / ")
                            or (effect.stats and table.concat(effect.stats, " / "))
                            or tostring(effect.effect)
                        out[#out+1] = { effect.effect, label }
                    end
                end
                return out
                "#,
            )?.call(node_id)?;
        let mut effects = Vec::new();
        for pair in raw.sequence_values::<Table>() {
            let pair = pair?;
            let id: u32 = pair.get(1)?;
            let label = crate::calcs::strip_color_escapes(&pair.get::<String>(2)?);
            effects.push((id, label.trim().to_owned()));
        }
        Ok(effects)
    }

    /// The currently chosen effect id for a mastery node, if any.
    pub fn mastery_selection(&self, node_id: u64) -> Result<Option<u32>, Error> {
        let sel: Option<u32> = self.chunk(
                r#"
                local id = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.spec
                if not spec then return nil end
                return spec.masterySelections[id]
                "#,
            )?.call(node_id)?;
        Ok(sel)
    }

    /// Choose `effect_id` for the mastery `node_id`, allocating the node if
    /// needed. Mirrors PoB's SaveMasteryPopup: set the node's stats to the
    /// chosen effect, record the selection, allocate, and recalc.
    pub fn set_mastery_effect(&self, node_id: u64, effect_id: u32) -> Result<bool, Error> {
        let ok: bool = self.chunk(
                r#"
                local id, effectId = ...
                local b = __mainObject__.main.modes["BUILD"]
                local spec = b.spec
                if not spec then return false end
                local node = spec.nodes[id]
                if not node or node.type ~= "Mastery" then return false end
                local effect = spec.tree.masteryEffects[effectId]
                if not effect then return false end
                node.sd = effect.sd
                node.allMasteryOptions = false
                node.reminderText = { "Tip: pick a different effect to change it" }
                spec.tree:ProcessStats(node)
                spec.masterySelections[node.id] = effect.id
                if not node.alloc then
                    if not node.path then return false end
                    spec:AllocNode(node)
                end
                spec:AddUndoState()
                spec:BuildAllDependsAndPaths()
                b.buildFlag = true
                runCallback("OnFrame")
                return true
                "#,
            )?.call((node_id, effect_id))?;
        Self::check_prompt(&self.lua)?;
        Ok(ok)
    }

    /// Begin ranking unallocated nodes by what taking them would gain.
    ///
    /// Split into start/step/collect rather than run to completion, because
    /// PoB computes this by re-running the *whole* calculation once per
    /// candidate node — about fifteen seconds on a real build. Doing that in
    /// one call blocks whatever thread the engine lives on, and since `Lua`
    /// is not `Send` that thread is the UI thread: the window stops painting
    /// and the OS shows a spinner. Upstream drives it as a coroutine across
    /// frames for exactly this reason, and so do we.
    ///
    /// `stat` picks the measure (a key from PoB's power stat list such as
    /// `"TotalDPS"`); `None` uses PoB's combined offence/defence estimate.
    pub fn node_power_start(&self, stat: Option<&str>) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local stat = ...
                local b = __mainObject__.main.modes["BUILD"]
                local calcs = b.calcsTab
                if not calcs or not b.spec then return end

                -- Pick the measure. nil means PoB's combined offence/defence.
                calcs.powerStat = nil
                if stat == "" then stat = nil end
                if stat then
                    for _, s in ipairs(data.powerStatList or {}) do
                        if s.stat == stat then calcs.powerStat = s break end
                    end
                end
                calcs.powerBuildFlag = true
                -- PoB's PowerBuilder yields after 100ms of its own clock,
                -- which is a dropped frame per step on a device. Run it on
                -- a clock that reads 16x faster, so it yields every ~6ms of
                -- real work and the caller can pack steps into a frame
                -- budget; node_power_step restores the clock when the
                -- coroutine finishes. And keep its progress figure.
                if not b.__pobRealGetTime then
                    b.__pobRealGetTime = GetTime
                    GetTime = function() return b.__pobRealGetTime() * 16 end
                end
                b.__pobPowerProgress = 0
                b.powerBuilderProgressCallback = function(p) b.__pobPowerProgress = p end
                "#,
            )?;
        f.call::<()>(stat)?;
        Ok(())
    }

    /// PoB's node-power measures, from `data.powerStatList`, minus the ones
    /// PoB itself hides from the tree.
    pub fn node_power_stats(&self) -> Result<Vec<skills::PowerStat>, Error> {
        let list: Table = self.chunk(
                r#"
                local out = {}
                for _, s in ipairs(data.powerStatList or {}) do
                    if not s.ignoreForNodes then
                        out[#out + 1] = { s.stat or "", s.label or (s.stat or "") }
                    end
                end
                return out
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for pair in list.sequence_values::<Table>() {
            let pair = pair?;
            out.push(skills::PowerStat { stat: pair.get(1)?, label: pair.get(2)? });
        }
        Ok(out)
    }

    /// PoB's Power Report, built the way `TreeTab:BuildPowerReportList`
    /// builds it: normal, notable and keystone nodes that are not
    /// ascendancy nodes, masteries with computed effects, and unallocated
    /// cluster notables. Sorted best first by PoB's own rule.
    pub fn node_power_report(&self) -> Result<Vec<skills::PowerReportRow>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local report = {}
                local calcs = b.calcsTab
                if not (b.spec and calcs and calcs.powerStat ~= nil or b.spec) then return report end
                local current = calcs and calcs.powerStat
                local displayStat = nil
                if current and current.stat then
                    for _, ds in ipairs(b.displayStats) do
                        if ds.stat == current.stat then displayStat = ds break end
                    end
                end
                displayStat = displayStat or { fmt = ".1f" }
                local lowerIsBetter = displayStat.lowerIsBetter and true or false
                local mult = (displayStat.pc or displayStat.mod) and 100 or 1
                local function fmt(power)
                    return formatNumSep(string.format("%" .. displayStat.fmt, power))
                end
                local function pathDist(node, isAlloc)
                    if isAlloc then
                        return #(node.depends or {}) == 0 and 1 or #node.depends
                    end
                    return node.power.distance or (#(node.path or {}) == 0 and 1 or #node.path)
                end
                local function add(node, name, nodePower, pathPower, dist, isAlloc, perPointStr)
                    report[#report + 1] = {
                        node.id, name, node.type or "", nodePower, fmt(nodePower),
                        perPointStr or fmt(pathPower), tostring(dist), isAlloc and true or false
                    }
                end
                -- Which value a node carries: the chosen stat, else PoB's
                -- combined offence when no stat is chosen.
                local function value(p)
                    if not p then return nil end
                    if current and current.stat then return p.singleStat end
                    return p.offence
                end
                local granted = calcs.mainEnv and calcs.mainEnv.grantedPassives or {}
                for nodeId, node in pairs(b.spec.nodes) do
                    local isAlloc = node.alloc or granted[nodeId]
                    if node.power and (node.type == "Normal" or node.type == "Keystone" or node.type == "Notable") and not node.ascendancyName then
                        local dist = pathDist(node, isAlloc)
                        local np = (value(node.power) or 0) * mult
                        local pp = (node.power.pathPower or 0) / dist * mult
                        add(node, node.dn, np, pp, dist, isAlloc)
                    elseif node.type == "Mastery" and node.power and node.power.masteryEffects and not node.ascendancyName then
                        local dist = pathDist(node, isAlloc)
                        for _, me in ipairs(node.masteryEffects or {}) do
                            local effect = b.spec.tree.masteryEffects[me.effect]
                            local ep = node.power.masteryEffects[me.effect]
                            if effect and ep then
                                local parts = isAlloc and not node.allMasteryOptions and node.sd or effect.stats or effect.sd
                                local name = parts and (node.dn .. ": " .. table.concat(parts, " / ")) or node.dn
                                local np = (value(ep) or 0) * mult
                                local pp = ((ep.pathPower or value(ep) or 0) / dist) * mult
                                add(node, name, np, pp, dist, isAlloc)
                            end
                        end
                    end
                end
                for _, node in pairs(b.spec.tree.clusterNodeMap or {}) do
                    if not node.alloc and node.power then
                        local np = (value(node.power) or 0) * mult
                        add(node, node.dn, np, 0, "Cluster", false, "--")
                    end
                end
                if lowerIsBetter then
                    table.sort(report, function(a, c) return a[4] < c[4] end)
                else
                    table.sort(report, function(a, c) return a[4] > c[4] end)
                end
                for _, r in ipairs(report) do r[9] = lowerIsBetter end
                return report
                "#,
            )?.call(())?;
        let mut out = Vec::new();
        for row in list.sequence_values::<Table>() {
            let row = row?;
            out.push(skills::PowerReportRow {
                id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                power: row.get(4)?,
                power_text: row.get(5)?,
                per_point_text: row.get(6)?,
                points_text: row.get(7)?,
                allocated: row.get(8)?,
                lower_is_better: row.get(9)?,
            });
        }
        Ok(out)
    }

    /// Percent complete of a ranking in progress, from PoB's own callback.
    pub fn node_power_progress(&self) -> Result<u32, Error> {
        let p: Option<f64> = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                return b.__pobPowerProgress
                "#,
            )?.call(())?;
        Ok(p.unwrap_or(0.0).clamp(0.0, 100.0) as u32)
    }

    /// Advance the ranking. Returns true once it has finished.
    ///
    /// One call resumes PoB's coroutine until its own ~100ms yield, so this
    /// costs roughly one dropped frame — call it once per frame while a
    /// ranking is in progress.
    pub fn node_power_step(&self) -> Result<bool, Error> {
        let done: bool = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local calcs = b.calcsTab
                if not calcs then return true end
                calcs:BuildPower()
                local done = calcs.powerBuilder == nil
                if done and b.__pobRealGetTime then
                    GetTime = b.__pobRealGetTime
                    b.__pobRealGetTime = nil
                    b.powerBuilderProgressCallback = nil
                end
                return done
                "#,
            )?.call(())?;
        Ok(done)
    }

    /// Stop a ranking part-way and put the clock back.
    ///
    /// Without this, cancelling from the UI only stopped the caller's
    /// timer: PoB's builder stayed parked mid-coroutine and `GetTime` kept
    /// running 16x fast for the rest of the session, so every later
    /// time-based yield in PoB (the trade generator, for one) fired early.
    pub fn node_power_cancel(&self) -> Result<(), Error> {
        self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                if not b then return end
                local calcs = b.calcsTab
                if calcs then
                    calcs.powerBuilder = nil
                    calcs.powerBuildFlag = false
                end
                if b.__pobRealGetTime then
                    GetTime = b.__pobRealGetTime
                    b.__pobRealGetTime = nil
                end
                b.powerBuilderProgressCallback = nil
                b.__pobPowerProgress = 0
                "#,
            )?.call::<()>(())?;
        Ok(())
    }

    /// Read the finished ranking, best first.
    pub fn node_power_results(&self) -> Result<Vec<(u64, f64)>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local out = {}
                if not b.spec then return out end
                -- The same nodes the Power Report lists: PoB scores every
                -- ascendancy node of every class too, and tinting those
                -- lights up circles the build cannot even reach.
                for id, node in pairs(b.spec.nodes or {}) do
                    local p = node.power
                    local t = node.type
                    if p and not node.ascendancyName
                        and (t == "Normal" or t == "Notable" or t == "Keystone" or t == "Mastery") then
                        local v = p.singleStat or p.offence
                        if v and v ~= 0 then
                            out[#out + 1] = { id, v }
                        end
                    end
                end
                return out
                "#,
            )?.call(())?;

        let mut out: Vec<(u64, f64)> = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let entry = entry?;
            out.push((entry.get(1)?, entry.get(2)?));
        }
        // Best first, and stable: Lua table order is arbitrary.
        out.sort_unstable_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        Ok(out)
    }

    /// Rank every unallocated node, blocking until done.
    ///
    /// Only safe off the UI thread, or in tests. Interactive callers want
    /// [`Engine::node_power_start`] and [`Engine::node_power_step`].
    pub fn node_power(&self, stat: Option<&str>) -> Result<Vec<(u64, f64)>, Error> {
        self.node_power_start(stat)?;
        // Bounded so a pathological build cannot spin forever.
        for _ in 0..100_000 {
            if self.node_power_step()? {
                break;
            }
        }
        self.node_power_results()
    }

    /// Set the character level, pinning it to manual.
    ///
    /// PoB clamps to 1-100 and normally drives this itself from the number of
    /// allocated points ("Auto"). Typing a level is what switches it to
    /// manual, exactly as it does upstream, so the value stops being
    /// overwritten on the next recalculation.
    pub fn set_level(&self, level: i64) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local level = ...
                local b = __mainObject__.main.modes["BUILD"]
                b.characterLevel = math.min(math.max(level, 1), 100)
                b.characterLevelAutoMode = false
                if b.configTab then b.configTab:BuildModList() end
                b.modFlag = true
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(level)?;
        Self::check_prompt(&self.lua)
    }

    /// Hand levelling back to PoB, which derives it from allocated points.
    pub fn set_level_auto(&self) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                b.characterLevelAutoMode = true
                if b.EstimatePlayerProgress then b:EstimatePlayerProgress() end
                if b.configTab then b.configTab:BuildModList() end
                b.modFlag = true
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(())?;
        Self::check_prompt(&self.lua)
    }

    /// Whether switching to `class_id` would wipe the allocated tree.
    ///
    /// Changing class only preserves a build when that class's start node is
    /// already connected to it; otherwise PoB resets the tree. It asks first,
    /// and so must we — this is the check behind that question.
    pub fn class_change_resets_tree(&self, class_id: u8) -> Result<bool, Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local classId = ...
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return false end
                if b.spec:CountAllocNodes() == 0 then return false end
                return not b.spec:IsClassConnected(classId)
                "#,
            )?;
        Ok(f.call::<bool>(class_id)?)
    }

    /// Switch class. Resets the tree when the new start node is unconnected;
    /// call [`Engine::class_change_resets_tree`] first if that matters.
    pub fn set_class(&self, class_id: u8) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local classId = ...
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return end
                b.spec:SelectClass(classId)
                b.spec:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(class_id)?;
        Self::check_prompt(&self.lua)
    }

    /// Switch ascendancy within the current class. Never resets the tree.
    pub fn set_ascendancy(&self, ascend_class_id: u8) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local id = ...
                local b = __mainObject__.main.modes["BUILD"]
                if not b.spec then return end
                b.spec:SelectAscendClass(id)
                b.spec:AddUndoState()
                b.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        f.call::<()>(ascend_class_id)?;
        Self::check_prompt(&self.lua)
    }

    /// Every class, with its ascendancies, as PoB lists them.
    pub fn classes(&self) -> Result<Vec<ClassChoice>, Error> {
        let list: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local out = {}
                if not b.spec or not b.spec.tree then return out end
                for classId, class in pairs(b.spec.tree.classes or {}) do
                    local ascend = {}
                    for i, a in ipairs(class.ascendancies or {}) do
                        ascend[#ascend + 1] = { i, tostring(a.name or "") }
                    end
                    out[#out + 1] = {
                        classId,
                        tostring(class.name or ""),
                        ascend,
                    }
                end
                table.sort(out, function(a, b) return a[2] < b[2] end)
                return out
                "#,
            )?.call(())?;

        let mut out = Vec::new();
        for entry in list.sequence_values::<Table>() {
            let entry = entry?;
            let ascend_table: Table = entry.get(3)?;
            let mut ascendancies = Vec::new();
            for a in ascend_table.sequence_values::<Table>() {
                let a = a?;
                ascendancies.push((a.get(1)?, a.get(2)?));
            }
            out.push(ClassChoice {
                id: entry.get(1)?,
                name: entry.get(2)?,
                ascendancies,
            });
        }
        Ok(out)
    }

    pub fn info(&self) -> Result<BuildInfo, Error> {
        let build = self.build()?;
        let spec: Option<Table> = build.get("spec")?;

        let (class, ascendancy, nodes) = match &spec {
            Some(s) => {
                let class: Option<String> = s.get("curClassName")?;
                let asc: Option<String> = s.get("curAscendClassName")?;
                let alloc: Option<Table> = s.get("allocNodes")?;
                let count = alloc.map_or(0, |t| t.pairs::<mlua::Value, mlua::Value>().count());
                (class.unwrap_or_default(), asc.unwrap_or_default(), count)
            }
            None => (String::new(), String::new(), 0),
        };

        // PoB recomputes the point totals in EstimatePlayerProgress, which
        // also drives its auto-levelling. Call it so the numbers reflect the
        // current tree rather than whatever the last frame left behind.
        let points: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                if b.EstimatePlayerProgress then b:EstimatePlayerProgress() end
                -- CountAllocNodes returns six values in PoB2; the weapon-set
                -- pair matters here. Passives shared into a weapon set do not
                -- count against the tree budget, so PoB2 compares
                -- PointsUsed - min(weaponSet1, weaponSet2) to the max
                -- (Build.lua EstimatePlayerProgress).
                local used, asc, ws1, ws2 = 0, 0, 0, 0
                if b.spec and b.spec.CountAllocNodes then
                    local pu, au, _sec, _sock, w1, w2 = b.spec:CountAllocNodes()
                    used, asc, ws1, ws2 = pu or 0, au or 0, w1 or 0, w2 or 0
                end
                local extra = b.calcsTab and b.calcsTab.mainOutput
                    and b.calcsTab.mainOutput.ExtraPoints or 0
                -- PoB2's own budget: 99 from levels plus the campaign's total
                -- quest points, which PoB2 tracks as maxWeaponSets. PoE1's
                -- fixed 23 undercounts it and reads a valid build as over.
                local questMax = b.maxWeaponSets or 23
                return {
                    used = used - math.min(ws1, ws2),
                    max = 99 + questMax + extra,
                    ascUsed = asc,
                    ascMax = 8,
                    auto = b.characterLevelAutoMode and 1 or 0,
                }
                "#,
            )?.call(())?;

        Ok(BuildInfo {
            class,
            ascendancy,
            level: build.get::<Option<i64>>("characterLevel")?.unwrap_or(0),
            allocated_nodes: nodes,
            tree_version: spec
                .and_then(|s| s.get::<Option<String>>("treeVersion").ok().flatten())
                .unwrap_or_default(),
            points_used: points.get("used")?,
            points_max: points.get("max")?,
            ascendancy_points_used: points.get("ascUsed")?,
            ascendancy_points_max: points.get("ascMax")?,
            level_is_auto: points.get::<u8>("auto")? == 1,
        })
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn a_runaway_lua_loop_is_stopped_by_the_time_budget() {
        let lua = Lua::new();
        let r = run_with_budget(&lua, "a test", Duration::from_millis(100), usize::MAX, || {
            lua.load("while true do end").exec()?;
            Ok(())
        });
        match r {
            Err(Error::Budget { what: "a test", reason }) => assert!(reason.contains("longer than")),
            other => panic!("expected a budget error, got {other:?}"),
        }
    }

    #[test]
    fn a_memory_hungry_load_is_stopped_by_the_cap() {
        let lua = Lua::new();
        let r = run_with_budget(&lua, "a test", Duration::from_secs(30), 8 * 1024 * 1024, || {
            lua.load("local t = {} for i = 1, 1e8 do t[i] = { i } end").exec()?;
            Ok(())
        });
        match r {
            Err(Error::Budget { reason, .. }) => assert!(reason.contains("MB")),
            other => panic!("expected a budget error, got {other:?}"),
        }
    }

    #[test]
    fn a_load_within_budget_returns_its_value_and_leaves_no_hook() {
        let lua = Lua::new();
        let v = run_with_budget(&lua, "a test", Duration::from_secs(5), usize::MAX, || {
            Ok(lua.load("return 40 + 2").eval::<i64>()?)
        })
        .unwrap();
        assert_eq!(v, 42);
        // Nothing left behind: a long loop after the call is not interrupted.
        lua.load("local n = 0 for i = 1, 200000 do n = n + i end").exec().unwrap();
    }
}
