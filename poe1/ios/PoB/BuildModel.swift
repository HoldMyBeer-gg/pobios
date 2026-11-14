import SwiftUI

/// The build, as SwiftUI sees it.
///
/// Wraps the uniffi `Build` object and republishes what the views read.
/// Everything here must run on the main thread: PoB allows one engine per
/// process and it cannot leave the thread that created it, so the Rust side
/// returns empty rather than answering from anywhere else.
@MainActor
final class BuildModel: ObservableObject {
    @Published var info: BuildInfo
    @Published var equipment: [Slot] = []
    @Published var items: [Item] = []
    @Published var weaponSetSecond = false
    @Published var stats: [Stat] = []
    /// The same stats, grouped as PoB groups them.
    @Published var sections: [StatSection] = []
    @Published var warnings: [String] = []
    @Published var buildWarnings: [String] = []
    @Published var loadouts: [String] = []
    @Published var activeLoadout: String?
    @Published var itemSets: [NamedSet] = []
    @Published var skillSets: [NamedSet] = []
    @Published var skillGroups: [SkillGroup] = []
    @Published var mainSkill: MainSkillControls?
    @Published var calcSections: [CalcSection] = []
    @Published var calcsInputs: CalcsInputs?
    @Published var configOptions: [ConfigOption] = []
    @Published var configSets: [NamedSet] = []
    @Published var customMods: [CustomModBlock] = []
    @Published var notes = ""
    /// The Compare tab's state, kept while the build is open as PoB keeps
    /// its comparison; tab switches must not lose a comparison that took
    /// seconds to compute.
    @Published var compare = CompareState()
    /// Bumped once at the end of every `refresh()`. Views that need a
    /// value the model does not cache (an item preview, a breakdown, the
    /// tree link) compute it in `.task(id: model.refreshCount)` rather than
    /// in `body`, so a re-render during a long engine job does not queue
    /// behind that job and stall the spinner.
    @Published private(set) var refreshCount = 0
    /// PoB's node-power measures, a fixed list from its data files.
    @Published var powerStats: [PowerStat] = []
    /// PoB's per-build gem defaults, keyed by option name.
    @Published var skillsOptions: [String: String] = [:]

    // Tree
    @Published var treeSpecs: [TreeSpec] = []
    @Published var activeSpec: UInt32 = 1
    @Published var treeVersionId = ""
    @Published var treeVersions: [TreeVersion] = []
    /// Which spec the tree is being compared against. Kept here: the engine
    /// holds it but does not read it back.
    @Published var compareSpec: UInt32?
    @Published var nodePower = NodePowerState()
    /// Undo availability for the scope of the tab on screen.
    @Published var undoState = UndoState(canUndo: false, canRedo: false)
    /// Which undo history the top bar acts on; set by the root as tabs change.
    var undoScope: UndoScope? { didSet { readUndoState() } }
    /// The library file the build lives in. PoB stores a build's name in
    /// the filename, not the XML, so the name is the file's.
    @Published var currentURL: URL?
    var buildName: String { currentURL?.deletingPathExtension().lastPathComponent ?? "" }
    /// Set when the engine could not start, which leaves a usable shell with
    /// an explanation rather than a blank screen.
    @Published var engineError: String?
    /// The tree node whose tooltip is pinned, mirrored from the canvas.
    @Published var pinnedNode: UInt64?

    /// "Jewel <id>" when a node is a jewel socket that takes items.
    func socketSlot(for node: UInt64) -> String? { build.socketSlot(nodeId: node) }
    func nodeAllocated(_ node: UInt64) -> Bool { build.nodeAllocated(nodeId: node) }
    /// Allocate a node (no-op if it is), from off the canvas (a jewel picked
    /// in a sheet). A static tree pauses the Metal view, so nudge it to redraw
    /// or the newly allocated node and its connectors never repaint.
    func allocateNode(_ node: UInt64) { build.allocateNode(nodeId: node); refresh(); save(); EguiBridge.unpin() }

    private let build: Build

    /// The library file opened last, so a relaunch lands back in it.
    private static let lastPathKey = "lastBuildPath"
    /// Set to the path while the last-opened build is loading at launch,
    /// cleared once the engine is up. Still set on the next launch means
    /// that load never finished: the watchdog killed the app under it. Such
    /// a build is not reopened, or every launch would die the same way and
    /// the only way out would be deleting the app's data.
    private static let loadingKey = "lastBuildLoading"

