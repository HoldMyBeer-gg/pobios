//! The loaded build, and the engine that computes it.
//!
//! Engine startup can fail — the PoB checkout may be missing, or upstream may
//! have moved something. That is a degraded UI, not a crash: the shell still
//! runs and says what went wrong, because a build tool that refuses to open is
//! worse than one that opens and reports it cannot calculate.

use pob_engine::calcs::Section;
use pob_engine::config::{ConfigOption, Value};
use pob_engine::items::Slot;
use pob_engine::tree::Tree;
use pob_engine::skills::SocketGroup;
use pob_engine::stats::ShowWhen;
use pob_engine::{BuildInfo, Engine, stats::StatSpec};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct BuildState {
    engine: Option<Engine>,
    /// Why the engine is unavailable, if it is.
    pub engine_error: Option<String>,
    pub info: BuildInfo,
    /// Last computed values, keyed by PoB's stat name.
    values: HashMap<String, f64>,
    /// Our own problems: the engine failing, a build refusing to load.
    pub warnings: Vec<String>,
    /// PoB's findings about the build itself — over-reserved mana, unmet
    /// requirements, too many passives. Kept separate from ours because they
    /// mean different things: one is "this app broke", the other is "your
    /// build is broken", and conflating them makes both easier to ignore.
    pub build_warnings: Vec<String>,
    pub socket_groups: Vec<SocketGroup>,
    /// Index of the group PoB treats as the main skill.
    pub main_socket_group: usize,
    pub equipment: Vec<Slot>,
    pub calc_sections: Vec<Section>,
    pub config_options: Vec<ConfigOption>,
    pub notes: String,
    /// Shared, not owned: the canvas draws from a snapshot of this on the
    /// render thread while the engine lives on its own. Every reload swaps
    /// in a fresh `Arc`, so a frame in progress keeps the tree it started
    /// with and the next frame picks up the new one.
    pub tree: Arc<Tree>,
    /// Every class and its ascendancies. Read once — the list is a property
    /// of the tree data, not of the build.
    pub classes: Vec<pob_engine::ClassChoice>,
}

/// Where the vendored PoB checkout lives relative to the repo root.
/// Where PoB's `src/` lives, set once by the host at startup.
///
/// Baked-in build paths do not survive leaving the build machine — the iPad
/// has no `~/code` checkout. Desktop still defaults to the checkout, so
/// nothing there has to change.
static POB_SRC: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Point the engine at PoB's `src/`. Call before creating a `BuildState`.
///
/// On iOS this is the app bundle; on desktop it is the vendored checkout and
/// does not need calling at all.
pub fn set_pob_src(path: impl Into<PathBuf>) {
    let _ = POB_SRC.set(path.into());
}

/// The data root, for callers outside this module.
pub fn pob_src_path() -> PathBuf {
    pob_src()
}

fn pob_src() -> PathBuf {
    POB_SRC
        .get()
        .cloned()
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../PathOfBuilding-PoE2/src")
        })
}

impl BuildState {
    /// Boot the engine and load a build. Never fails: a broken engine leaves
    /// the UI running with an explanation.
    pub fn new(build_xml: Option<&str>) -> Self {
        let mut state = Self {
            engine: None,
            engine_error: None,
            info: BuildInfo::default(),
            classes: Vec::new(),
            values: HashMap::new(),
            warnings: Vec::new(),
            build_warnings: Vec::new(),
            socket_groups: Vec::new(),
            main_socket_group: 0,
            equipment: Vec::new(),
            calc_sections: Vec::new(),
            config_options: Vec::new(),
            notes: String::new(),
            tree: Arc::new(Tree::default()),
        };

        match Engine::new(&pob_src()) {
            Ok(engine) => state.engine = Some(engine),
            Err(e) => {
                state.engine_error = Some(e.to_string());
                state.warnings.push(format!("Calculation engine unavailable: {e}"));
                return state;
            }
        }

        if let Some(xml) = build_xml {
            state.load(xml);
        }
        state
    }

    pub fn load(&mut self, xml: &str) {
        let Some(engine) = &self.engine else { return };

        if let Err(e) = engine.load_build_xml(xml) {
            self.warnings.push(format!("Could not load build: {e}"));
            return;
        }

        match engine.info() {
            Ok(info) => self.info = info,
            Err(e) => self.warnings.push(format!("Could not read build info: {e}")),
        }

        match engine.socket_groups() {
            Ok(groups) => self.socket_groups = groups,
            Err(e) => self.warnings.push(format!("Could not read skills: {e}")),
        }
        self.main_socket_group = engine.main_socket_group().unwrap_or(0);

        match engine.equipment() {
            Ok(slots) => self.equipment = slots,
            Err(e) => self.warnings.push(format!("Could not read items: {e}")),
        }

        match engine.calc_sections() {
            Ok(sections) => self.calc_sections = sections,
            Err(e) => self.warnings.push(format!("Could not read calcs: {e}")),
        }

        match engine.config_options() {
            Ok(options) => self.config_options = options,
            Err(e) => self.warnings.push(format!("Could not read config: {e}")),
        }

        match engine.notes() {
            Ok(notes) => self.notes = notes,
            Err(e) => self.warnings.push(format!("Could not read notes: {e}")),
        }

        match engine.tree() {
            Ok(tree) => self.tree = Arc::new(tree),
            Err(e) => self.warnings.push(format!("Could not read tree: {e}")),
        }

        if self.classes.is_empty() {
            match engine.classes() {
                Ok(classes) => self.classes = classes,
                Err(e) => self.warnings.push(format!("Could not read classes: {e}")),
            }
        }

        self.refresh();
    }

    /// Would switching to this class wipe the allocated tree?
    ///
    /// PoB asks before doing that, and so must we: a class change only keeps
    /// a build when the new start node is already connected to it.
    pub fn classes(&self) -> Vec<pob_engine::ClassChoice> {
        self.engine.as_ref().and_then(|e| e.classes().ok()).unwrap_or_default()
    }

