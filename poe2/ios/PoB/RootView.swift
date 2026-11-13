import SwiftUI

/// The build's tabs, in the canvas's order. PoB's Import and Party tabs are
/// folded into the top chrome and left out respectively.
enum Tab: String, CaseIterable, Identifiable {
    case tree = "Tree", skills = "Skills", items = "Items", calcs = "Calcs"
    case config = "Config", compare = "Compare", notes = "Notes"
    var id: String { rawValue }

    /// Which of PoB's undo histories this tab edits, if any.
    var undoScope: UndoScope? {
        switch self {
        case .tree: return .tree
        case .skills: return .skills
        case .items: return .items
        case .config: return .config
        default: return nil
        }
    }
}

struct RootView: View {
    @StateObject private var model = BuildModel()
    @StateObject private var library = BuildLibrary()
    @ObservedObject private var theme = ThemeManager.shared
    @State private var tab: Tab = .tree
    @State private var sidebarOpen = true
    @State private var showImport = false
    /// The library, or the open build.
    @State private var home = true
    /// A library build being loaded into the engine from the home screen.
    @State private var opening = false
    @State private var showSave = false
    @State private var showSettings = false
    @State private var showAbout = false
    @ObservedObject private var links = LinkRouter.shared
    @State private var linkError: String?
    @State private var fetching = false
    /// Set while a link import is in flight, so a duplicate emission of the
    /// same pending link cannot start a second one.
    @State private var importingLink = false
    /// The stats sidebar is draggable between these widths. Named because the
    /// clamp is applied at two sites (the drag and the accessibility adjust);
    /// a lone literal in each drifts apart the moment one is changed.
    private static let sidebarMinWidth: Double = 200
    private static let sidebarMaxWidth: Double = 420
    @AppStorage("sidebarWidth") private var sidebarWidth: Double = Double(Tok.Size.sidebar)
    @State private var dragStart: Double?

    /// Load a library build on the engine's thread with the home screen
    /// dimmed under a spinner, then show it.
    private func openFromHome(_ url: URL) {
        guard !opening else { return }
        opening = true
        Task {
            await model.open(url)
            opening = false
            home = false
        }
    }

