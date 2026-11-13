import SwiftUI

/// PoB's "Find a Timeless Jewel": which seeds of a timeless jewel, in a given
/// socket, turn the passives in radius into the ones you want.
///
/// The engine runs PoB's own dialog headless for every step, so the node
/// lists, the weighting, the result order, the trade query and the item it
/// adds are all PoB's. This view is the controls around that: settings on
/// the left, the desired-node list beneath them, results on the right.
struct TimelessJewelSheet: View {
    @ObservedObject var model: BuildModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL

    @State private var state = TimelessState(
        jewelType: 1, conqueror: 1, devotion1: 1, devotion2: 1, socket: -1,
        socketFilter: false, socketDistance: 0, desired: [], fallback: [],
        fallbackMode: 1, totalMinimumWeight: nil
    )
    @State private var options: TimelessOptions?
    @State private var search = TimelessSearch(desired: [], results: [])
    @State private var protected: [String] = []
    @State private var selected: Int?
    /// Width of the results pane, so the per-node columns divide it rather
    /// than growing the sheet: six desired nodes at a fixed 96pt pushed the
    /// settings column off the left edge.
    @State private var resultsWidth: CGFloat = 800
    private var columnWidth: CGFloat {
        let cols = CGFloat(max(1, min(search.desired.count, 7)))
        let fixed: CGFloat = 72 + 72 + (multiSocket ? 150 : 0) + Tok.Space.s3 * 2 + Tok.Space.s2 * (cols + 2)
        return min(96, max(56, (resultsWidth - fixed) / cols))
    }
    @State private var listKind = ListKind.desired
    @State private var searching = false
    @State private var generating = false
    @State private var totalMinText = ""
    @State private var lastURL: String?
    @State private var addedMessage: String?

    // Trade settings are the user's, not the build's.
    @AppStorage("timelessLeague") private var league = "Standard"
    /// The trade site's league list, fetched from the endpoint PoB uses.
    /// Empty offline; the field still takes a typed name.
    @State private var leagues: [(id: String, realm: String)] = []
    /// Leagues for the chosen realm; the endpoint lists each once per realm.
    private var realmLeagues: [String] {
        let key = ["pc", "sony", "xbox"][min(realm, 2)]
        var seen = Set<String>()
        return leagues.filter { $0.realm == key || $0.realm.isEmpty }.map(\.id).filter { seen.insert($0).inserted }
    }
    @AppStorage("timelessRealm") private var realm = 0
    @AppStorage("timelessBuyout") private var buyout = 0
    /// 0: the selected seed only. 1: PoB's ten or so from it. 2: up to 180.
    @AppStorage("timelessTradeScope") private var tradeScope = 1

    enum ListKind: String, CaseIterable, Identifiable {
        case desired = "Desired nodes", fallback = "Fallback nodes"
        var id: String { rawValue }
    }

    private static let realms = ["PC", "Sony", "Xbox"]
    private static let buyouts = [
        "Instant buyout", "Instant buyout and in person", "In person (online in league)",
        "In person (online)", "Any (includes offline)",
    ]