    pub fn class_change_resets_tree(&self, class_id: u8) -> bool {
        self.engine
            .as_ref()
            .and_then(|e| e.class_change_resets_tree(class_id).ok())
            .unwrap_or(false)
    }

    /// Apply a character-level change and re-read everything it affects.
    ///
    /// Level, class and ascendancy all feed the calculation, so this reloads
    /// the derived state rather than only the field that changed — a class
    /// switch can de-allocate half a tree.
    pub fn apply(&mut self, edit: &crate::build_edit::BuildEdit) {
        use crate::build_edit::BuildEdit;
        let Some(engine) = &self.engine else { return };

        let result = match edit {
            BuildEdit::Level(n) => engine.set_level(*n),
            BuildEdit::LevelAuto => engine.set_level_auto(),
            BuildEdit::Class(id) => engine.set_class(*id),
            BuildEdit::Ascendancy(id) => engine.set_ascendancy(*id),
            // Loadouts move every set at once, so they need the whole build
            // re-read rather than this function's info-and-tree refresh.
            // Handled by `set_active_loadout` and `new_loadout` instead.
            BuildEdit::Loadout(_) | BuildEdit::NewLoadout => return,
        };
        if let Err(e) = result {
            self.warnings.push(format!("Could not apply change: {e}"));
            return;
        }

        match engine.info() {
            Ok(info) => self.info = info,
            Err(e) => self.warnings.push(format!("Could not read build info: {e}")),
        }
        match engine.tree() {
            Ok(tree) => self.tree = Arc::new(tree),
            Err(e) => self.warnings.push(format!("Could not read tree: {e}")),
        }
        self.refresh();
    }

    /// The build's saved tree specs, as PoB labels them.
    pub fn tree_specs(&self) -> Vec<(usize, String)> {
        self.engine
            .as_ref()
            .and_then(|e| e.tree_specs().ok())
            .unwrap_or_default()
    }