    var body: some View {
        Group {
            if home {
                HomeView(library: library,
                         open: { file in openFromHome(file.url) },
                         newBuild: {
                             if let f = library.create(named: "New build", xml: BuildLibrary.emptyBuildXML) {
                                 openFromHome(f.url)
                             }
                         },
                         importCode: { showImport = true },
                         settings: { showSettings = true },
                         about: { showAbout = true })
                .id(theme.theme.id)
            } else {
                buildScreen
            }
        }
        .overlay {
            if opening {
                ZStack {
                    Color.black.opacity(0.35).ignoresSafeArea()
                    ProgressView("Opening…").tint(Tok.textPrimary).foregroundStyle(Tok.textPrimary)
                        .padding(Tok.Space.s6).background(Tok.ink200, in: RoundedRectangle(cornerRadius: Tok.Radius.md))
                }
            }
        }
        .background(Tok.ink050)
        .background(
            // The canvas's atmosphere: cold light from above, nothing flat.
            RadialGradient(
                colors: [Tok.groundTop, Tok.groundMid, Tok.groundBot],
                center: UnitPoint(x: 0.5, y: -0.12), startRadius: 0, endRadius: 1400
            )
            .ignoresSafeArea()
        )
        .preferredColorScheme(theme.colorScheme)
        // No .id() here: it recreated the whole view tree, tearing down the
        // egui canvas and resetting its zoom on every render. `theme` is an
        // observed object, so publishing a change already re-renders the
        // chrome, and the colour tokens are read fresh at draw time.
        .animation(.easeOut(duration: 0.18), value: sidebarOpen)
        .sheet(isPresented: $showImport) {
            ImportSheet(model: model) { name in
                // From the home screen an import is a new library file,
                // named after the character when it came from the account;
                // inside a build it replaces the build, as PoB's does.
                if home, let xml = model.currentXML(), let f = library.create(named: name ?? "Imported build", xml: xml) {
                    // The engine already holds this build: adopt the file.
                    Task { await model.open(f.url, xml: xml); home = false }
                }
            }
        }
        .sheet(isPresented: $showSettings) { SettingsView(model: model, library: library) }
        .sheet(isPresented: $showAbout) { AboutView() }
        .sheet(isPresented: $showSave) {
            SaveSheet(name: model.buildName,
                      rename: { name in
                          guard let url = model.currentURL, let f = library.rename(BuildFile(url: url), to: name) else { return }
                          model.currentURL = f.url
                          UserDefaults.standard.set(f.url.path, forKey: "lastBuildPath")
                      },
                      saveCopy: { name in
                          model.save()
                          guard let xml = model.currentXML(), let f = library.create(named: name, xml: xml) else { return }
                          Task { await model.open(f.url, xml: xml) }
                      })
        }
        .onAppear {
            home = model.currentURL == nil
            model.undoScope = tab.undoScope
            model.beforeImport = { url, xml in library.snapshot(of: url, xml: xml, reason: "before import") }
            // A link stored before this view existed is delivered by the
            // onReceive above once it subscribes; nothing to do here.
            // A first launch with nothing in the library gets the sample
            // build the design was drawn against, so there is something to look at.
            if library.builds.isEmpty, let url = Bundle.main.url(forResource: "sample-build", withExtension: "xml"),
               let xml = try? String(contentsOf: url, encoding: .utf8) {
                library.create(named: "Flameblast Gemling Legionnaire", xml: xml)
            }
        }
        .onChange(of: tab) { _, t in model.undoScope = t.undoScope }
        .onReceive(links.$tick) { _ in
            // consume() returns the URL once; a second delivery of the same
            // link (cold launch fires both scene callbacks) yields nil.
            guard let url = links.consume() else { return }
            Task { await importLink(url.absoluteString) }
        }
        .alert("Could not import", isPresented: Binding(get: { linkError != nil }, set: { if !$0 { linkError = nil } })) {
            Button("OK") { linkError = nil }
        } message: { Text(linkError ?? "") }
        .overlay {
            if fetching {
                VStack(spacing: Tok.Space.s3) {
                    ProgressView().tint(Tok.cyan500)
                    Text("Fetching build…").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                }
                .padding(Tok.Space.s6).background(Tok.ink300)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.lg).stroke(Tok.ink500, lineWidth: 1))
                .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.lg))
            }
        }
    }

    /// A share-site link, from a pob:// URI or pasted: download the code
    /// and make a new build in the library. Never replaces the open build.
    private func importLink(_ text: String) async {
        guard !importingLink else { return }
        importingLink = true
        defer { importingLink = false }
        guard let (site, url) = BuildSites.resolve(text) else {
            linkError = "That is not a link to a site PoB knows (pobb.in, poe.ninja, Maxroll, pob.codes, Pastebin, Rentry, poedb)."
            return
        }
        fetching = true
        defer { fetching = false }
        do {
            let code = try await BuildSites.fetch(url)
            // Load the code, write it to a new library file, then open that
            // file so it becomes the current build. Any build that was open
            // is saved by close(); the new one always wins.
            let previous = model.currentURL
            if previous != nil { model.close() }
            if let err = await model.importPaste(code) {
                // Put the build that was open back; close() saved it.
                if let previous { await model.open(previous) }
                linkError = err
                return
            }
            guard let xml = model.currentXML(),
                  let f = library.create(named: "\(site.label) build", xml: xml) else {
                linkError = "Could not save the imported build."
                return
            }
            await model.open(f.url, xml: xml)
            home = false
        } catch {
            linkError = error.localizedDescription
        }
    }

    private var buildScreen: some View {
        VStack(spacing: 0) {
            TopChrome(model: model, showImport: $showImport, goHome: {
                model.close()
                library.reload()
                home = true
            }, showSave: $showSave)
            .id(theme.theme.id)
            Rectangle().fill(Tok.ink500).frame(height: 1)

            GeometryReader { geo in
            // In portrait on a narrow screen the fixed-width sidebar plus the
            // content is wider than the screen, which shoved the whole row off
            // to the left. Cap the sidebar at half the available width so the
            // row always fits, and never below the sidebar's minimum.
            let cappedSidebar = min(sidebarWidth, max(Self.sidebarMinWidth, geo.size.width * 0.5))
            HStack(spacing: 0) {
                if sidebarOpen {
                    StatsSidebar(model: model, collapse: { sidebarOpen = false })
                        .id(theme.theme.id)
                        .frame(width: cappedSidebar)
                        .transition(.move(edge: .leading))
                    // The drag handle: a hairline with a wide hit area.
                    Rectangle().fill(dragStart == nil ? Tok.ink500 : Tok.cyan700)
                        .frame(width: 1)
                        .padding(.horizontal, 6)
                        .contentShape(Rectangle())
                        .gesture(
                            DragGesture(minimumDistance: 2)
                                .onChanged { g in
                                    if dragStart == nil { dragStart = sidebarWidth }
                                    sidebarWidth = min(Self.sidebarMaxWidth, max(Self.sidebarMinWidth, (dragStart ?? sidebarWidth) + g.translation.width))
                                }
                                .onEnded { _ in dragStart = nil }
                        )
                        .accessibilityLabel("Resize build stats")
                        .accessibilityValue("\(Int(sidebarWidth)) points")
                        .accessibilityAdjustableAction { d in
                            sidebarWidth = min(Self.sidebarMaxWidth, max(Self.sidebarMinWidth, sidebarWidth + (d == .increment ? 20 : -20)))
                        }
                }

                VStack(spacing: 0) {
                    TabRow(selection: $tab, sidebarOpen: sidebarOpen, expand: { sidebarOpen = true })
                        .id(theme.theme.id)
                    Rectangle().fill(Tok.ink500).frame(height: 1)
                    ZStack {
                        // The Tree stays mounted and merely hidden when
                        // another tab is up: it hosts the egui/Metal canvas,
                        // and rebuilding it (as a `switch` would) tears the
                        // canvas down and resets its zoom every time you
                        // return to the tab.
                        TreeTab(model: model)
                            .opacity(tab == .tree ? 1 : 0)
                            .allowsHitTesting(tab == .tree)
                            .accessibilityHidden(tab != .tree)

                        Group {
                            switch tab {
                            case .tree:
                                EmptyView()
                            case .skills:
                                SkillsTab(model: model)
                            case .items:
                                ItemsTab(model: model)
                            case .calcs:
                                CalcsTab(model: model)
                            case .config:
                                ConfigTab(model: model)
                            case .notes:
                                NotesTab(model: model)
                            case .compare:
                                CompareTab(model: model, library: library, goToTree: { tab = .tree })
                            default:
                                Placeholder(name: tab.rawValue)
                            }
                        }
                        .id(theme.theme.id)
                        .opacity(tab == .tree ? 0 : 1)
                    }
                    // A tab that overflows must not push the sidebar; it
                    // gets clipped on its own edge instead.
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .clipped()
                }
                .frame(maxWidth: .infinity)
            }
            .frame(width: geo.size.width, height: geo.size.height)
            }
        }
    }
}