    /// Boot the engine off the main thread and reopen the last build. The
    /// launch screen shows while this runs; PoB takes seconds to start.
    static func load() async -> BuildModel {
        // PoB's Lua and data ship in the bundle as a folder reference, so the
        // nesting its `require` paths depend on survives.
        let dataDir = Bundle.main.resourceURL?
            .appendingPathComponent("src").path ?? ""

        // Reopen whatever was open last. Nothing open is a real state now:
        // the home screen shows the library.
        var xml: String?
        var openPath: String?
        var stuckMessage: String?
        if let path = UserDefaults.standard.string(forKey: Self.lastPathKey),
           FileManager.default.fileExists(atPath: path) {
            if UserDefaults.standard.string(forKey: Self.loadingKey) == path {
                // Last launch never got past loading this one.
                let name = URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
                stuckMessage = "\(name) did not finish loading last time, so it was not reopened. Open it from the library to try again."
                UserDefaults.standard.removeObject(forKey: Self.lastPathKey)
            } else {
                xml = try? String(contentsOfFile: path, encoding: .utf8)
                if xml != nil { openPath = path }
            }
        }

        if let openPath {
            UserDefaults.standard.set(openPath, forKey: Self.loadingKey)
            UserDefaults.standard.synchronize()
        }
        let build = await Build.boot(dataDir: dataDir, buildXml: xml)
        UserDefaults.standard.removeObject(forKey: Self.loadingKey)
        return BuildModel(build: build, openPath: openPath, loadedXml: xml != nil, stuckMessage: stuckMessage)
    }

    private init(build: Build, openPath: String?, loadedXml: Bool, stuckMessage: String?) {
        self.build = build
        if let openPath { currentURL = URL(fileURLWithPath: openPath) }
        info = build.info()
        powerStats = build.nodePowerStats()
        if let stuckMessage { engineError = stuckMessage }
        // The last build failed to load: forget it, or every launch fails.
        if loadedXml, build.engineError() != nil {
            currentURL = nil
            UserDefaults.standard.removeObject(forKey: Self.lastPathKey)
        }
        applySavedOptions()
        refresh()
        seenGeneration = build.generation()
        // Allocating in the tree changes the build without any call from
        // here; the counter is how that reaches the sidebar and points.
        EguiBridge.frameDidDraw = { [weak self] in
            guard let self else { return }
            // The pinned node is how a tapped socket reaches SwiftUI.
            let pinned = EguiBridge.pinnedNode()
            if pinned != self.pinnedNode { self.pinnedNode = pinned }
            // A node tapped twice on the canvas. The engine lives on its
            // own thread now, so the canvas records the tap and it is
            // applied here, after the frame, then the canvas is woken to
            // draw the new tree.
            if let node = self.build.takePendingTap() {
                // Off the display link: the recalculation runs on the
                // engine's thread and the canvas keeps panning meanwhile.
                Task { @MainActor in
                    await self.build.toggleNodeAsync(nodeId: node)
                    self.seenGeneration = self.build.generation()
                    self.refresh()
                    self.save()
                    EguiBridge.unpin()
                }
                return
            }
            guard self.build.generation() != self.seenGeneration else { return }
            self.seenGeneration = self.build.generation()
            self.refresh()
            self.save()
        }
    }

    /// The bridge's change counter as of the last refresh.
    private var seenGeneration: UInt64 = 0
    /// Drives a node-power ranking in small steps. Not published: a tick
    /// must not republish the whole model.
    var nodePowerTimer: Timer?
    /// The tree set made for a compare overlay, to remove when done, and
    /// the set that was active before it, to return to.
    var compareOverlaySpec: UInt32?
    var compareOverlayReturn: UInt32?

    /// Re-read everything the views show. Called after any edit, because a
    /// change to one thing routinely moves another — equipping an item moves
    /// every stat.
    func refresh() {
        info = build.info()
        equipment = build.equipment()
        items = build.itemList()
        weaponSetSecond = build.weaponSetSecond()
        stats = build.sidebarStats()
        sections = build.sidebarSections()
        warnings = build.warnings()
        buildWarnings = build.buildWarnings()
        loadouts = build.loadouts()
        activeLoadout = build.activeLoadout()
        itemSets = build.sets(kind: .item)
        skillSets = build.sets(kind: .skill)
        skillGroups = build.skillGroups()
        mainSkill = build.mainSkillControls()
        calcSections = build.calcSections()
        calcsInputs = build.calcsInputs()
        configOptions = build.configOptions()
        configSets = build.sets(kind: .config)
        customMods = build.customMods()
        notes = build.notes()
        engineError = build.engineError()
        readUndoState()
        treeSpecs = build.treeSpecs()
        activeSpec = build.activeSpec()
        treeVersionId = build.treeVersion()
        treeVersions = build.treeVersions()
        skillsOptions = Dictionary(uniqueKeysWithValues: build.skillsOptions().map { ($0.name, $0.value) })
        refreshCount &+= 1
        // The canvas draws from the same engine; an edit here may change
        // what it shows (tooltips, overlays), so ask for a frame.
        EguiBridge.controller?.wake()
    }