    var body: some View {
        // A GeometryReader is sized by its container, not its content, so
        // this is the presented sheet's width whatever the columns want.
        GeometryReader { geo in
            content
                .frame(width: geo.size.width, height: geo.size.height)
                .onAppear { resultsWidth = geo.size.width - Tok.Space.s6 * 2 - 460 - Tok.Space.s4 }
                .onChange(of: geo.size.width) { _, w in resultsWidth = w - Tok.Space.s6 * 2 - 460 - Tok.Space.s4 }
        }
        .frame(minWidth: 1100, minHeight: 720)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear(perform: load)
        .task { await fetchLeagues() }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                Text("Find a Timeless Jewel").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
            }
            HStack(alignment: .top, spacing: Tok.Space.s4) {
                ScrollView {
                    VStack(alignment: .leading, spacing: Tok.Space.s3) {
                        settings
                        nodeLists
                    }
                }
                .frame(width: 460)
                results.frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            }
        }
        .padding(Tok.Space.s6)
    }

    // MARK: Settings

    private var jewelLabel: String {
        options?.jewelTypes.first { $0.id == state.jewelType }?.label ?? "Jewel"
    }
    private var conquerorLabel: String {
        options?.conquerors.first { $0.id == state.conqueror }?.label ?? "Any"
    }
    private var socketLabel: String {
        options?.sockets.first { $0.id == state.socket }?.label ?? "All Sockets"
    }
    private func devotionLabel(_ id: UInt8) -> String {
        options?.devotionVariants.first { $0.id == id }?.label ?? "Any"
    }
    /// Militant Faith and Reclaimed Malevolence can replace allocated
    /// notables; PoB lets those be protected.
    private var canProtect: Bool { (state.jewelType == 4 || state.jewelType == 11) && state.socketFilter }

    private var settings: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            SectionLabel(text: "Jewel")
            row("Jewel type") {
                DropPill(title: jewelLabel, maxWidth: 220) {
                    ForEach(options?.jewelTypes ?? [], id: \.id) { j in
                        Button(j.label) { setJewelType(j.id) }
                    }
                }
            }
            row("Conqueror") {
                DropPill(title: conquerorLabel, maxWidth: 220) {
                    ForEach(options?.conquerors ?? [], id: \.id) { c in
                        Button(c.label) { state.conqueror = c.id; store() }
                    }
                }
            }
            row("Jewel socket") {
                DropPill(title: socketLabel, maxWidth: 220) {
                    ForEach(options?.sockets ?? [], id: \.id) { s in
                        Button {
                            state.socket = s.id
                            protected = []
                            store(reloadOptions: true)
                        } label: {
                            if s.allocated { Label(s.label, systemImage: "checkmark.circle") } else { Text(s.label) }
                        }
                    }
                }
            }
            if let s = options?.sockets.first(where: { $0.id == state.socket }), s.allocated {
                Text("Allocated on the active tree").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
            row("Filter nodes") {
                HStack(spacing: Tok.Space.s3) {
                    Toggle(isOn: Binding(get: { state.socketFilter }, set: { on in
                        state.socketFilter = on
                        if !on { protected = [] }
                        store(reloadOptions: true)
                    })) { Text("Only nodes I have allocated").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }
                    .toggleStyle(CheckStyle())
                }
            }
            if state.socketFilter {
                row("Node distance") {
                    HStack(spacing: Tok.Space.s3) {
                        NumStepper(value: Int64(state.socketDistance), editable: true, compact: true, range: 0...10) {
                            state.socketDistance = .init($0); store()
                        }
                        Text("points past the tree still count").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                    }
                }
            }
            if canProtect {
                protectList
            }
            if state.jewelType == 4 {
                row("Devotion 1") {
                    DropPill(title: devotionLabel(state.devotion1), maxWidth: 220) {
                        ForEach(options?.devotionVariants ?? [], id: \.id) { d in
                            Button(d.label) { state.devotion1 = d.id; store() }
                        }
                    }
                }
                row("Devotion 2") {
                    DropPill(title: devotionLabel(state.devotion2), maxWidth: 220) {
                        ForEach(options?.devotionVariants ?? [], id: \.id) { d in
                            Button(d.label) { state.devotion2 = d.id; store() }
                        }
                    }
                }
            }
            if state.jewelType == 11, let asc = options?.abyssAscendancies, !asc.isEmpty {
                row("Ascendancy outcome") {
                    DropPill(title: requiredAscendancyLabel(asc), maxWidth: 220) {
                        Button("Any") { setRequiredAscendancy(nil) }
                        ForEach(asc, id: \.id) { a in Button(a.name) { setRequiredAscendancy(a.id) } }
                    }
                }
            }
        }
        .padding(Tok.Space.s4)
        .plate()
    }

    private var protectList: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            row("Protect") {
                DropPill(title: "Add a notable to keep", enabled: !(options?.protectCandidates.isEmpty ?? true), maxWidth: 220) {
                    ForEach(options?.protectCandidates ?? [], id: \.self) { name in
                        Button(name) { if !protected.contains(name) { protected.append(name) } }
                    }
                }
            }
            ForEach(protected, id: \.self) { name in
                HStack {
                    Text(name).font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                    Spacer()
                    Button { protected.removeAll { $0 == name } } label: {
                        Image(systemName: "xmark").font(.system(size: 12, weight: .bold))
                    }
                    .buttonStyle(IconPillStyle())
                    .accessibilityLabel("Stop protecting \(name)")
                }
            }
            Text("These stay as they are; seeds that would replace them are dropped.")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
        }
    }

    private func row<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: Tok.Space.s3) {
            Text(label).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift().frame(width: 130, alignment: .leading)
            content()
            Spacer(minLength: 0)
        }
    }

    // MARK: Desired and fallback nodes

    private var list: [DesiredNode] { listKind == .desired ? state.desired : state.fallback }

    private func setList(_ rows: [DesiredNode]) {
        if listKind == .desired { state.desired = rows } else { state.fallback = rows }
        store()
    }

    private var nodeLists: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            Picker("List", selection: $listKind) {
                // Counts, so a list you are not looking at cannot silently
                // filter the search: a leftover Desired minimum on the other
                // tab returned nothing and gave no hint why.
                ForEach(ListKind.allCases) { k in
                    let n = k == .desired ? state.desired.count : state.fallback.count
                    Text(n > 0 ? "\(k.rawValue) (\(n))" : k.rawValue).tag(k)
                }
            }
            .pickerStyle(.segmented)

            if listKind == .fallback {
                Text("Optional. Search needs only the desired list; these weights break ties between seeds that score the same on it. Generate scores every other node by what it does for a stat of this build.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                HStack(spacing: Tok.Space.s2) {
                    DropPill(title: options?.fallbackModes.indices.contains(Int(state.fallbackMode) - 1) == true
                             ? options!.fallbackModes[Int(state.fallbackMode) - 1] : "Sort by", maxWidth: 240) {
                        ForEach(Array((options?.fallbackModes ?? []).enumerated()), id: \.offset) { i, m in
                            Button(m) { state.fallbackMode = UInt32(i + 1); store() }
                        }
                    }
                    Button(generating ? "Generating…" : "Generate") { generateFallback() }
                        .buttonStyle(PillStyle()).disabled(generating)
                }
            } else {
                Text("Primary weight scores each passive the seed turns into this node; Second weights the node's second stat, when it has one. Min drops seeds scoring under it: primary 1 with min 3 means at least three of them, min 1 means it must appear. Results are sorted by Total, the sum of the columns.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }

            AddLegionNodeField(nodes: options?.nodes ?? [], existing: Set(list.map(\.id))) { node in
                var rows = list
                rows.append(DesiredNode(id: node.id, weight1: "1", weight2: node.statCount > 1 ? "1" : "0", minimum: "0"))
                setList(rows)
            }

            if list.isEmpty {
                Text(listKind == .desired ? "No nodes yet. Add the notables or stats you want the jewel to give you."
                                          : "No fallback weights.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift().padding(.vertical, Tok.Space.s2)
            } else {
                VStack(spacing: 0) {
                    HStack(spacing: Tok.Space.s2) {
                        SectionLabel(text: "Node").frame(maxWidth: .infinity, alignment: .leading)
                        SectionLabel(text: "Primary").frame(width: 70)
                        SectionLabel(text: "Second").frame(width: 70)
                        SectionLabel(text: "Min").frame(width: 96)
                        Color.clear.frame(width: 32)
                    }
                    .frame(height: 30)
                    ForEach(Array(list.enumerated()), id: \.element.id) { i, row in
                        DesiredNodeRow(
                            row: row,
                            node: options?.nodes.first { $0.id == row.id } ?? options?.abyssAscendancies.first { $0.id == row.id },
                            update: { updated in var rows = list; rows[i] = updated; setList(rows) },
                            remove: { var rows = list; rows.remove(at: i); setList(rows) }
                        )
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                }
            }

            row("Total minimum") {
                WellField(placeholder: "optional", text: $totalMinText).frame(width: 120)
                    .keyboardType(.decimalPad)
                    .onChange(of: totalMinText) { _, t in
                        state.totalMinimumWeight = Double(t.trimmingCharacters(in: .whitespaces))
                        store()
                    }
            }

            HStack {
                Button("Reset") {
                    state.desired = []; state.fallback = []; protected = []
                    search = TimelessSearch(desired: [], results: [])
                    selected = nil
                    store()
                }
                .buttonStyle(PillStyle(danger: true))
                Spacer()
                Button(searching ? "Searching…" : "Search") { runSearch() }
                    .buttonStyle(PillStyle(primary: true))
                    .disabled(searching || (state.desired.isEmpty && state.fallback.isEmpty))
            }
        }
        .padding(Tok.Space.s4)
        .plate()
    }

    // MARK: Results

    private var multiSocket: Bool { search.results.contains { $0.socketId != nil } }

    private var results: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                SectionLabel(text: "Results")
                Text(search.results.isEmpty ? "" : "\(search.results.count) seeds").font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift()
                Spacer()
                if let s = selected, search.results.indices.contains(s) {
                    Button(search.results[s].added ? "Added to build" : "Add to build") { addToBuild(s) }
                        .buttonStyle(PillStyle()).disabled(search.results[s].added)
                }
            }
            if let addedMessage {
                Text(addedMessage).font(Tok.ui(13)).foregroundStyle(Tok.ok)
            }

            VStack(spacing: 0) {
                HStack(spacing: Tok.Space.s2) {
                    SectionLabel(text: "Seed").frame(width: 72, alignment: .trailing)
                    if multiSocket { SectionLabel(text: "Socket").frame(width: 150, alignment: .leading) }
                    ForEach(Array(search.desired.prefix(7).enumerated()), id: \.offset) { _, name in
                        // A node name is one or two words; shrink a little
                        // before wrapping so a long word is not broken in two.
                        Text(name.uppercased()).font(Tok.label).tracking(0.5).foregroundStyle(Tok.ice700)
                            .lineLimit(2).minimumScaleFactor(0.6).multilineTextAlignment(.center).frame(width: columnWidth)
                    }
                    Spacer(minLength: 0)
                    SectionLabel(text: "Total").frame(width: 72, alignment: .trailing)
                }
                .padding(.horizontal, Tok.Space.s3).frame(height: 40)
                Rectangle().fill(Tok.ink400).frame(height: 1)
                if search.results.isEmpty {
                    Text(searching ? "Searching every seed…" : emptyMessage)
                        .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(Array(search.results.prefix(500).enumerated()), id: \.offset) { i, r in
                                Button { selected = (selected == i) ? nil : i } label: {
                                    HStack(spacing: Tok.Space.s2) {
                                        Text("\(r.seed)").font(Tok.mono(15)).foregroundStyle(r.added ? Tok.textTertiary : Tok.textPrimary).textLift()
                                            .frame(width: 72, alignment: .trailing)
                                        if multiSocket {
                                            Text(r.socketLabel).font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift()
                                                .lineLimit(1).frame(width: 150, alignment: .leading)
                                        }
                                        ForEach(Array(r.matches.prefix(7).enumerated()), id: \.offset) { _, m in
                                            Text(m.weight == 0 ? "·" : Self.fmt(m.weight))
                                                .font(Tok.mono(14)).foregroundStyle(m.weight == 0 ? Tok.textDisabled : Tok.textSecondary).textLift()
                                                .frame(width: columnWidth)
                                        }
                                        Spacer(minLength: 0)
                                        Text(Self.fmt(r.total)).font(Tok.mono(15)).foregroundStyle(Tok.cyan300)
                                            .frame(width: 72, alignment: .trailing)
                                    }
                                    .padding(.horizontal, Tok.Space.s3)
                                    .frame(height: 40)
                                    .background(selected == i ? Tok.cyan800 : Color.clear)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("Seed \(r.seed), total \(Self.fmt(r.total))\(r.added ? ", added to build" : "")")
                                if selected == i { detail(r) }
                                Rectangle().fill(Tok.ink400).frame(height: 1)
                            }
                            if search.results.count > 500 {
                                Text("Showing the best 500.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s3)
                            }
                        }
                    }
                }
            }
            .plate()
            .frame(maxHeight: .infinity)

            trade
        }
    }

    /// What a seed turns each passive into, PoB's tooltip on a result.
    private func detail(_ r: TimelessResult) -> some View {
        VStack(alignment: .leading, spacing: Tok.Space.s1) {
            ForEach(Array(r.matches.enumerated()), id: \.offset) { _, m in
                if !m.targets.isEmpty {
                    HStack(alignment: .top, spacing: Tok.Space.s2) {
                        // Read left to right as what happens: these passives
                        // become that node. Leading with the node read as
                        // the seed turning it into the passives.
                        Text(m.targets.joined(separator: ", ")).font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift()
                        Text("→").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        Text("\(m.targets.count > 1 ? "become" : "becomes") \(m.name)").font(Tok.ui(13, .medium)).foregroundStyle(Tok.textPrimary).textLift()
                    }
                }
            }
            if r.matches.allSatisfy({ $0.targets.isEmpty }) {
                Text("No passive in radius becomes a desired node; the score comes from fallback or total stats.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
        }
        .padding(.horizontal, Tok.Space.s3).padding(.bottom, Tok.Space.s2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Tok.cyan800.opacity(0.5))
    }

    private var trade: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            SectionLabel(text: "Trade")
            HStack(spacing: Tok.Space.s3) {
                Picker("Realm", selection: $realm) {
                    ForEach(Array(Self.realms.enumerated()), id: \.offset) { i, r in Text(r).tag(i) }
                }
                .pickerStyle(.segmented).frame(width: 200)
                Text("League").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                if realmLeagues.isEmpty {
                    WellField(placeholder: "e.g. Standard", text: $league).frame(width: 200)
                } else {
                    DropPill(title: league.isEmpty ? "Choose" : league, maxWidth: 260) {
                        ForEach(realmLeagues, id: \.self) { l in Button(l) { league = l } }
                    }
                }
                DropPill(title: Self.buyouts[min(buyout, Self.buyouts.count - 1)], maxWidth: 260) {
                    ForEach(Array(Self.buyouts.enumerated()), id: \.offset) { i, b in Button(b) { buyout = i } }
                }
            }
            HStack(spacing: Tok.Space.s3) {
                Picker("Seeds", selection: $tradeScope) {
                    Text("This seed").tag(0)
                    Text("Next 10").tag(1)
                    Text("Up to 180").tag(2)
                }
                .pickerStyle(.segmented).frame(width: 300)
                .accessibilityLabel("Seeds to search for")
                Text(tradeScope == 0 ? "Only the selected row's seed."
                     : tradeScope == 1 ? "The best 10 or so from the selected row, in order."
                     : "Up to 180 seeds from the selected row, unsorted.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                Spacer()
                Button("Open trade URL") { openTrade() }
                    .buttonStyle(PillStyle(primary: true))
                    .disabled(search.results.isEmpty || league.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let lastURL {
                HStack(spacing: Tok.Space.s2) {
                    Text(lastURL).font(Tok.mono(11)).foregroundStyle(Tok.textTertiary).textLift().lineLimit(1).truncationMode(.middle)
                    Button("Copy") { UIPasteboard.general.string = lastURL }.buttonStyle(PillStyle())
                }
            }
        }
        .padding(Tok.Space.s4)
        .plate()
    }

    // MARK: Actions

    private func load() {
        if let s = model.timelessState() { state = s }
        totalMinText = state.totalMinimumWeight.map { Self.fmt($0) } ?? ""
        options = model.timelessOptions()
        search = model.timelessResults()
    }

    /// Write the settings to the build, and re-read the options when the
    /// socket or filter changed what can be protected.
    private func store(reloadOptions: Bool = false) {
        model.setTimelessState(state)
        if reloadOptions { options = model.timelessOptions() }
    }

    /// PoB empties both node lists on a type change: the ids are per type.
    private func setJewelType(_ id: UInt8) {
        guard id != state.jewelType else { return }
        state.jewelType = id
        state.conqueror = 1
        state.desired = []
        state.fallback = []
        protected = []
        search = TimelessSearch(desired: [], results: [])
        selected = nil
        store(reloadOptions: true)
    }

    private func requiredAscendancyLabel(_ asc: [LegionNode]) -> String {
        state.desired.compactMap { d in asc.first { $0.id == d.id }?.name }.first ?? "Any"
    }

    /// Reclaimed Malevolence's required outcome is a desired row PoB writes
    /// as `id, 1, 0, 1`: weight one, minimum one, so seeds without it drop.
    private func setRequiredAscendancy(_ id: String?) {
        state.desired.removeAll { $0.id.hasPrefix("abyss_special_ascendancy_notable_") }
        if let id { state.desired.append(DesiredNode(id: id, weight1: "1", weight2: "0", minimum: "1")) }
        store()
    }

    /// Why the results are empty: never searched, or a minimum nothing met.
    private var emptyMessage: String {
        guard !search.desired.isEmpty else { return "No results yet. Set what you want and press Search." }
        let mins = state.desired.filter { ($0.minimum.lowercased() == "required") || (Double($0.minimum) ?? 0) > 0 }
        if mins.isEmpty { return "No seed scored anything. Widen the search: untick the allocated filter, or add nodes." }
        let list = mins.map { d in
            let name = options?.nodes.first { $0.id == d.id }?.name ?? d.id
            return d.minimum.lowercased() == "required" ? "\(name) required" : "\(name) at least \(d.minimum)"
        }.joined(separator: ", ")
        return "No seed met the minimums on the Desired list (\(list))\(state.socketFilter ? " on nodes you have allocated" : ""). Lower or clear them, then search again."
    }

    private func runSearch() {
        searching = true
        selected = nil
        addedMessage = nil
        store()
        Task {
            search = await model.timelessSearch(protected: protected)
            searching = false
        }
    }

    private func generateFallback() {
        generating = true
        store()
        Task {
            // PoB emits these in generation order; best first is what a
            // list of weights should read as.
            state.fallback = await model.timelessGenerateFallback()
                .sorted { (Double($0.weight1) ?? 0) > (Double($1.weight1) ?? 0) }
            store()
            generating = false
            listKind = .fallback
        }
    }

    private func addToBuild(_ i: Int) {
        guard let socket = model.timelessAddToBuild(i) else { return }
        let seed = search.results[i].seed
        search = model.timelessResults()
        let socketAllocated = options?.sockets.first { $0.id == state.socket }?.allocated ?? true
        addedMessage = socket.isEmpty
            ? "\(jewelLabel) \(seed) is in the item list. Socket it from the Items tab."
            : socketAllocated
                ? "\(jewelLabel) \(seed) is socketed in \(socket). The tree and the numbers reflect it; the previous jewel, if any, is back in the item list."
                : "\(jewelLabel) \(seed) is socketed in \(socket), but that socket is not allocated on the tree, so the jewel does nothing until you allocate it (tap the socket, then tap it again)."
    }

    /// The trade site's leagues, as PoB's TradeQuery fetches them. Failure
    /// leaves the typed field, so the sheet works offline.
    private func fetchLeagues() async {
        guard let url = URL(string: "https://www.pathofexile.com/api/trade/data/leagues"),
              let text = try? await BuildSites.fetch(url),
              let data = text.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = json["result"] as? [[String: Any]] else { return }
        let all = result.compactMap { entry -> (id: String, realm: String)? in
            guard let id = entry["id"] as? String else { return nil }
            return (id, (entry["realm"] as? String ?? "").lowercased())
        }
        guard !all.isEmpty else { return }
        leagues = all
        let ids = realmLeagues
        if !ids.isEmpty && !ids.contains(league) { league = ids[0] }
    }

    private func openTrade() {
        let start = selected ?? 0
        guard let url = model.timelessTradeURL(start: start, scope: tradeScope, league: league.trimmingCharacters(in: .whitespaces),
                                               realm: realm, buyout: buyout) else { return }
        lastURL = url
        UIPasteboard.general.string = url
        if let u = URL(string: url) { openURL(u) }
    }

    static func fmt(_ v: Double) -> String {
        v == v.rounded() ? String(Int(v)) : String(format: "%.2f", v)
    }
}

/// One desired or fallback node with its three weights.
struct DesiredNodeRow: View {
    let row: DesiredNode
    let node: LegionNode?
    let update: (DesiredNode) -> Void
    let remove: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: Tok.Space.s2) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(node?.name ?? row.id).font(Tok.ui(15, .medium)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(2)
                    if let s = node?.stats.first {
                        Text(s).font(Tok.ui(12)).foregroundStyle(Tok.textTertiary).textLift().lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                WeightField(value: row.weight1, allowRequired: false) { var r = row; r.weight1 = $0; update(r) }
                    .frame(width: 70)
                    .accessibilityLabel("Primary weight")
                WeightField(value: row.weight2, allowRequired: false) { var r = row; r.weight2 = $0; update(r) }
                    .frame(width: 70)
                    .disabled((node?.statCount ?? 1) < 2)
                    .opacity((node?.statCount ?? 1) < 2 ? 0.4 : 1)
                    .accessibilityLabel("Secondary weight")
                WeightField(value: row.minimum, allowRequired: true) { var r = row; r.minimum = $0; update(r) }
                    .frame(width: 96)
                    .accessibilityLabel("Minimum weight")
                // Removing is destructive, so it sits a clear gap away from
                // the lock rather than sharing its tap space.
                Button(action: remove) { Image(systemName: "xmark").font(.system(size: 12, weight: .bold)) }
                    .buttonStyle(IconPillStyle()).frame(width: 36)
                    .padding(.leading, Tok.Space.s3)
                    .accessibilityLabel("Remove \(node?.name ?? row.id)")
            }
        }
        .frame(height: 48)
    }
}