/// The 52pt bar: back, name, class, loadout · points, level, import, save.
struct TopChrome: View {
    /// The level number tapped open for typing.
    @State private var editingLevel = false
    @State private var levelText = ""

    private func commitLevel() {
        if let n = Int64(levelText.trimmingCharacters(in: .whitespaces)) { model.setLevel(n) }
        editingLevel = false
    }

    /// PoB ties an item, skill and config set into a loadout by giving
    /// their titles the same "{n}" tag. It is bookkeeping, not the name.
    static func loadoutLabel(_ raw: String) -> String {
        raw.replacingOccurrences(of: #"\s*\{[\w,]+\}\s*$"#, with: "", options: .regularExpression)
    }

    @ObservedObject var model: BuildModel
    @Binding var showImport: Bool
    var goHome: () -> Void
    @Binding var showSave: Bool
    @State private var confirmClass: ClassChoice?

    /// The level by which every Labyrinth is expected done, so an unspent
    /// ascendancy point past it reads as a misclick rather than progress.
    /// PoB uses this same threshold for its "Uber Lab" progress suggestion.
    private static let endgameLevel: Int64 = 90

    /// An ascendancy point is unspent on a character old enough that every lab
    /// should be finished: almost always a stray refund, not unfinished labs.
    private var ascendancyUnderAllocated: Bool {
        model.info.ascendancyPointsUsed < model.info.ascendancyPointsMax
            && model.info.level >= Self.endgameLevel
    }

    /// The colour for the ascendancy X / 8 count. Red when over max (as PoB
    /// shows it) and also red when under-allocated at endgame (see above).
    private var ascendancyPointsColor: Color {
        let over = model.info.ascendancyPointsUsed > model.info.ascendancyPointsMax
        return over || ascendancyUnderAllocated ? Tok.coral500 : Tok.textSecondary
    }

    private var pointsAccessibilityLabel: String {
        var s = "Points \(model.info.pointsUsed) of \(model.info.pointsMax), ascendancy \(model.info.ascendancyPointsUsed) of \(model.info.ascendancyPointsMax)"
        if ascendancyUnderAllocated {
            s += ". You have an unspent ascendancy point"
        }
        return s
    }

    var body: some View {
        // The whole bar scrolls horizontally: in portrait on a narrow screen
        // it is wider than the viewport, and clipping it hid Import / Rename
        // off the right edge. On a wide screen it does not overflow, so it
        // reads as a plain bar.
        HScrollRow {
        HStack(spacing: Tok.Space.s3) {
            Button(action: goHome) {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.left").font(.system(size: 12, weight: .bold))
                    Text("Builds")
                }
            }
            .buttonStyle(PillStyle())
            .accessibilityLabel("Back to builds")

            Text(model.buildName.isEmpty ? "Untitled build" : model.buildName)
                .font(Tok.display(19))
                .foregroundStyle(Tok.textPrimary).textLift()
                .lineLimit(1).truncationMode(.tail)
                // A fixed width inside a scrolling row: the name no longer
                // needs to yield space to fit, since the row can grow past
                // the viewport and be scrolled.
                .frame(width: 240, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)

            // Class, then its ascendancies. PoB warns when the class change
            // would unallocate the tree; so does this.
            DropPill(title: model.info.class) {
                ForEach(model.classes, id: \.id) { c in
                    Button {
                        if model.classChangeResetsTree(c.id) { confirmClass = c } else { model.setClass(c.id) }
                    } label: {
                        if c.name == model.info.class { Label(c.name, systemImage: "checkmark") } else { Text(c.name) }
                    }
                }
            }
            DropPill(title: model.info.ascendancy.isEmpty ? "None" : model.info.ascendancy) {
                ForEach(model.classes.first { $0.name == model.info.class }?.ascendancies ?? [], id: \.id) { a in
                    Button {
                        model.setAscendancy(a.id)
                    } label: {
                        if a.name == model.info.ascendancy { Label(a.name, systemImage: "checkmark") } else { Text(a.name) }
                    }
                }
            }

            DropPill(title: "Loadout: \(Self.loadoutLabel(model.activeLoadout ?? "Default"))", maxWidth: 240) {
                ForEach(model.loadouts, id: \.self) { name in
                    Button(Self.loadoutLabel(name)) { model.switchLoadout(name) }
                }
            }

            Spacer(minLength: Tok.Space.s5).frame(width: Tok.Space.s5)

            HStack(spacing: Tok.Space.s2) {
                Text("POINTS").font(Tok.label).tracking(Tok.labelTracking).foregroundStyle(Tok.ice700)
                    .lineLimit(1)
                // Over-allocated points read red, as PoB shows them. The
                // ascendancy pool also reads red when it is *under* max on an
                // endgame character: at level 90+ every lab is long done, so
                // an unspent ascendancy point is a misclick, not progress.
                // PoB never flags that; the red number catches it without a
                // popup to dismiss.
                Text("\(model.info.pointsUsed) / \(model.info.pointsMax)")
                    .font(Tok.mono(15))
                    .foregroundStyle(model.info.pointsUsed > model.info.pointsMax ? Tok.coral500 : Tok.textPrimary).textLift()
                Text("\(model.info.ascendancyPointsUsed) / \(model.info.ascendancyPointsMax)")
                    .font(Tok.mono(13))
                    .foregroundStyle(ascendancyPointsColor)
            }
            .fixedSize()
            .accessibilityElement(children: .combine)
            .accessibilityLabel(pointsAccessibilityLabel)

            // Level: step it, or long-press for PoB's automatic level.
            HStack(spacing: 0) {
                Button("−") { model.setLevel(model.info.level - 1) }.frame(width: Tok.Size.touchMin)
                    .accessibilityLabel("Level down")
                // Tap the number to type a level, or use PoB's automatic one.
                Button {
                    levelText = "\(model.info.level)"
                    editingLevel = true
                } label: {
                    HStack(spacing: 4) {
                        Text("LV").font(Tok.label).tracking(1).foregroundStyle(Tok.ice700)
                        Text("\(model.info.level)").font(Tok.mono(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                    }
                    .padding(.horizontal, Tok.Space.s2)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Level \(model.info.level), tap to type a level")
                .popover(isPresented: $editingLevel, arrowEdge: .top) {
                    VStack(spacing: Tok.Space.s3) {
                        Text("Character level").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        TextField("95", text: $levelText)
                            .keyboardType(.numberPad)
                            .font(Tok.mono(23, .bold)).foregroundStyle(Tok.textPrimary)
                            .multilineTextAlignment(.center)
                            .frame(width: 110, height: 44)
                            .background(Tok.ink100)
                            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                            .onSubmit { commitLevel() }
                        HStack(spacing: Tok.Space.s2) {
                            Button("Automatic") { model.setLevelAuto(); editingLevel = false }.buttonStyle(PillStyle())
                            Button("Set") { commitLevel() }.buttonStyle(PillStyle(primary: true))
                        }
                        Text("1 to 100. Automatic uses the tree's point count, as PoB does.")
                            .font(Tok.ui(12)).foregroundStyle(Tok.textTertiary).textLift()
                            .multilineTextAlignment(.center).frame(width: 220)
                    }
                    .padding(Tok.Space.s4)
                    .background(Tok.ink300)
                    .presentationCompactAdaptation(.popover)
                }
                Button("+") { model.setLevel(model.info.level + 1) }.frame(width: Tok.Size.touchMin)
                    .accessibilityLabel("Level up")
            }
            .font(Tok.ui(17))
            .foregroundStyle(Tok.textSecondary).textLift()
            .fixedSize()
            .frame(height: Tok.Size.touchMin)
            .background(Tok.ink300)
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            .buttonStyle(.plain)

            // Undo and redo for the tab on screen. PoB keeps a history per
            // tab, and an accidental tap on the tree is the case that
            // matters most: the build is written on every edit, so undo is
            // the only way back.
            HStack(spacing: 2) {
                Button { model.undo() } label: {
                    Image(systemName: "arrow.uturn.backward").font(.system(size: 15, weight: .semibold))
                }
                .keyboardShortcut("z", modifiers: .command)
                .disabled(!model.undoState.canUndo)
                .accessibilityLabel("Undo")
                Button { model.redo() } label: {
                    Image(systemName: "arrow.uturn.forward").font(.system(size: 15, weight: .semibold))
                }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!model.undoState.canRedo)
                .accessibilityLabel("Redo")
            }
            .buttonStyle(IconPillStyle())

            Button("Import / Export…") { showImport = true }.buttonStyle(PillStyle())
            // Every edit is already written; Save is PoB's Save As: rename,
            // or a copy in the library.
            // Everything autosaves; this is naming, not saving.
            Button("Rename / copy…") { showSave = true }.buttonStyle(PillStyle(primary: true))
        }
        .padding(.horizontal, Tok.Space.s4)
        .frame(height: Tok.Size.topBar)
        .confirmationDialog(
            "Change class to \(confirmClass?.name ?? "")?",
            isPresented: Binding(get: { confirmClass != nil }, set: { if !$0 { confirmClass = nil } }),
            titleVisibility: .visible
        ) {
            Button("Change class and reset the tree", role: .destructive) {
                if let c = confirmClass { model.setClass(c.id) }
            }
        } message: {
            Text("The allocated passives do not connect to that class's start, so PoB unallocates them. Undo on the Tree tab brings them back.")
        }
        }
        .frame(height: Tok.Size.topBar)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Tok.bar)
    }
}

/// The tab row: cyan underline under the current tab, never a filled pill.
struct TabRow: View {
    @Binding var selection: Tab
    var sidebarOpen: Bool
    var expand: () -> Void