    private func readUndoState() {
        undoState = undoScope.map { build.undoState(scope: $0) } ?? UndoState(canUndo: false, canRedo: false)
    }

    /// PoB keeps one undo history per tab; this acts on the current tab's.
    func undo() {
        guard let scope = undoScope else { return }
        undoState = build.undo(scope: scope)
        refresh(); save()
    }

    func redo() {
        guard let scope = undoScope else { return }
        undoState = build.redo(scope: scope)
        refresh(); save()
    }

    /// Change one field on a gem. Indices are 1-based, as PoB's own are.
    func setGem(
        _ group: Int,
        _ gem: Int,
        level: Int64? = nil,
        quality: Int64? = nil,
        enabled: Bool? = nil,
        count: Int64? = nil
    ) {
        build.setGem(
            group: UInt32(group),
            gem: UInt32(gem),
            level: level,
            quality: quality,
            enabled: enabled,
            count: count
        )
        refresh()
    }

    func setGroupEnabled(_ group: Int, _ enabled: Bool) {
        build.setGroupEnabled(group: UInt32(group), enabled: enabled)
        refresh()
    }

    func setMainActiveSkill(_ index: Int) { build.setMainActiveSkill(index: UInt32(index)); refresh(); save() }
    func setMainSkillPart(_ index: Int) { build.setMainSkillPart(index: UInt32(index)); refresh(); save() }
    func setMainSkillStages(_ n: Int64) { build.setMainSkillStages(count: n); refresh(); save() }
    func setMainSkillMines(_ n: Int64) { build.setMainSkillMines(count: n); refresh(); save() }
    func setMainSkillMinion(_ value: String, byItemSet: Bool) { build.setMainSkillMinion(value: value, byItemSet: byItemSet); refresh(); save() }
    func setMainSkillMinionSkill(_ index: Int) { build.setMainSkillMinionSkill(index: UInt32(index)); refresh(); save() }

    /// Add an empty socket group and return its 1-based index.
    @discardableResult
    func newSocketGroup() -> Int? {
        let i = build.newSocketGroup().map { Int($0) }
        refresh(); save()
        return i
    }

    /// Delete a socket group. False when PoB refuses (item-granted).
    @discardableResult
    func deleteSocketGroup(_ group: Int) -> Bool {
        let ok = build.deleteSocketGroup(group: UInt32(group))
        refresh(); save()
        return ok
    }

    func setGroupSlot(_ group: Int, _ slot: String?) {
        build.setGroupSlot(group: UInt32(group), slot: slot)
        refresh(); save()
    }

    // Skill sets, the same shape as item sets.
    func newSkillSet(_ title: String) { build.newSet(kind: .skill, title: title); refresh(); save() }
    func renameSkillSet(_ id: Int64, to title: String) { build.renameSet(kind: .skill, id: id, title: title); refresh(); save() }
    func deleteSkillSet(_ id: Int64) { build.deleteSet(kind: .skill, id: id); refresh(); save() }

    func setMainGroup(_ group: Int) {
        build.setMainGroup(group: UInt32(group))
        refresh()
        save()
    }

    func setGroupFullDps(_ group: Int, _ include: Bool) {
        build.setGroupFullDps(group: UInt32(group), include: include)
        refresh()
        save()
    }

    func removeGem(_ group: Int, _ gem: Int) {
        build.removeGem(group: UInt32(group), gem: UInt32(gem))
        refresh()
        save()
    }

    /// Supports ranked by the DPS each adds to a group, best first. A real
    /// calc per gem, so callers run it off the main actor and show a spinner.
    /// One full recalculation per support gem: seconds to tens of
    /// seconds, on the engine's thread while the sheet keeps its spinner.
    func rankSupports(for group: Int) async -> [RankedGem] {
        await build.rankSupportsForGroupAsync(group: UInt32(group))
    }

    /// A socketed gem's description (what it does), for tapping it open.
    func gemDescription(_ group: Int, _ gem: Int) -> String {
        build.gemDescription(group: UInt32(group), gem: UInt32(gem))
    }

    /// Replace a gem, or add one with `gem` one past the group's last.
    func setGemId(_ group: Int, _ gem: Int, _ id: String) {
        build.setGemId(group: UInt32(group), gem: UInt32(gem), gemId: id)
        refresh()
        save()
    }