/// A weight: a number, or "required" for the minimum. Committed on end of
/// editing rather than per keystroke, since each commit writes the build.
struct WeightField: View {
    let value: String
    let allowRequired: Bool
    let commit: (String) -> Void
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 2) {
            TextField("0", text: $text)
                .textFieldStyle(.plain)
                .font(Tok.mono(14))
                .foregroundStyle(Tok.textPrimary).textLift()
                .multilineTextAlignment(.trailing)
                .keyboardType(allowRequired ? .default : .decimalPad)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .focused($focused)
                .padding(.horizontal, Tok.Space.s2)
                .frame(height: 32)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
                .onAppear { text = value }
                .onChange(of: value) { _, v in if !focused { text = v } }
                .onChange(of: focused) { _, f in if !f { commitText() } }
                .onSubmit(commitText)
            // No lock: PoB's "required" only means a score above 0.001, so
            // it is weaker than any minimum you would type. The word is still
            // accepted, since builds from the desktop save it.
        }
    }

    private func commitText() {
        let t = text.trimmingCharacters(in: .whitespaces).lowercased()
        let clean = (t == "required" && allowRequired) ? "required" : (Double(t).map { WeightField.trim($0) } ?? "0")
        text = clean
        if clean != value { commit(clean) }
    }

    static func trim(_ v: Double) -> String {
        v == v.rounded() ? String(Int(v)) : String(format: "%g", v)
    }
}