    var body: some View {
        // The sidebar toggle stays pinned; the tabs scroll horizontally so
        // that on a narrow (portrait) screen every tab stays reachable
        // rather than being clipped off the right edge.
        HStack(spacing: 0) {
            if !sidebarOpen {
                Button(action: expand) {
                    Image(systemName: "sidebar.leading").foregroundStyle(Tok.cyan500)
                }
                .frame(width: Tok.Size.touchMin, height: Tok.Size.touchMin)
                .accessibilityLabel("Show build stats")
                .padding(.leading, Tok.Space.s2)
            }
            HScrollRow {
                HStack(spacing: 0) {
                    ForEach(Tab.allCases) { t in
                        Button { selection = t } label: {
                            VStack(spacing: 0) {
                                Text(t.rawValue)
                                    .font(Tok.ui(17, .bold))
                                    .foregroundStyle(selection == t ? Tok.textPrimary : Tok.textSecondary).textLift()
                                    .padding(.horizontal, Tok.Space.s4)
                                    .frame(height: 50)
                                Rectangle()
                                    .fill(selection == t ? Tok.cyan500 : .clear)
                                    .frame(height: 2)
                            }
                            .contentShape(Rectangle())
                            // The underline has no intrinsic width and would grow the
                            // tab to fill the row; tabs pack left in the canvas.
                            .fixedSize(horizontal: true, vertical: false)
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(selection == t ? [.isSelected] : [])
                    }
                }
            }
        }
        .padding(.leading, Tok.Space.s3)
        .frame(height: 52)
        // The tab row is chrome, like the top bar; the pills under it
        // belong to the content. A bar colour fading into the ground keeps
        // the two rows from reading as one.
        .background(
            LinearGradient(colors: [Tok.bar, Tok.bar.opacity(0.55), Tok.ink050.opacity(0)], startPoint: .top, endPoint: .bottom)
        )
    }
}

/// A tab that has not been built yet.
struct Placeholder: View {
    let name: String
    var body: some View {
        VStack(spacing: Tok.Space.s3) {
            Spacer()
            SectionLabel(text: name)
            Text("Not built yet").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Paste a share code or XML. Codes decode on device.
struct ImportSheet: View {
    @ObservedObject var model: BuildModel
    /// Called after an import; the character's name when it came from the
    /// account, so a build made from the home screen can carry it.
    var onSuccess: (String?) -> Void = { _ in }
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var error: String?
    @State private var code: String?
    @State private var importing = false
    @State private var copied = false
    @ObservedObject private var account = PoeAccount.shared
    // PoE2 characters live on a single "poe2" realm (PoB2 offers only this;
    // the PoE1 pc/sony/xbox platform split does not apply). The API path
    // builder turns this into /character/poe2.
    private let realm = "poe2"
    @State private var league = "Any"
    @State private var character: PoeAccount.Character?
    @State private var deleteJewels = true
    @State private var clearItems = true
    @State private var clearSkills = true
    @State private var ignoreWeaponSwap = false
    @State private var importMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text(model.currentURL != nil ? "Import / Export" : "Import build").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            if model.currentURL != nil {
                Text("Import a build").font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
            }
            Text("Paste a PoB share code, raw XML, or a link from pobb.in, poe.ninja, Maxroll, pob.codes, Pastebin, Rentry or poedb.").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            TextEditor(text: $text)
                .font(Tok.mono(13))
                .scrollContentBackground(.hidden)
                .padding(Tok.Space.s2)
                .frame(minHeight: model.currentURL != nil ? 160 : 220)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(error == nil ? Tok.ink500 : Tok.coral500, lineWidth: 1))
            if let error {
                Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300)
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Import") {
                    if BuildSites.resolve(text) != nil {
                        // A link: hand it to the root, which downloads the
                        // code and makes a new build from it.
                        dismiss()
                        LinkRouter.shared.deliver(URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)) ?? URL(string: "pob://invalid/x")!)
                        return
                    }
                    importing = true
                    Task {
                        error = await model.importPaste(text)
                        importing = false
                        if error == nil { dismiss(); onSuccess(nil) }
                    }
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(importing || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if importing { ProgressView().tint(Tok.textPrimary) }
            }
            accountImport
            if model.currentURL != nil { export }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 640)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }

    /// PoB's Import tab, account side: sign in, pick a character, pull its
    /// tree or its items and skills through PoB's own import code.
    private var accountImport: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            Rectangle().fill(Tok.ink500).frame(height: 1).padding(.vertical, Tok.Space.s2)
            Text("Import from your account").font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
            if !account.signedIn {
                Text("Sign in to pathofexile.com to list your characters. The sign-in stays in this app, so the Steam app cannot interrupt it.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Button(account.busy ? "Signing in…" : "Sign in to Path of Exile") { account.signIn() }
                    .buttonStyle(PillStyle(primary: true)).disabled(account.busy)
            } else {
                HStack(spacing: Tok.Space.s3) {
                    Button(account.busy ? "Loading…" : "Load characters") { Task { await account.loadCharacters(realm: realm) } }
                        .buttonStyle(PillStyle()).disabled(account.busy)
                    Spacer()
                    Button("Sign out") { account.signOut(); character = nil }.buttonStyle(PillStyle())
                }
                if !account.characters.isEmpty {
                    let leagues = ["Any"] + Array(Set(account.characters.map(\.league))).sorted()
                    let shown = account.characters.filter { league == "Any" || $0.league == league }
                    HStack(spacing: Tok.Space.s3) {
                        DropPill(title: league, maxWidth: 220) {
                            ForEach(leagues, id: \.self) { l in Button(l) { league = l } }
                        }
                        DropPill(title: character.map { "\($0.name)  ·  \($0.class) \($0.level)" } ?? "Choose a character", maxWidth: 360) {
                            ForEach(shown) { c in Button("\(c.name)  ·  \(c.class) \(c.level)  ·  \(c.league)") { character = c } }
                        }
                    }
                    if fromHome {
                        // A new build wants the whole character; the halves
                        // only make sense inside a build you are updating.
                        HStack(spacing: Tok.Space.s3) {
                            Button(account.busy ? "Importing…" : "Import character as a new build") { importTree() }
                                .buttonStyle(PillStyle(primary: true)).disabled(character == nil || account.busy)
                            Toggle(isOn: $ignoreWeaponSwap) { Text("Ignore weapon swap").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                        }
                    } else {
                        Text("Replace part of this build with the character's, as the desktop's Import tab does.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        HStack(spacing: Tok.Space.s3) {
                            Button("Passive tree and jewels") { importTree() }
                                .buttonStyle(PillStyle(primary: true)).disabled(character == nil || account.busy)
                            Toggle(isOn: $deleteJewels) { Text("Delete jewels").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                        }
                        HStack(spacing: Tok.Space.s3) {
                            Button("Items and skills") { importItems() }
                                .buttonStyle(PillStyle(primary: true)).disabled(character == nil || account.busy)
                            Toggle(isOn: $clearSkills) { Text("Delete skills").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                            Toggle(isOn: $clearItems) { Text("Delete equipment").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                            Toggle(isOn: $ignoreWeaponSwap) { Text("Ignore weapon swap").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                        }
                    }
                }
            }
            if let e = account.error { Text(e).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }
            if let m = importMessage { Text(m).font(Tok.ui(13)).foregroundStyle(Tok.ok) }
        }
        .task { if account.signedIn && account.characters.isEmpty { await account.loadCharacters(realm: realm) } }
        // Default the league filter to the first character's league (the API
        // lists the most recently played first) rather than "Any", so a
        // leveling player lands on their current-league character.
        .onChange(of: account.characters) { _, chars in
            if league == "Any", let current = chars.first?.league { league = current }
        }
    }

    /// From the home screen there is no build to import into: start an
    /// empty one in the engine, and the caller files it under the
    /// character's name afterwards.
    private var fromHome: Bool { model.currentURL == nil }

    private func importTree() {
        guard let c = character else { return }
        importMessage = nil
        Task {
            guard let json = await account.loadCharacter(realm: realm, name: c.name) else {
                if account.error == nil { account.error = "No character data came back for \(c.name)." }
                return
            }
            if fromHome, let err = await model.importPaste(BuildLibrary.emptyBuildXML) {
                account.error = "Couldn't start a build: \(err)"; return
            }
            if let err = model.importCharacterTree(json, deleteJewels: deleteJewels) { account.error = "Tree import failed: \(err)"; return }
            importMessage = "Imported \(c.name)'s passive tree\(deleteJewels ? "" : " and jewels")." + (fromHome ? "" : " The build as it was is kept under Compare.")
            if fromHome {
                // Both halves, since a new build wants the whole character.
                if let err = model.importCharacterItems(json, clearItems: true, clearSkills: true, ignoreWeaponSwap: ignoreWeaponSwap) {
                    account.error = "Items import failed: \(err)"; return
                }
                dismiss(); onSuccess(c.name)
            }
        }
    }

    private func importItems() {
        guard let c = character else { return }
        importMessage = nil
        Task {
            guard let json = await account.loadCharacter(realm: realm, name: c.name) else {
                if account.error == nil { account.error = "No character data came back for \(c.name)." }
                return
            }
            if fromHome, let err = await model.importPaste(BuildLibrary.emptyBuildXML) {
                account.error = "Couldn't start a build: \(err)"; return
            }
            if let err = model.importCharacterItems(json, clearItems: clearItems, clearSkills: clearSkills, ignoreWeaponSwap: ignoreWeaponSwap) { account.error = "Items import failed: \(err)"; return }
            importMessage = "Imported \(c.name)'s items and skills." + (fromHome ? "" : " The build as it was is kept under Compare.")
            if fromHome {
                if let err = model.importCharacterTree(json, deleteJewels: false) { account.error = "Tree import failed: \(err)"; return }
                dismiss(); onSuccess(c.name)
            }
        }
    }

    /// PoB's Export section: the build as a share code, the same bytes the
    /// desktop's Generate button makes. Copied to the clipboard on Generate,
    /// as PoB's Copy does, and offered to the share sheet.
    private var export: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            Rectangle().fill(Tok.ink500).frame(height: 1).padding(.vertical, Tok.Space.s2)
            Text("Export this build").font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
            Text("A share code for pobb.in, poe.ninja, Discord, or the desktop's Import tab.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            HStack(spacing: Tok.Space.s2) {
                Button("Generate code") {
                    code = model.exportCode()
                    if let code { UIPasteboard.general.string = code; copied = true }
                }
                .buttonStyle(PillStyle())
                if let code {
                    Text(code).font(Tok.mono(11)).foregroundStyle(Tok.textTertiary).textLift().lineLimit(1).truncationMode(.middle)
                    Button(copied ? "Copied" : "Copy") { UIPasteboard.general.string = code; copied = true }.buttonStyle(PillStyle())
                    ShareLink(item: code) { Text("Share") }.buttonStyle(PillStyle())
                }
            }
        }
    }
}

/// The canvas's Save dialog: rename this build, or save a copy to the list.
struct SaveSheet: View {
    let name: String
    var rename: (String) -> Void
    var saveCopy: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var draft = ""

    private var clean: String { draft.trimmingCharacters(in: .whitespaces) }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Rename or copy").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            Text("Every change is already saved to \"\(name)\". Rename it, or make a copy under a new name and switch to that.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            WellField(placeholder: "Build name", text: $draft)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Rename") { rename(clean); dismiss() }
                    .buttonStyle(PillStyle())
                    .disabled(clean.isEmpty || clean == name)
                Button("Save as copy") { saveCopy(clean); dismiss() }
                    .buttonStyle(PillStyle(primary: true))
                    .disabled(clean.isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 480)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear { draft = name }
    }
}