    /// A mastery node's selectable effects. Empty for anything that is not a
    /// mastery with a choice still open.
    func masteryEffects(_ nodeId: UInt64) -> [MasteryEffect] { build.masteryEffects(nodeId: nodeId) }
    /// The effect currently chosen for a mastery, if any.
    func masterySelection(_ nodeId: UInt64) -> UInt32? { build.masterySelection(nodeId: nodeId) }
    /// Choose an effect for a mastery, allocating it if needed. Tree, info and
    /// stats all move, so refresh and save. The change is made off the canvas
    /// (in a sheet), and a static tree pauses the Metal view, so nudge it to
    /// redraw or the new connector state never repaints.
    func setMasteryEffect(_ nodeId: UInt64, _ effectId: UInt32) {
        build.setMasteryEffect(nodeId: nodeId, effectId: effectId)
        refresh()
        save()
        EguiBridge.unpin()
    }

    func switchSkillSet(_ id: Int64) {
        build.setActiveSet(kind: .skill, id: id)
        refresh()
        save()
    }

    /// PoB's full gem list. A few thousand rows, read once.
    lazy var gemCatalogue: [GemChoice] = build.gemCatalogue()

    func calcBreakdown(_ s: CalcSection, row: Int, cell: Int) -> [BreakdownPart] {
        build.calcBreakdown(sectionIndex: s.sectionIndex, subIndex: s.subIndex, row: UInt32(row), cell: UInt32(cell))
    }

    // The Calcs tab's own skill selection. Not saved: PoB keeps these as
    // view state of the tab.
    func setCalcsGroup(_ i: Int) { build.setCalcsGroup(index: UInt32(i)); refresh() }
    func setCalcsActiveSkill(_ i: Int) { build.setCalcsActiveSkill(index: UInt32(i)); refresh() }
    func setCalcsPart(_ i: Int) { build.setCalcsPart(index: UInt32(i)); refresh() }
    func setCalcsStages(_ n: Int64) { build.setCalcsStages(count: n); refresh() }
    func setCalcsMines(_ n: Int64) { build.setCalcsMines(count: n); refresh() }
    func setCalcsMinion(_ v: String, byItemSet: Bool) { build.setCalcsMinion(value: v, byItemSet: byItemSet); refresh() }
    func setCalcsMinionSkill(_ i: Int) { build.setCalcsMinionSkill(index: UInt32(i)); refresh() }
    func setCalcsBuffMode(_ m: String) { build.setCalcsBuffMode(mode: m); refresh() }
    func setCalcsShowMinion(_ on: Bool) { build.setCalcsShowMinion(show: on); refresh() }

    func setConfig(_ option: ConfigOption, value: String) {
        build.setConfig(var: option.var, value: value, kind: option.kind)
        refresh(); save()
    }

    func switchConfigSet(_ id: Int64) { build.setActiveSet(kind: .config, id: id); refresh(); save() }
    func newConfigSet(_ title: String) { build.newSet(kind: .config, title: title); refresh(); save() }
    func renameConfigSet(_ id: Int64, to title: String) { build.renameSet(kind: .config, id: id, title: title); refresh(); save() }
    func deleteConfigSet(_ id: Int64) { build.deleteSet(kind: .config, id: id); refresh(); save() }

    func addCustomModBlock() { build.addCustomModBlock(); refresh(); save() }
    func removeCustomModBlock(_ i: Int) { build.removeCustomModBlock(index: UInt32(i)); refresh(); save() }
    func setCustomModBlock(_ i: Int, title: String, enabled: Bool, text: String) {
        build.setCustomModBlock(index: UInt32(i), title: title, enabled: enabled, text: text)
        refresh(); save()
    }

    func itemPreview(text: String, in slot: String) -> ItemPreview? { build.itemPreviewText(slot: slot, text: text) }
    func uniqueRaw(_ name: String) -> String? { build.uniqueRaw(name: name) }
    func primarySlot(of text: String) -> String? { build.itemPrimarySlot(text: text) }
    func parseItem(_ text: String) -> ItemDetail? { build.parseItem(text: text) }
    /// The other build is loaded in place of this one and both are
    /// recalculated; the UI stays live for the seconds that takes.
    func compare(withXML xml: String) async -> CompareOutcome { await build.compareWithXmlAsync(xml: xml) }
    func compare(withSpec index: UInt32) async -> CompareOutcome { await build.compareWithSpecAsync(index: index) }

    func setNotes(_ text: String) {
        // Not a full refresh: notes do not move any number, and re-reading
        // the whole build on every keystroke would make typing crawl.
        build.setNotes(text: text)
    }

    /// Called with the open build's file and XML just before an import
    /// replaces its contents; the root points this at the library's
    /// snapshots. Nothing is taken for a build that has no file yet.
    var beforeImport: ((URL, String) -> Void)?

    private func snapshotBeforeImport() {
        guard let url = currentURL, let xml = build.exportXml() else { return }
        beforeImport?(url, xml)
    }