    /// Change a gem's level, quality or enabled state.
    ///
    /// Re-reads the skill list as well as the stats: PoB re-resolves a gem
    /// against its data on every edit, so the level it settles on may not be
    /// the one asked for — a level 25 request on a gem that caps at 21 comes
    /// back as 21, and the UI has to show what was actually applied.
    pub fn set_gem(
        &mut self,
        group: usize,
        gem: usize,
        level: Option<i64>,
        quality: Option<i64>,
        enabled: Option<bool>,
        count: Option<i64>,
    ) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_gem(group, gem, level, quality, enabled, count) {
            self.warnings.push(format!("Could not change gem: {e}"));
            return;
        }
        self.reread_skills();
    }

    /// Put a different gem in a slot, or add one if `gem` is past the end.
    pub fn set_gem_id(&mut self, group: usize, gem: usize, gem_id: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_gem_id(group, gem, gem_id) {
            self.warnings.push(format!("Could not change gem: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn remove_gem(&mut self, group: usize, gem: usize) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.remove_gem(group, gem) {
            self.warnings.push(format!("Could not remove gem: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn main_skill_controls(&self) -> pob_engine::skills::MainSkillControls {
        self.engine
            .as_ref()
            .and_then(|e| e.main_skill_controls().ok())
            .unwrap_or_default()
    }

    /// Apply one main-skill control write and re-read what it moved.
    fn main_skill_write(&mut self, what: &str, r: Result<(), pob_engine::Error>) {
        if let Err(e) = r {
            self.warnings.push(format!("Could not change {what}: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn set_main_active_skill(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_active_skill(index);
        self.main_skill_write("active skill", r);
    }

    pub fn set_main_skill_part(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_skill_part(index);
        self.main_skill_write("skill part", r);
    }

    pub fn set_main_skill_stages(&mut self, count: i64) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_skill_stages(count);
        self.main_skill_write("stages", r);
    }

    pub fn set_main_skill_mines(&mut self, count: i64) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_skill_mines(count);
        self.main_skill_write("mines", r);
    }

    pub fn set_main_skill_minion(&mut self, value: &str, by_item_set: bool) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_skill_minion(value, by_item_set);
        self.main_skill_write("minion", r);
    }

    pub fn set_main_skill_minion_skill(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_main_skill_minion_skill(index);
        self.main_skill_write("minion skill", r);
    }

    pub fn set_main_group(&mut self, group: usize) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_main_group(group) {
            self.warnings.push(format!("Could not set main skill: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn set_group_full_dps(&mut self, group: usize, include: bool) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_group_full_dps(group, include) {
            self.warnings.push(format!("Could not change Full DPS: {e}"));
            return;
        }
        self.reread_skills();
    }

    /// Add an empty socket group; returns its 1-based index.
    pub fn new_socket_group(&mut self) -> Option<usize> {
        let i = self.engine.as_ref()?.new_socket_group().ok()?;
        self.reread_skills();
        Some(i)
    }

    /// Delete a socket group. False when PoB refuses (item-granted).
    pub fn delete_socket_group(&mut self, group: usize) -> bool {
        let Some(engine) = &self.engine else { return false };
        match engine.delete_socket_group(group) {
            Ok(ok) => {
                self.reread_skills();
                ok
            }
            Err(e) => {
                self.warnings.push(format!("Could not delete group: {e}"));
                false
            }
        }
    }

    pub fn set_group_slot(&mut self, group: usize, slot: Option<&str>) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_group_slot(group, slot) {
            self.warnings.push(format!("Could not change slot: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn set_group_enabled(&mut self, group: usize, enabled: bool) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_group_enabled(group, enabled) {
            self.warnings.push(format!("Could not change skill group: {e}"));
            return;
        }
        self.reread_skills();
    }

    /// Put an item in a slot, or empty it with an id of 0.
    pub fn equip_item(&mut self, slot: &str, item_id: i64) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.equip_item(slot, item_id) {
            self.warnings.push(format!("Could not equip item: {e}"));
            return;
        }
        self.reread_items();
        // A jewel socket is a tree node. Socketing a cluster jewel grows the
        // tree by a whole subgraph, and unsocketing removes one, so the tree
        // has to be re-read or the canvas keeps drawing the old shape.
        if slot.starts_with("Jewel ") {
            self.reload_tree();
        }
    }

    pub fn set_flask_active(&mut self, slot: &str, active: bool) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_flask_active(slot, active) {
            self.warnings.push(format!("Could not toggle flask: {e}"));
            return;
        }
        self.reread_items();
    }

    /// Add an item from pasted text. Returns an error message to show, or
    /// `None` when it went in.
    pub fn add_item_from_text(&mut self, text: &str) -> Option<String> {
        let Some(engine) = &self.engine else { return Some("No build loaded".into()) };
        match engine.add_item_from_text(text) {
            Ok(Some(_)) => {
                self.reread_items();
                None
            }
            Ok(None) => Some("PoB could not read that as an item. Paste the full item text, starting with its rarity line.".into()),
            Err(e) => Some(format!("Could not add item: {e}")),
        }
    }

    pub fn weapon_set_second(&self) -> bool {
        self.engine.as_ref().and_then(|e| e.weapon_set_second().ok()).unwrap_or(false)
    }

    pub fn set_weapon_set(&mut self, second: bool) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_weapon_set(second) {
            self.warnings.push(format!("Could not switch weapon set: {e}"));
            return;
        }
        self.reread_items();
        self.reread_skills();
    }

    pub fn unique_catalogue(&self) -> Vec<pob_engine::items::DbItem> {
        self.engine.as_ref().and_then(|e| e.unique_catalogue().ok()).unwrap_or_default()
    }

    pub fn unique_raw(&self, name: &str) -> Option<String> {
        self.engine.as_ref().and_then(|e| e.unique_raw(name).ok()).flatten()
    }

    pub fn add_unique(&mut self, name: &str, equip: bool) -> Option<i64> {
        let id = self.engine.as_ref()?.add_unique(name, equip).ok()?;
        self.reread_items();
        id
    }

    pub fn craft_bases(&self) -> Vec<pob_engine::items::CraftBase> {
        self.engine.as_ref().and_then(|e| e.craft_bases().ok()).unwrap_or_default()
    }

    pub fn craft_item(&mut self, item_type: &str, base: &str, rarity: &str, title: &str) -> Option<i64> {
        let id = self.engine.as_ref()?.craft_item(item_type, base, rarity, title).ok()?;
        self.reread_items();
        id
    }

    pub fn parse_item_text(&self, raw: &str) -> Option<pob_engine::items::Item> {
        self.engine.as_ref().and_then(|e| e.parse_item_text(raw).ok()).flatten()
    }

    pub fn item_primary_slot(&self, text: &str) -> Option<String> {
        self.engine.as_ref().and_then(|e| e.item_primary_slot(text).ok()).flatten()
    }

    pub fn item_raw(&self, item_id: i64) -> Option<String> {
        self.engine.as_ref().and_then(|e| e.item_raw(item_id).ok()).flatten()
    }

    /// Returns an error to show, or `None`.
    pub fn replace_item_from_text(&mut self, item_id: i64, text: &str) -> Option<String> {
        let Some(engine) = &self.engine else { return Some("No build loaded".into()) };
        match engine.replace_item_from_text(item_id, text) {
            Ok(true) => {
                self.reread_items();
                None
            }
            Ok(false) => Some("PoB could not read that as an item. Keep the rarity and base lines.".into()),
            Err(e) => Some(format!("Could not save item: {e}")),
        }
    }

    pub fn delete_item(&mut self, item_id: i64) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.delete_item(item_id) {
            self.warnings.push(format!("Could not delete item: {e}"));
            return;
        }
        self.reread_items();
    }

    /// What equipping an item in a slot would do. `None` when the engine is
    /// unavailable or the pairing makes no sense.
    pub fn item_preview(
        &self,
        slot: &str,
        item_id: i64,
    ) -> Option<pob_engine::items::ItemPreview> {
        self.engine
            .as_ref()
            .and_then(|e| e.item_preview(slot, item_id).ok())
            .flatten()
    }

    pub fn item_preview_text(&self, slot: &str, text: &str) -> Option<pob_engine::items::ItemPreview> {
        self.engine.as_ref().and_then(|e| e.item_preview_text(slot, text).ok()).flatten()
    }

    /// The build's loadouts, by name.
    pub fn loadouts(&self) -> Vec<String> {
        self.engine
            .as_ref()
            .and_then(|e| e.loadouts().ok())
            .unwrap_or_default()
    }

    /// Which loadout is live, if the active sets agree on one.
    ///
    /// A build can sit between loadouts — active tree from one, gear from
    /// another — after switching a single set by hand. That is a real state
    /// and PoB allows it, so this reports None rather than picking whichever
    /// set happens to be first.
    pub fn active_loadout(&self) -> Option<String> {
        let engine = self.engine.as_ref()?;
        let tree = engine.active_spec_title().ok()?;
        for kind in [
            pob_engine::items::SetKind::Item,
            pob_engine::items::SetKind::Skill,
            pob_engine::items::SetKind::Config,
        ] {
            let list = engine.sets(kind).ok()?;
            // A kind holding one set belongs to every loadout, so it cannot
            // disagree — the same rule the matching itself uses.
            if list.sets.len() <= 1 {
                continue;
            }
            if list.active_title() != Some(tree.as_str()) {
                return None;
            }
        }
        Some(tree)
    }

    /// Switch every set to the one belonging to this loadout.
    pub fn set_active_loadout(&mut self, name: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_active_loadout(name) {
            self.warnings.push(format!("Could not switch loadout: {e}"));
            return;
        }
        self.reread_everything();
    }

    /// Create a loadout: one set of each kind, all sharing a name.
    pub fn new_loadout(&mut self, name: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.new_loadout(name) {
            self.warnings.push(format!("Could not create loadout: {e}"));
        }
    }

    /// Re-read every part of the build. A loadout switch changes the tree as
    /// well as the sets, which `reread_all` does not cover.
    fn reread_everything(&mut self) {
        let Some(engine) = &self.engine else { return };
        if let Ok(info) = engine.info() {
            self.info = info;
        }
        if let Ok(tree) = engine.tree() {
            self.tree = Arc::new(tree);
        }
        self.reread_all();
    }

    /// The sets of one kind, and which is active.
    pub fn sets(&self, kind: pob_engine::items::SetKind) -> pob_engine::items::SetList {
        self.engine
            .as_ref()
            .and_then(|e| e.sets(kind).ok())
            .unwrap_or_default()
    }

    /// Switch which set of a kind is live. Every set change rewrites what is
    /// equipped or socketed, so the whole build is re-read.
    pub fn set_active_set(&mut self, kind: pob_engine::items::SetKind, id: i64) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_active_set(kind, id) {
            self.warnings.push(format!("Could not switch set: {e}"));
            return;
        }
        self.reread_all();
    }

    pub fn new_set(&mut self, kind: pob_engine::items::SetKind, title: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.new_set(kind, title) {
            self.warnings.push(format!("Could not create set: {e}"));
        }
    }

    pub fn rename_set(
        &mut self,
        kind: pob_engine::items::SetKind,
        id: i64,
        title: &str,
    ) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.rename_set(kind, id, title) {
            self.warnings.push(format!("Could not rename set: {e}"));
        }
    }

    pub fn delete_set(&mut self, kind: pob_engine::items::SetKind, id: i64) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.delete_set(kind, id) {
            // Refusing to delete the last set of a kind is expected, not a
            // failure, so it is reported the same way as any other reason.
            self.warnings.push(format!("Could not delete set: {e}"));
            return;
        }
        self.reread_all();
    }

    /// Re-read everything a set switch can change: gear, skills and config
    /// all move at once, and reading only one leaves the tab showing a build
    /// that is half old.
    fn reread_all(&mut self) {
        let Some(engine) = &self.engine else { return };
        if let Ok(equipment) = engine.equipment() {
            self.equipment = equipment;
        }
        if let Ok(groups) = engine.socket_groups() {
            self.socket_groups = groups;
        }
        if let Ok(main) = engine.main_socket_group() {
            self.main_socket_group = main;
        }
        if let Ok(options) = engine.config_options() {
            self.config_options = options;
        }
        self.refresh();
    }

    /// The build as PoB's own XML, for saving or sharing.
    pub fn build_xml(&self) -> Option<String> {
        self.engine.as_ref()?.build_xml().ok()
    }

    /// The items this build owns.
    pub fn item_list(&self) -> Vec<pob_engine::items::ItemEntry> {
        self.engine
            .as_ref()
            .and_then(|e| e.item_list().ok())
            .unwrap_or_default()
    }

    /// What a slot will accept, per PoB's own validity rules.
    pub fn slot_candidates(&self, slot: &str) -> Vec<pob_engine::items::ItemEntry> {
        self.engine
            .as_ref()
            .and_then(|e| e.slot_candidates(slot).ok())
            .unwrap_or_default()
    }

    /// Re-read equipment and recalculate. Every item edit changes both what
    /// is worn and the numbers it produces.
    fn reread_items(&mut self) {
        let Some(engine) = &self.engine else { return };
        match engine.equipment() {
            Ok(equipment) => self.equipment = equipment,
            Err(e) => self.warnings.push(format!("Could not read items: {e}")),
        }
        self.refresh();
    }

    /// Every gem that can be socketed. Read once and cached by the caller —
    /// it is a property of the game data, not of the build.
    pub fn gem_catalogue(&self) -> Vec<pob_engine::skills::GemChoice> {
        self.engine
            .as_ref()
            .and_then(|e| e.gem_catalogue().ok())
            .unwrap_or_default()
    }

    /// Supports ranked by the DPS they add to a socket group, best first.
    /// A socketed gem's description text (1-based indices).
    pub fn gem_description(&self, group: usize, gem: usize) -> String {
        self.engine
            .as_ref()
            .and_then(|e| e.gem_description(group, gem).ok())
            .unwrap_or_default()
    }

    pub fn rank_gems_for_group(&self, group: usize) -> Vec<(String, f64, bool)> {
        self.engine
            .as_ref()
            .and_then(|e| e.rank_gems_for_group(group).ok())
            .unwrap_or_default()
    }

    /// Re-read the skills and recalculate. Shared by every gem edit: each one
    /// changes what is socketed, so each one changes the numbers.
    fn reread_skills(&mut self) {
        let Some(engine) = &self.engine else { return };
        match engine.socket_groups() {
            Ok(groups) => self.socket_groups = groups,
            Err(e) => self.warnings.push(format!("Could not read skills: {e}")),
        }
        match engine.main_socket_group() {
            Ok(main) => self.main_socket_group = main,
            Err(e) => self.warnings.push(format!("Could not read main skill: {e}")),
        }
        self.refresh();
    }

    /// PoB's sidebar stat list, refreshed with the build.
    pub fn sidebar_stats(&self) -> Vec<pob_engine::tree::SidebarRow> {
        self.engine
            .as_ref()
            .and_then(|e| e.sidebar_stats().ok())
            .unwrap_or_default()
    }

    /// Every stat key PoB's display table names, for reading a full
    /// comparison rather than a sampled one.
    pub fn display_stat_keys(&self) -> Vec<String> {
        self.engine
            .as_ref()
            .and_then(|e| e.display_stat_keys().ok())
            .unwrap_or_default()
    }

    /// A side-by-side stat comparison against another build's output, as
    /// PoB's Compare tab renders it.
    pub fn compare_rows(
        &self,
        other: &std::collections::HashMap<String, f64>,
    ) -> Vec<pob_engine::tree::CompareRow> {
        self.engine
            .as_ref()
            .and_then(|e| e.compare_rows(other).ok())
            .unwrap_or_default()
    }

    /// Turn tree compare mode on against a saved spec, or off.
    pub fn set_compare_spec(&mut self, index: Option<usize>) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_compare_spec(index) {
            self.warnings.push(format!("Could not set comparison: {e}"));
            return;
        }
        // The diff lives on the tree, so it has to be re-read.
        match engine.tree() {
            Ok(tree) => self.tree = Arc::new(tree),
            Err(e) => self.warnings.push(format!("Could not read tree: {e}")),
        }
    }

    /// Create a new spec, inheriting class and ascendancy.
    pub fn new_spec(&mut self, title: &str) -> Option<usize> {
        let index = self.engine.as_ref()?.new_spec(title).ok()?;
        self.reload_tree();
        Some(index)
    }

    /// Duplicate a spec, tree and jewels included.
    pub fn copy_spec(&mut self, index: usize, title: &str) -> Option<usize> {
        let new = self.engine.as_ref()?.copy_spec(index, title).ok()?;
        self.reload_tree();
        Some(new)
    }

    pub fn rename_spec(&mut self, index: usize, title: &str) {
        if let Some(engine) = &self.engine
            && let Err(e) = engine.rename_spec(index, title)
        {
            self.warnings.push(format!("Could not rename: {e}"));
        }
    }

    /// Delete a spec. A no-op at one spec, as PoB has it.
    pub fn delete_spec(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.delete_spec(index) {
            self.warnings.push(format!("Could not delete: {e}"));
            return;
        }
        self.reload_tree();
    }

    pub fn move_spec(&mut self, from: usize, to: usize) {
        if let Some(engine) = &self.engine
            && let Err(e) = engine.move_spec(from, to)
        {
            self.warnings.push(format!("Could not reorder: {e}"));
        }
    }

    /// Switch which spec is being edited.
    pub fn set_active_spec(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_active_spec(index) {
            self.warnings.push(format!("Could not switch tree: {e}"));
            return;
        }
        self.reload_tree();
    }

    /// Which saved spec is currently active, 1-based as PoB indexes them.
    pub fn active_spec(&self) -> usize {
        self.engine
            .as_ref()
            .and_then(|e| e.active_spec().ok())
            .unwrap_or(1)
    }

    /// Stats for one saved spec, without changing which one is active.
    pub fn stats_for_spec(
        &self,
        index: usize,
        keys: &[&str],
    ) -> Result<std::collections::HashMap<String, f64>, String> {
        let Some(engine) = &self.engine else {
            return Err("Calculation engine unavailable".into());
        };
        engine.stats_for_spec(index, keys).map_err(|e| e.to_string())
    }

    /// Node ids matching a search, using PoB's own matcher.
    pub fn search_nodes(&self, query: &str) -> Vec<u64> {
        self.engine
            .as_ref()
            .and_then(|e| e.search_nodes(query).ok())
            .unwrap_or_default()
    }

    /// What allocating or unallocating a node would do to the build.
    ///
    /// Two full recalculations, so never call this per frame — the caller is
    /// expected to cache it against the hovered node id.
    pub fn node_preview(&self, node_id: u64) -> Option<pob_engine::tree::NodePreview> {
        self.engine
            .as_ref()
            .and_then(|e| e.node_preview(node_id).ok())
            .flatten()
    }

    /// Allocate or deallocate a node, re-reading everything it affects.
    ///
    /// A single click can change dozens of nodes: allocating takes the whole
    /// path to the target, and deallocating drops everything that depended on
    /// it. The tree and every stat are re-read for that reason.
    pub fn toggle_node(&mut self, node_id: u64) {
        let Some(engine) = &self.engine else { return };
        match engine.toggle_node(node_id) {
            Ok(true) => self.reload_tree(),
            Ok(false) => {}
            Err(e) => self.warnings.push(format!("Could not change node: {e}")),
        }
    }

    /// A mastery node's selectable effects: `(effect id, label)`.
    pub fn mastery_effects(&self, node_id: u64) -> Vec<(u32, String)> {
        self.engine.as_ref().and_then(|e| e.mastery_effects(node_id).ok()).unwrap_or_default()
    }

    /// The effect currently chosen for a mastery, if any.
    pub fn mastery_selection(&self, node_id: u64) -> Option<u32> {
        self.engine.as_ref().and_then(|e| e.mastery_selection(node_id).ok()).flatten()
    }

    /// Choose an effect for a mastery, allocating it if needed.
    pub fn set_mastery_effect(&mut self, node_id: u64, effect_id: u32) {
        let Some(engine) = &self.engine else { return };
        match engine.set_mastery_effect(node_id, effect_id) {
            Ok(true) => self.reload_tree(),
            Ok(false) => {}
            Err(e) => self.warnings.push(format!("Could not set mastery: {e}")),
        }
    }

    /// Begin ranking unallocated nodes by the DPS they would add.
    ///
    /// Never run to completion in one call: PoB recalculates the whole build
    /// once per candidate node, which is roughly fifteen seconds of blocked
    /// UI thread — a spinning cursor and a window that stops painting.
    /// Undo or redo in one scope ("tree", "skills", "items", "config") and
    /// re-read what it moved. Returns (can_undo, can_redo).
    pub fn undo_redo(&mut self, scope: &str, redo: bool) -> (bool, bool) {
        let Some(engine) = &self.engine else { return (false, false) };
        let r = match engine.undo_redo(scope, redo) {
            Ok(r) => r,
            Err(e) => {
                self.warnings.push(format!("Could not {}: {e}", if redo { "redo" } else { "undo" }));
                return (false, false);
            }
        };
        match scope {
            "tree" => self.reload_tree(),
            "skills" => self.reread_skills(),
            "items" => self.reread_items(),
            "config" => self.refresh(),
            _ => {}
        }
        r
    }

    pub fn undo_state(&self, scope: &str) -> (bool, bool) {
        self.engine
            .as_ref()
            .and_then(|e| e.undo_state(scope).ok())
            .unwrap_or((false, false))
    }

    pub fn main_option(&self, name: &str) -> Option<String> {
        self.engine.as_ref().and_then(|e| e.main_option(name).ok()).flatten()
    }

    pub fn set_main_option(&mut self, name: &str, value: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_main_option(name, value) {
            self.warnings.push(format!("Could not set {name}: {e}"));
            return;
        }
        self.refresh();
    }

    pub fn skills_options(&self) -> Vec<(String, String)> {
        self.engine.as_ref().and_then(|e| e.skills_options().ok()).unwrap_or_default()
    }

    pub fn set_skills_option(&mut self, name: &str, value: &str) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_skills_option(name, value) {
            self.warnings.push(format!("Could not set {name}: {e}"));
            return;
        }
        self.reread_skills();
    }

    pub fn reset_tree(&mut self) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.reset_tree() {
            self.warnings.push(format!("Could not reset tree: {e}"));
            return;
        }
        self.reload_tree();
    }

    /// Begin ranking nodes by how much they would add to `stat`, one of
    /// PoB's stat keys (`TotalDPS`, `TotalEHP`, ...).
    pub fn node_power_start_for(&self, stat: &str) -> bool {
        self.engine
            .as_ref()
            .map(|e| e.node_power_start(Some(stat)).is_ok())
            .unwrap_or(false)
    }

    pub fn node_power_start(&self) -> bool {
        self.engine
            .as_ref()
            .map(|e| e.node_power_start(Some("TotalDPS")).is_ok())
            .unwrap_or(false)
    }

    /// Abandon a ranking in progress and restore PoB's clock.
    pub fn node_power_cancel(&self) {
        if let Some(e) = &self.engine {
            let _ = e.node_power_cancel();
        }
    }

    /// Advance the ranking by one slice. True once it has finished.
    pub fn node_power_step(&self) -> bool {
        self.engine
            .as_ref()
            .map(|e| e.node_power_step().unwrap_or(true))
            .unwrap_or(true)
    }

    /// Read the finished ranking, best first.
    pub fn node_power_stats(&self) -> Vec<pob_engine::skills::PowerStat> {
        self.engine
            .as_ref()
            .and_then(|e| e.node_power_stats().ok())
            .unwrap_or_default()
    }

    pub fn node_power_report(&self) -> Vec<pob_engine::skills::PowerReportRow> {
        self.engine
            .as_ref()
            .and_then(|e| e.node_power_report().ok())
            .unwrap_or_default()
    }

    pub fn node_power_progress(&self) -> u32 {
        self.engine
            .as_ref()
            .and_then(|e| e.node_power_progress().ok())
            .unwrap_or(0)
    }

    // Find Timeless Jewel. Every call drives PoB's own dialog; see
    // `pob_engine::timeless`.

    pub fn timeless_state(&self) -> pob_engine::timeless::TimelessState {
        self.engine
            .as_ref()
            .and_then(|e| e.timeless_state().ok())
            .unwrap_or_default()
    }

    pub fn set_timeless_state(&mut self, state: &pob_engine::timeless::TimelessState) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_timeless_state(state) {
            self.warnings.push(format!("Could not store jewel search: {e}"));
        }
    }

    pub fn timeless_options(&mut self) -> pob_engine::timeless::TimelessOptions {
        let Some(engine) = &self.engine else { return Default::default() };
        match engine.timeless_options() {
            Ok(o) => o,
            Err(e) => {
                self.warnings.push(format!("Could not read jewel options: {e}"));
                Default::default()
            }
        }
    }

    pub fn timeless_search(&mut self, protected: &[String]) -> pob_engine::timeless::TimelessSearch {
        let Some(engine) = &self.engine else { return Default::default() };
        match engine.timeless_search(protected) {
            Ok(s) => s,
            Err(e) => {
                self.warnings.push(format!("Jewel search failed: {e}"));
                Default::default()
            }
        }
    }

    pub fn timeless_results(&self) -> pob_engine::timeless::TimelessSearch {
        self.engine
            .as_ref()
            .and_then(|e| e.timeless_results().ok())
            .unwrap_or_default()
    }

    pub fn timeless_generate_fallback(&mut self) -> Vec<pob_engine::timeless::DesiredNode> {
        let Some(engine) = &self.engine else { return Vec::new() };
        match engine.timeless_generate_fallback() {
            Ok(rows) => rows,
            Err(e) => {
                self.warnings.push(format!("Could not generate weights: {e}"));
                Vec::new()
            }
        }
    }

    pub fn timeless_trade_url(
        &self,
        start: usize,
        scope: u8,
        league: &str,
        realm: u8,
        buyout: u8,
    ) -> Option<String> {
        self.engine
            .as_ref()
            .and_then(|e| e.timeless_trade_url(start, scope, league, realm, buyout).ok())
            .flatten()
    }

    /// PoB's weighted "Find best" trade search for one slot. `weights` is the
    /// sort-stat list (a `data.powerStatList` key and a multiplier each; PoB's
    /// default is `[("FullDPS", 1.0), ("TotalEHP", 0.5)]`), `buyout` the
    /// 0-based index into PoB's trade-status list. Returns the query JSON, or
    /// the generator's own message when nothing is worth searching for.
    pub fn trade_query(
        &self,
        slot: &str,
        weights: &[(String, f64)],
        buyout: u8,
    ) -> Result<String, String> {
        match self.engine.as_ref() {
            Some(e) => match e.trade_query(slot, weights, buyout) {
                Ok(inner) => inner,
                Err(err) => Err(err.to_string()),
            },
            None => Err("No build is loaded.".to_owned()),
        }
    }

    /// Add a found jewel to the build's items. Items and stats change.
    /// The socket label the jewel went into, "" if only added, None if not.
    pub fn timeless_add_to_build(&mut self, index: usize) -> Option<String> {
        let Some(engine) = &self.engine else { return None };
        match engine.timeless_add_to_build(index) {
            Ok(added) => {
                if added.is_some() {
                    self.reread_items();
                    // Socketing changes what the passives in radius are.
                    self.reload_tree();
                }
                added
            }
            Err(e) => {
                self.warnings.push(format!("Could not add the jewel: {e}"));
                None
            }
        }
    }

    /// Import a character's tree from the API's JSON, through PoB's own
    /// import. Returns an error message to show, or None on success.
    pub fn import_character_tree(&mut self, json: &str, delete_jewels: bool) -> Option<String> {
        let engine = self.engine.as_ref()?;
        // PoB's importer edits the build as it goes; an error halfway
        // would leave it half a character. Keep the build to put back.
        let before = engine.build_xml().ok();
        if let Err(e) = engine.import_character_tree(json, delete_jewels) {
            if let Some(xml) = before {
                let _ = engine.load_build_xml(&xml);
            }
            return Some(e.to_string());
        }
        // A new class, tree and jewels: everything downstream changes.
        self.reload_tree();
        self.reread_items();
        self.reread_skills();
        None
    }

    /// Import a character's items and skills, likewise.
    pub fn import_character_items(
        &mut self,
        json: &str,
        clear_items: bool,
        clear_skills: bool,
        ignore_weapon_swap: bool,
    ) -> Option<String> {
        let engine = self.engine.as_ref()?;
        let before = engine.build_xml().ok();
        if let Err(e) = engine.import_character_items(json, clear_items, clear_skills, ignore_weapon_swap) {
            if let Some(xml) = before {
                let _ = engine.load_build_xml(&xml);
            }
            return Some(e.to_string());
        }
        self.reread_items();
        self.reread_skills();
        // Jewels are tree nodes' contents.
        self.reload_tree();
        None
    }

    pub fn node_power_results(&self) -> Vec<(u64, f64)> {
        self.engine
            .as_ref()
            .and_then(|e| e.node_power_results().ok())
            .unwrap_or_default()
    }

    /// The current tree as a pathofexile.com link.
    pub fn export_tree_link(&self) -> Option<String> {
        self.engine
            .as_ref()
            .and_then(|e| e.export_tree_link().ok())
            .filter(|l| !l.is_empty())
    }

    /// Replace the tree from a link. Returns PoB's own error text on failure,
    /// which distinguishes an unrecognised format from a version mismatch.
    pub fn import_tree_link(&mut self, link: &str) -> Result<(), String> {
        let Some(engine) = &self.engine else {
            return Err("Calculation engine unavailable".into());
        };
        engine.import_tree_link(link).map_err(|e| e.to_string())?;
        self.reload_tree();
        Ok(())
    }

    /// Tree versions PoB supports: `(id, display)`.
    pub fn tree_versions(&self) -> Vec<(String, String)> {
        self.engine
            .as_ref()
            .and_then(|e| e.tree_versions().ok())
            .unwrap_or_default()
    }

    /// Convert to another tree version. Lossy: nodes that no longer exist are
    /// dropped, so callers are expected to have warned first.
    pub fn convert_tree_version(&mut self, version: &str) -> Result<(), String> {
        let Some(engine) = &self.engine else {
            return Err("Calculation engine unavailable".into());
        };
        engine.convert_tree_version(version).map_err(|e| e.to_string())?;
        self.reload_tree();
        Ok(())
    }

    /// Re-read the tree and everything derived from it.
    fn reload_tree(&mut self) {
        let Some(engine) = &self.engine else { return };
        match engine.tree() {
            Ok(tree) => self.tree = Arc::new(tree),
            Err(e) => self.warnings.push(format!("Could not read tree: {e}")),
        }
        match engine.info() {
            Ok(info) => self.info = info,
            Err(e) => self.warnings.push(format!("Could not read build info: {e}")),
        }
        self.refresh();
    }

    /// Re-read every stat the UI displays.
    pub fn refresh(&mut self) {
        let Some(engine) = &self.engine else { return };

        // PoB recomputes its warnings on each calculation, so they are read
        // here rather than only at load: allocating a node can make a build
        // unusable, and the warning has to appear when it does.
        self.build_warnings = engine.build_warnings().unwrap_or_default();

        let keys: Vec<&str> = pob_engine::stats::GROUPS
            .iter()
            .flat_map(|g| g.stats.iter().map(|s| s.key))
            .collect();

        match engine.stats(&keys) {
            Ok(pairs) => {
                self.values = pairs
                    .into_iter()
                    .filter_map(|(k, v)| v.map(|v| (k, v)))
                    .collect();
            }
            Err(e) => self.warnings.push(format!("Could not read stats: {e}")),
        }
    }

    /// Apply a config change and recalculate everything that depends on it.
    ///
    /// A config toggle can move DPS, defences, and the calc breakdowns at
    /// once, so this refreshes the derived views rather than only the stat
    /// the user was looking at.
    pub fn calc_sections_for(&self, other_xml: &str) -> Vec<pob_engine::calcs::Section> {
        self.engine.as_ref().and_then(|e| e.calc_sections_for(other_xml).ok()).unwrap_or_default()
    }

    pub fn calc_breakdown(&self, section: usize, sub: usize, row: usize, cell: usize) -> Vec<pob_engine::calcs::BreakdownPart> {
        self.engine
            .as_ref()
            .and_then(|e| e.calc_breakdown(section, sub, row, cell).ok())
            .unwrap_or_default()
    }

    pub fn calcs_inputs(&self) -> pob_engine::calcs::CalcsInputs {
        self.engine.as_ref().and_then(|e| e.calcs_inputs().ok()).unwrap_or_default()
    }

    pub fn set_calcs_group(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_group(index);
        self.calcs_write("calc skill", r);
    }

    pub fn set_calcs_active_skill(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_active_skill(index);
        self.calcs_write("calc active skill", r);
    }

    pub fn set_calcs_part(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_part(index);
        self.calcs_write("calc part", r);
    }

    pub fn set_calcs_stages(&mut self, count: i64) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_stages(count);
        self.calcs_write("calc stages", r);
    }

    pub fn set_calcs_mines(&mut self, count: i64) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_mines(count);
        self.calcs_write("calc mines", r);
    }

    pub fn set_calcs_minion_skill(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_minion_skill(index);
        self.calcs_write("calc minion skill", r);
    }

    pub fn set_calcs_show_minion(&mut self, show: bool) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_show_minion(show);
        self.calcs_write("show minion", r);
    }

    pub fn set_calcs_minion(&mut self, value: &str, by_item_set: bool) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_minion(value, by_item_set);
        self.calcs_write("calc minion", r);
    }

    pub fn set_calcs_buff_mode(&mut self, mode: &str) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_calcs_buff_mode(mode);
        self.calcs_write("calculation mode", r);
    }

    /// Apply one Calcs input write and re-read the tab.
    fn calcs_write(&mut self, what: &str, r: Result<(), pob_engine::Error>) {
        if let Err(e) = r {
            self.warnings.push(format!("Could not change {what}: {e}"));
            return;
        }
        self.refresh();
    }

    pub fn custom_mods(&self) -> Vec<pob_engine::config::CustomModBlock> {
        self.engine.as_ref().and_then(|e| e.custom_mods().ok()).unwrap_or_default()
    }

    fn custom_mods_apply(&mut self, r: Result<(), pob_engine::Error>) {
        if let Err(e) = r {
            self.warnings.push(format!("Could not change custom modifiers: {e}"));
            return;
        }
        self.refresh();
    }

    pub fn add_custom_mod_block(&mut self) {
        let Some(engine) = &self.engine else { return };
        let r = engine.add_custom_mod_block();
        self.custom_mods_apply(r);
    }

    pub fn remove_custom_mod_block(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let r = engine.remove_custom_mod_block(index);
        self.custom_mods_apply(r);
    }

    pub fn set_custom_mod_block(&mut self, index: usize, title: &str, enabled: bool, text: &str) {
        let Some(engine) = &self.engine else { return };
        let r = engine.set_custom_mod_block(index, title, enabled, text);
        self.custom_mods_apply(r);
    }

    pub fn set_config(&mut self, var: &str, value: Value) {
        let Some(engine) = &self.engine else { return };

        let before = engine.stat("TotalDPS").ok().flatten();

        match engine.set_config(var, value.clone()) {
            // A prompt here is informational, not a failure: the write is
            // already applied, so surface the note but carry on and re-read.
            Ok(Some(note)) => self.warnings.push(note),
            Ok(None) => {}
            Err(e) => {
                self.warnings.push(format!("Could not set {var}: {e}"));
                return;
            }
        }

        // Verifying a write by reading numbers off a screenshot is guesswork;
        // this makes the before/after unambiguous when something looks wrong.
        if std::env::var_os("POBNG_TRACE_CONFIG").is_some() {
            let after = engine.stat("TotalDPS").ok().flatten();
            eprintln!(
                "config: {var} = {value:?} | TotalDPS {before:?} -> {after:?}{}",
                if before == after { "  (unchanged)" } else { "" }
            );
        }

        match engine.config_options() {
            Ok(options) => self.config_options = options,
            Err(e) => self.warnings.push(format!("Could not re-read config: {e}")),
        }
        match engine.calc_sections() {
            Ok(sections) => self.calc_sections = sections,
            Err(e) => self.warnings.push(format!("Could not re-read calcs: {e}")),
        }
        self.refresh();
    }

    /// Read another build's stats without disturbing the loaded one.
    pub fn stats_of(
        &self,
        xml: &str,
        keys: &[&str],
    ) -> Result<HashMap<String, f64>, String> {
        let engine = self.engine.as_ref().ok_or("engine unavailable")?;
        let pairs = engine.stats_for(xml, keys).map_err(|e| e.to_string())?;
        Ok(pairs
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .collect())
    }

    /// A stat's raw value, for comparison arithmetic.
    pub fn raw(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }

    /// Produce a shareable build code for the current build.
    pub fn export_code(&self) -> Result<String, String> {
        let engine = self.engine.as_ref().ok_or("engine unavailable")?;
        let xml = engine.build_xml().map_err(|e| e.to_string())?;
        if xml.trim().is_empty() {
            return Err("nothing to export".to_owned());
        }
        pob_import::encode(&xml).map_err(|e| e.to_string())
    }

    /// Replace the build's notes.
    ///
    /// Notes do not feed the calculations, so this deliberately skips the
    /// recalculation a config change triggers.
    pub fn set_notes(&mut self, text: String) {
        let Some(engine) = &self.engine else { return };
        if let Err(e) = engine.set_notes(&text) {
            self.warnings.push(format!("Could not save notes: {e}"));
            return;
        }
        self.notes = text;
    }

    /// Formatted value for a stat, or None when the engine did not produce it.
    ///
    /// A missing stat renders as a dash rather than a zero: zero is a
    /// meaningful damage number, and showing it for "not computed" would be a
    /// lie the user cannot distinguish from a real result.
    pub fn display(&self, spec: &StatSpec) -> Option<String> {
        let value = *self.values.get(spec.key)?;
        Some(spec.format.render(value))
    }

    /// Whether a stat belongs on screen at all for this build.
    ///
    /// Distinct from having no value: a stat the engine did not compute shows
    /// a dash, because "not calculated" is worth saying. A stat that is
    /// merely uninformative — a "total DPS inc. X" equal to Total DPS, which
    /// says nothing about X — should not occupy a row at all. Conflating the
    /// two turns a misleading number into a misleading dash.
    pub fn is_relevant(&self, spec: &StatSpec) -> bool {
        match spec.show_when {
            ShowWhen::Always => true,
            ShowWhen::DiffersFromTotalDps => {
                let Some(value) = self.values.get(spec.key).copied() else {
                    return false;
                };
                let total = self.values.get("TotalDPS").copied().unwrap_or(0.0);
                (value - total).abs() >= 0.01
            }
            ShowWhen::NonZero => self
                .values
                .get(spec.key)
                .is_some_and(|v| v.abs() >= 0.01),
        }
    }

    pub fn is_live(&self) -> bool {
        self.engine.is_some()
    }
}