/// Find a legion node by name or stat text and add it.
struct AddLegionNodeField: View {
    let nodes: [LegionNode]
    let existing: Set<String>
    let add: (LegionNode) -> Void
    @State private var query = ""

    private var matches: [LegionNode] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return [] }
        return nodes.filter { n in
            !existing.contains(n.id)
                && (n.name.lowercased().contains(q) || n.stats.contains { $0.lowercased().contains(q) })
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s1) {
            WellField(placeholder: "Add a node: name or stat, e.g. \"double damage\"", text: $query)
            if !matches.isEmpty {
                VStack(spacing: 0) {
                    ForEach(matches.prefix(8), id: \.id) { n in
                        Button {
                            add(n)
                            query = ""
                        } label: {
                            HStack(spacing: Tok.Space.s2) {
                                Text(n.name).font(Tok.ui(15, n.notable || n.total ? .medium : .regular)).foregroundStyle(Tok.textPrimary).textLift()
                                    .lineLimit(1)
                                Text(n.stats.first ?? "").font(Tok.ui(12)).foregroundStyle(Tok.textTertiary).textLift().lineLimit(1)
                                Spacer()
                                Text(n.total ? "total" : n.notable ? "notable" : "small")
                                    .font(Tok.ui(11)).foregroundStyle(Tok.textTertiary).textLift()
                            }
                            .padding(.horizontal, Tok.Space.s3).frame(height: 36).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                }
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
            } else if !query.isEmpty {
                Text("Nothing matches.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
        }
    }
}