    /// Import a build. Returns an error to show, or nil on success. The
    /// load runs on the engine's thread; the caller shows its own spinner.
    func importPaste(_ text: String) async -> String? {
        snapshotBeforeImport()
        let error = await build.importPasteAsync(text: text)
        refresh()
        if error == nil { save() }
        return error
    }

    /// Write the build to its library file.
    ///
    /// Called after any edit. The write is a few tens of kilobytes and the
    /// alternative is losing work to a backgrounded app being killed, which
    /// iOS does without warning.
    func save() {
        guard let url = currentURL, let xml = build.exportXml() else { return }
        try? xml.write(to: url, atomically: true, encoding: .utf8)
    }

    /// Load a library file into the engine and make it the current build.
    /// Make a library file the current build. When the caller already has
    /// the XML (a fresh import), pass it so open() does not re-read a file
    /// that may not be flushed to disk yet.
    func open(_ url: URL, xml: String? = nil) async {
        let text = xml ?? (try? String(contentsOf: url, encoding: .utf8))
        guard let text else { return }
        // Already loaded into the engine? Skip the re-parse; just adopt the file.
        // A file that does not load is not adopted: PoB closes the build on
        // a failed load, and adopting the path anyway left an empty screen
        // under the corrupt file's name, and reopened it on every launch.
        if xml == nil, let err = await build.importPasteAsync(text: text) {
            engineError = "Could not open \(url.deletingPathExtension().lastPathComponent): \(err)"
            return
        }
        currentURL = url
        UserDefaults.standard.set(url.path, forKey: Self.lastPathKey)
        UserDefaults.standard.synchronize()
        compare = CompareState()
        removeLeftoverCompareSets()
        refresh()
    }

    /// A killed app can leave the Compare overlay's temporary tree set in
    /// the file. Nothing else names a set "Compare: ", so they are safe to drop.
    private func removeLeftoverCompareSets() {
        let specs = build.treeSpecs()
        guard specs.count > 1 else { return }
        for s in specs.reversed() where s.title.hasPrefix("Compare: ") && specs.count > 1 {
            if build.activeSpec() == s.index, let first = specs.first(where: { !$0.title.hasPrefix("Compare: ") }) {
                build.setActiveSpec(index: first.index)
            }
            build.deleteSpec(index: s.index)
        }
    }

    /// Leave the build: write it, and forget it as the one to reopen.
    func close() {
        clearTreeOverlay()
        compare = CompareState()
        save()
        currentURL = nil
        UserDefaults.standard.removeObject(forKey: Self.lastPathKey)
    }

    /// The build as XML, for putting into a new library file.
    func currentXML() -> String? { build.exportXml() }

    /// The build as XML, for exporting to a file.
    func exportXml() -> String? {
        build.exportXml()
    }

    /// The build as a PoB share code, for pobb.in, Discord, or the desktop.
    func exportCode() -> String? { build.exportCode() }

    /// Import a character's tree from the account API; an error to show, or nil.
    func importCharacterTree(_ json: String, deleteJewels: Bool) -> String? {
        snapshotBeforeImport()
        let err = build.importCharacterTree(json: json, deleteJewels: deleteJewels)
        refresh()
        if err == nil { save() }
        return err
    }

    /// Import a character's items and skills, likewise.
    func importCharacterItems(_ json: String, clearItems: Bool, clearSkills: Bool, ignoreWeaponSwap: Bool) -> String? {
        snapshotBeforeImport()
        let err = build.importCharacterItems(json: json, clearItems: clearItems, clearSkills: clearSkills, ignoreWeaponSwap: ignoreWeaponSwap)
        refresh()
        if err == nil { save() }
        return err
    }

    func searchNodes(_ query: String) -> [UInt64] {
        build.searchNodes(query: query)
    }

    func candidates(for slot: String) -> [Item] {
        build.slotCandidates(slot: slot)
    }

    func equip(_ itemId: Int64, in slot: String) {
        build.equipItem(slot: slot, itemId: itemId)
        refresh()
        save()
    }

    /// PoB's classes, read once.
    lazy var classes: [ClassChoice] = build.classes()

    func classChangeResetsTree(_ id: UInt8) -> Bool { build.classChangeResetsTree(classId: id) }
    func setClass(_ id: UInt8) { build.setClass(classId: id); refresh(); save() }
    func setAscendancy(_ id: UInt8) { build.setAscendancy(ascendancyId: id); refresh(); save() }
    func setLevel(_ n: Int64) { build.setLevel(level: max(1, min(100, n))); refresh(); save() }
    func setLevelAuto() { build.setLevelAuto(); refresh(); save() }

    // MARK: PoB options

    /// The PoB application options the Settings screen edits, and their
    /// defaults. Kept in UserDefaults too and re-applied on launch, since
    /// the engine's own Settings.xml lives in its sandbox and a fresh
    /// engine starts from PoB's defaults.
    static let optionKeys: [(String, String)] = [
        ("showThousandsSeparators", "true"), ("showWarnings", "true"), ("slotOnlyTooltips", "true"),
        ("defaultGemQuality", "0"), ("defaultCharLevel", "1"), ("defaultItemAffixQuality", "0.5"),
        ("edgeSearchHighlight", "true"),
    ]

    func option(_ name: String) -> String {
        build.mainOption(name: name) ?? Self.optionKeys.first { $0.0 == name }?.1 ?? ""
    }

    func setOption(_ name: String, _ value: String) {
        build.setMainOption(name: name, value: value)
        UserDefaults.standard.set(value, forKey: "pob.option.\(name)")
        refresh()
    }

    /// Re-apply saved options to a fresh engine.
    func applySavedOptions() {
        for (name, _) in Self.optionKeys {
            if let v = UserDefaults.standard.string(forKey: "pob.option.\(name)") { build.setMainOption(name: name, value: v) }
        }
    }

    func setSkillsOption(_ name: String, _ value: String) {
        build.setSkillsOption(name: name, value: value)
        refresh(); save()
    }

    func setWeaponSet(second: Bool) {
        build.setWeaponSet(second: second)
        refresh(); save()
    }

    /// PoB's unique database, read once (a couple of thousand rows).
    lazy var uniqueCatalogue: [DbItem] = build.uniqueCatalogue()

    func uniqueDetail(_ name: String) -> ItemDetail? { build.uniqueDetail(name: name) }

    @discardableResult
    func addUnique(_ name: String, equip: Bool) -> Int64? {
        let id = build.addUnique(name: name, equip: equip)
        refresh(); save()
        return id
    }

    lazy var craftBases: [CraftBase] = build.craftBases()

    @discardableResult
    func craftItem(type: String, base: String, rarity: String, title: String) -> Int64? {
        let id = build.craftItem(itemType: type, base: base, rarity: rarity, title: title)
        refresh(); save()
        return id
    }

    func itemRaw(_ id: Int64) -> String? { build.itemRaw(itemId: id) }

    /// Returns an error to show, or nil.
    func replaceItem(_ id: Int64, text: String) -> String? {
        let error = build.replaceItemFromText(itemId: id, text: text)
        refresh()
        if error == nil { save() }
        return error
    }

    func itemDetail(in slot: String) -> ItemDetail? {
        build.itemDetail(slot: slot)
    }

    func itemPreview(_ itemId: Int64, in slot: String) -> ItemPreview? {
        build.itemPreview(slot: slot, itemId: itemId)
    }

    /// Add an item from pasted text. Returns an error to show, or nil.
    func addItem(text: String) -> String? {
        let error = build.addItemFromText(text: text)
        refresh()
        if error == nil { save() }
        return error
    }

    func setFlask(_ slot: String, active: Bool) {
        build.setFlaskActive(slot: slot, active: active)
        refresh()
        save()
    }

    func delete(item id: Int64) {
        build.deleteItem(itemId: id)
        refresh()
    }

    func switchLoadout(_ name: String) {
        build.setActiveLoadout(name: name)
        refresh()
        save()
    }

    func newLoadout(_ name: String) {
        build.newLoadout(name: name)
        build.setActiveLoadout(name: name)
        refresh()
    }

    func switchItemSet(_ id: Int64) {
        build.setActiveSet(kind: .item, id: id)
        refresh()
    }

    func newItemSet(_ title: String) {
        build.newSet(kind: .item, title: title)
        refresh()
    }

    func renameItemSet(_ id: Int64, to title: String) {
        build.renameSet(kind: .item, id: id, title: title)
        refresh()
    }

    func deleteItemSet(_ id: Int64) {
        build.deleteSet(kind: .item, id: id)
        refresh()
    }
}

// MARK: - Tree

/// Seconds between node-power ticks, and how much of each tick is spent
/// stepping. The engine runs PoB's ranking on a clock that makes it yield
/// every ~6ms of real work; steps are packed into the budget, and the rest
/// of the tick is left for drawing and touch. About 70% duty: a ranking
/// that takes PoB 20s flat out takes ~30s here without freezing anything.
let NODE_POWER_TICK: TimeInterval = 1.0 / 30.0
let NODE_POWER_BUDGET: TimeInterval = 0.023

/// A ranking in progress or shown.
struct NodePowerState {
    /// Which of PoB's measures; nil when off.
    var metric: PowerStat?
    var running = false
    /// Percent complete while running, from PoB's own callback.
    var progress: UInt32 = 0
    /// PoB's Power Report for the finished ranking, best first.
    var report: [PowerReportRow] = []
}

extension BuildModel {
    var activeSpecTitle: String {
        treeSpecs.first { $0.index == activeSpec }?.title ?? "Default"
    }

    var treeVersionLabel: String {
        treeVersions.first { $0.id == treeVersionId }?.label
            ?? treeVersionId.replacingOccurrences(of: "_", with: ".")
    }

    func switchSpec(_ index: UInt32) {
        build.setActiveSpec(index: index)
        compareSpec = nil
        refresh(); save()
    }

    func newSpec(_ title: String) {
        if let i = build.newSpec(title: title) { build.setActiveSpec(index: i) }
        refresh(); save()
    }

    func copySpec(_ index: UInt32, as title: String) {
        if let i = build.copySpec(index: index, title: title) { build.setActiveSpec(index: i) }
        refresh(); save()
    }

    func renameSpec(_ index: UInt32, to title: String) {
        build.renameSpec(index: index, title: title)
        refresh(); save()
    }

    func deleteSpec(_ index: UInt32) {
        build.deleteSpec(index: index)
        compareSpec = nil
        refresh(); save()
    }

    /// Returns an error to show, or nil.
    @discardableResult
    func convertTree(to version: String) -> String? {
        let error = build.convertTreeVersion(version: version)
        refresh()
        if error == nil { save() }
        return error
    }

    func setCompareSpec(_ index: UInt32?) {
        build.setCompareSpec(index: index)
        compareSpec = index
        refresh()
    }

    func exportTreeLink() -> String? { build.exportTreeLink() }

    /// Returns an error to show, or nil.
    func importTreeLink(_ link: String) -> String? {
        let error = build.importTreeLink(link: link.trimmingCharacters(in: .whitespacesAndNewlines))
        refresh()
        if error == nil { save() }
        return error
    }

    /// Show another build's tree as PoB's compare overlay: its link goes
    /// into a temporary tree set, and the canvas diffs against that.
    /// Returns an error to show, or nil.
    func overlayTree(link: String, title: String) -> String? {
        clearTreeOverlay()
        // Read from the engine, not the published copy, so a stale value
        // cannot send the build back to the wrong set.
        let mine = build.activeSpec()
        compareOverlayReturn = mine
        guard let temp = build.newSpec(title: "Compare: \(title)") else { return "Could not make a tree set." }
        build.setActiveSpec(index: temp)
        if let err = build.importTreeLink(link: link) {
            build.setActiveSpec(index: mine)
            build.deleteSpec(index: temp)
            refresh()
            return err
        }
        build.setActiveSpec(index: mine)
        build.setCompareSpec(index: temp)
        compareSpec = temp
        compareOverlaySpec = temp
        refresh()
        return nil
    }

    func clearTreeOverlay() {
        guard let temp = compareOverlaySpec else { return }
        build.setCompareSpec(index: nil)
        compareSpec = nil
        if let back = compareOverlayReturn, build.activeSpec() != back { build.setActiveSpec(index: back) }
        build.deleteSpec(index: temp)
        compareOverlaySpec = nil
        compareOverlayReturn = nil
        refresh()
    }

    /// PoB's Copy tree: the other build's tree as a new set here; with
    /// `use`, switch to it.
    func copyTree(link: String, title: String, use: Bool) -> String? {
        let mine = activeSpec
        guard let i = build.newSpec(title: title) else { return "Could not make a tree set." }
        build.setActiveSpec(index: i)
        if let err = build.importTreeLink(link: link) {
            build.setActiveSpec(index: mine)
            build.deleteSpec(index: i)
            refresh()
            return err
        }
        if !use { build.setActiveSpec(index: mine) }
        refresh(); save()
        return nil
    }

    func calcSections(for xml: String) async -> [CalcSection] { await build.calcSectionsForAsync(xml: xml) }

    func resetTree() {
        build.resetTree()
        refresh(); save()
    }

    // MARK: Find Timeless Jewel. Each call drives PoB's own dialog headless.

    func timelessState() -> TimelessState? { build.timelessState() }
    /// Stored on the build, where PoB saves it, so the search survives a relaunch.
    func setTimelessState(_ s: TimelessState) { build.setTimelessState(state: s); save() }
    func timelessOptions() -> TimelessOptions? { build.timelessOptions() }
    func timelessSearch(protected: [String]) async -> TimelessSearch { await build.timelessSearchAsync(protected: protected) }
    func timelessResults() -> TimelessSearch { build.timelessResults() }
    func timelessGenerateFallback() async -> [DesiredNode] { await build.timelessGenerateFallbackAsync() }
    /// `scope`: 0 the selected seed only, 1 the ten or so from it, 2 up to 180.
    func timelessTradeURL(start: Int, scope: Int, league: String, realm: Int, buyout: Int) -> String? {
        build.timelessTradeUrl(start: UInt32(start), scope: UInt8(scope), league: league, realm: UInt8(realm), buyout: UInt8(buyout))
    }
    /// PoB's weighted "Find best" trade search for one slot. Returns the
    /// pathofexile.com/trade URL, built by PoB itself (`buildUrl` plus the
    /// query gzipped into the path), or PoB's message when there is nothing
    /// worth searching for. `weights` is the sort-stat list; empty uses PoB's
    /// default (Full DPS + Effective Hit Pool). `buyout` is the 0-based index
    /// into PoB's trade-status list.
    func tradeQuery(slot: String, weights: [StatWeight], buyout: Int, league: String, realm: Int)
        async -> (url: String?, error: String?)
    {
        let r = await build.tradeQueryAsync(slot: slot, weights: weights, buyout: UInt8(buyout))
        guard let json = r.query else { return (nil, r.error) }
        guard let url = build.tradeSearchUrl(queryJson: json, league: league, realm: UInt8(min(max(realm, 0), 2))) else {
            return (nil, "Could not build the trade link.")
        }
        return (url, nil)
    }

    /// Adds the found jewel to the item list; it is not socketed.
    /// The socket the jewel went into, "" if only added, nil if nothing happened.
    func timelessAddToBuild(_ index: Int) -> String? {
        let socket = build.timelessAddToBuild(index: UInt32(index))
        if socket != nil { refresh(); save() }
        return socket
    }

    /// Rank nodes for a metric, or clear the ranking with nil.
    ///
    /// PoB recalculates the whole build once per candidate node and there
    /// are thousands, so this is stepped on a time budget from a timer and
    /// the UI keeps drawing between steps. Never run to completion in one
    /// call: on desktop pobng that hung the app.
    /// PoB's node-power measures, read once.

    func setNodePower(_ metric: PowerStat?) {
        if nodePowerTimer != nil {
            // A ranking was under way: PoB's builder is parked mid-way and
            // its clock is running fast. Put both back, or the fast clock
            // outlives the ranking.
            build.nodePowerCancel()
        }
        nodePowerTimer?.invalidate()
        nodePowerTimer = nil
        nodePower = NodePowerState(metric: metric, running: false)
        guard let metric else {
            EguiBridge.setPower([], [])
            return
        }
        guard build.nodePowerStart(stat: metric.stat) else { return }
        nodePower.running = true
        // Steps until the tick's budget is spent, never past it. Each step
        // runs PoB's coroutine to its next yield (~6ms); the budget is what
        // keeps the frame loop alive.
        nodePowerTimer = Timer.scheduledTimer(withTimeInterval: NODE_POWER_TICK, repeats: true) { [weak self] t in
            guard let self else { t.invalidate(); return }
            let deadline = Date().addingTimeInterval(NODE_POWER_BUDGET)
            var done = false
            repeat { done = self.build.nodePowerStep() } while !done && Date() < deadline
            let p = self.build.nodePowerProgress()
            if p != self.nodePower.progress {
                self.nodePower.progress = p
                // PoB paints nodes as they are scored; showing the partial
                // ranking every few percent gives the same live picture.
                if p % 4 == 0 { self.pushNodePowerToCanvas() }
            }
            if done {
                t.invalidate()
                self.nodePowerTimer = nil
                self.nodePower.running = false
                self.showNodePower()
            }
        }
    }

    private func showNodePower() {
        nodePower.report = build.nodePowerReport()
        pushNodePowerToCanvas()
    }

    /// Tint the canvas by whatever the ranking has scored so far.
    private func pushNodePowerToCanvas() {
        let results = build.nodePowerResults()
        let values = results.map(\.value)
        // Normalise to 0..1 across the positive range; PoB colours by rank
        // against the best node, and negatives read as zero.
        let top = values.max() ?? 0
        guard top > 0 else { EguiBridge.setPower([], []); return }
        EguiBridge.setPower(results.map(\.id), results.map { Float(max(0, $0.value) / top) })
    }
}


/// What the Compare tab is comparing against, and what it found.
struct CompareState {
    enum Source: Equatable {
        case file(BuildFile)
        case code(String, name: String)
        case spec(UInt32, title: String)

        var name: String {
            switch self {
            case .file(let f): return f.name
            case .code(_, let n): return n
            case .spec(_, let t): return "Tree: \(t)"
            }
        }
    }
    var source: Source?
    var rows: [CompareRow] = []
    var other: OtherBuild?
    var otherXML: String?
}
