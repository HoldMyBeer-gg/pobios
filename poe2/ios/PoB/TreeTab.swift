import SwiftUI

/// The passive tree: native toolbar and footer around the egui canvas.
///
/// The canvas stays egui on purpose. It draws tens of thousands of nodes
/// from PoB's sprite sheets with pan, zoom and hit-testing, which is a
/// custom Metal surface in any toolkit. It is hosted in an MTKView that
/// SwiftUI sizes, so it is native by every measure that matters.
struct TreeTab: View {
    @ObservedObject var model: BuildModel
    @State private var search = ""
    @State private var matched = 0
    @State private var showLinks = false
    @State private var confirmReset = false
    @State private var confirmConvert: TreeVersion?
    @State private var naming: SpecNaming?
    @State private var showReport = false
    @State private var showTimeless = false
    /// Restores the search highlight after a focus lights one node.
    @State private var focusReset: Timer?
    /// The search in flight: PoB's matcher walks every node, so it runs
    /// once the typing pauses rather than on every keystroke.
    @State private var searchTask: Task<Void, Never>?
    /// A tapped jewel socket: the slot to fill, shown as a picker.
    @State private var jewelSocket: JewelSocketTarget?
    /// A tapped mastery node with effects to choose from.
    @State private var masteryTarget: MasteryTarget?

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            toolbar

            ZStack(alignment: .bottomLeading) {
                EguiView()
                    .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.lg))
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.lg).stroke(Tok.ink500, lineWidth: 1))

                HStack(spacing: Tok.Space.s2) {
                    Chip("\(model.info.pointsUsed) / \(model.info.pointsMax) allocated")
                    Chip("Compare: \(model.compareSpec.flatMap { i in model.treeSpecs.first { $0.index == i }?.title } ?? "off")")
                    if !search.isEmpty { Chip("\(matched) matched") }
                    if model.nodePower.running { Chip("Ranking nodes… \(model.nodePower.progress)%") }
                }
                .padding(Tok.Space.s5)
                .allowsHitTesting(false)

                // PoB's Power Report: the ranked list, once a ranking exists.
                if !model.nodePower.report.isEmpty, !model.nodePower.running {
                    HStack {
                        Spacer()
                        Button("Power report · \(model.nodePower.report.count)") { showReport = true }
                            .buttonStyle(PillStyle(primary: true))
                    }
                    .padding(Tok.Space.s5)
                }
            }
        }
        .padding(.horizontal, Tok.Space.s4)
        .padding(.top, Tok.Space.s3)
        .padding(.bottom, Tok.Space.s4)
        .onChange(of: search) { _, q in
            searchTask?.cancel()
            let searching = !q.trimmingCharacters(in: .whitespaces).isEmpty
            // Clearing is instant; a query waits for the typing to pause.
            if !searching {
                matched = 0
                EguiBridge.setSearch([], searching: false)
                return
            }
            searchTask = Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(150))
                guard !Task.isCancelled else { return }
                let ids = model.searchNodes(q)
                matched = ids.count
                EguiBridge.setSearch(ids, searching: true)
            }
        }
        // A tap pins a socket's tooltip; the same tap opens the jewel
        // picker, since "put a jewel here" is what a tap on a socket means.
        .onChange(of: model.pinnedNode) { _, node in
            guard let node else { return }
            // A socket tap fills a jewel; a mastery tap picks its effect.
            if let slot = model.socketSlot(for: node) {
                jewelSocket = JewelSocketTarget(node: node, slot: slot)
                return
            }
            let effects = model.masteryEffects(node)
            if !effects.isEmpty {
                masteryTarget = MasteryTarget(node: node, effects: effects,
                                              selected: model.masterySelection(node))
            }
        }
        .sheet(item: $jewelSocket) { target in JewelPickerSheet(model: model, target: target) }
        .sheet(item: $masteryTarget) { target in MasteryPickerSheet(model: model, target: target) }
        .sheet(isPresented: $showLinks) { TreeLinksSheet(model: model) }
        .sheet(isPresented: $showTimeless) { TimelessJewelSheet(model: model) }
        .sheet(isPresented: $showReport) { PowerReportSheet(model: model, onFocus: { id in
            EguiBridge.focusNode(id)
            // The focus borrows the search highlight; give it back.
            focusReset?.invalidate()
            focusReset = Timer.scheduledTimer(withTimeInterval: 4, repeats: false) { _ in
                let q = search
                let ids = model.searchNodes(q)
                EguiBridge.setSearch(ids, searching: !q.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }) }
        .sheet(item: $naming) { n in SpecNameSheet(model: model, naming: n) }
        .confirmationDialog("Reset the tree?", isPresented: $confirmReset, titleVisibility: .visible) {
            Button("Unallocate every node", role: .destructive) { model.resetTree() }
        } message: {
            Text("Every allocated node in \"\(model.activeSpecTitle)\" is unallocated. Undo is not available on the iPad yet.")
        }
        .confirmationDialog(
            "Convert to \(confirmConvert?.label ?? "")?",
            isPresented: Binding(get: { confirmConvert != nil }, set: { if !$0 { confirmConvert = nil } }),
            titleVisibility: .visible
        ) {
            Button("Convert") { if let v = confirmConvert { model.convertTree(to: v.id) } }
        } message: {
            Text("PoB re-maps the allocated nodes onto the \(confirmConvert?.label ?? "") tree. Nodes that no longer exist are dropped.")
        }
    }

    /// A single scrolling row of pills. On a wide screen it fits; on a narrow
    /// (portrait) one it scrolls horizontally rather than wrapping, which
    /// keeps the canvas below it as tall as possible.
    private var toolbar: some View {
        HScrollRow {
        HStack(spacing: Tok.Space.s3) {
            // Tree sets: PoB's "specs". Switch, plus manage in the same menu.
            DropPill(title: "Tree: \(model.activeSpecTitle)") {
                ForEach(model.treeSpecs, id: \.index) { spec in
                    Button {
                        model.switchSpec(spec.index)
                    } label: {
                        if spec.index == model.activeSpec { Label(spec.title, systemImage: "checkmark") } else { Text(spec.title) }
                    }
                }
                Divider()
                Button("New tree…") { naming = SpecNaming(kind: .new) }
                Button("Copy \"\(model.activeSpecTitle)\"…") { naming = SpecNaming(kind: .copy) }
                Button("Rename…") { naming = SpecNaming(kind: .rename) }
                if model.treeSpecs.count > 1 {
                    Button("Delete \"\(model.activeSpecTitle)\"", role: .destructive) { model.deleteSpec(model.activeSpec) }
                }
            }

            DropPill(title: model.treeVersionLabel) {
                ForEach(model.treeVersions, id: \.id) { v in
                    Button {
                        if v.id != model.treeVersionId { confirmConvert = v }
                    } label: {
                        if v.id == model.treeVersionId { Label(v.label, systemImage: "checkmark") } else { Text(v.label) }
                    }
                }
            }

            WellField(placeholder: "Search nodes — e.g. chill effect", text: $search)
                .frame(width: 420)
                .accessibilityLabel("Search nodes")

            // PoB compares the active tree against another spec and colours
            // the difference on the canvas.
            DropPill(title: "Compare: \(model.compareSpec == nil ? "off" : "on")") {
                Button {
                    model.setCompareSpec(nil)
                } label: {
                    if model.compareSpec == nil { Label("Off", systemImage: "checkmark") } else { Text("Off") }
                }
                Divider()
                ForEach(model.treeSpecs.filter { $0.index != model.activeSpec }, id: \.index) { spec in
                    Button {
                        model.setCompareSpec(spec.index)
                    } label: {
                        if spec.index == model.compareSpec { Label(spec.title, systemImage: "checkmark") } else { Text(spec.title) }
                    }
                }
                if model.treeSpecs.count < 2 {
                    Text("Add a second tree to compare against")
                }
            }

            DropPill(title: "Node power: \(model.nodePower.metric?.label ?? "Off")") {
                Button {
                    model.setNodePower(nil)
                } label: {
                    if model.nodePower.metric == nil { Label("Off", systemImage: "checkmark") } else { Text("Off") }
                }
                Divider()
                // PoB's own list, in PoB's order.
                ForEach(model.powerStats, id: \.stat) { m in
                    Button {
                        model.setNodePower(m)
                    } label: {
                        if model.nodePower.metric?.stat == m.stat { Label(m.label, systemImage: "checkmark") } else { Text(m.label) }
                    }
                }
            }

            Button("Find Timeless Jewel") { showTimeless = true }.buttonStyle(PillStyle())
            Button("Import / Export…") { showLinks = true }.buttonStyle(PillStyle())
            Button("Reset…") { confirmReset = true }.buttonStyle(PillStyle(danger: true))
        }
        }
    }
}

/// Which spec operation a name is being asked for.
struct SpecNaming: Identifiable {
    enum Kind { case new, copy, rename }
    let kind: Kind
    var id: String { "\(kind)" }
}

/// A one-field sheet for naming a tree set.
struct SpecNameSheet: View {
    @ObservedObject var model: BuildModel
    let naming: SpecNaming
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""

    private var title: String {
        switch naming.kind {
        case .new: return "New tree"
        case .copy: return "Copy \"\(model.activeSpecTitle)\""
        case .rename: return "Rename \"\(model.activeSpecTitle)\""
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text(title).font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            WellField(placeholder: "Name", text: $name)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Save") {
                    let n = name.trimmingCharacters(in: .whitespaces)
                    switch naming.kind {
                    case .new: model.newSpec(n)
                    case .copy: model.copySpec(model.activeSpec, as: n)
                    case .rename: model.renameSpec(model.activeSpec, to: n)
                    }
                    dismiss()
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 440)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear { if naming.kind == .rename { name = model.activeSpecTitle } }
    }
}

/// Export the tree as a pathofexile.com link, or import one.
struct TreeLinksSheet: View {
    @ObservedObject var model: BuildModel
    @Environment(\.dismiss) private var dismiss
    @State private var link = ""
    @State private var error: String?
    @State private var copied = false
    @State private var exported: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Tree link").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()

            SectionLabel(text: "Export")
            if let out = exported {
                HStack(spacing: Tok.Space.s2) {
                    Text(out).font(Tok.mono(12)).foregroundStyle(Tok.textSecondary).textLift().lineLimit(2).truncationMode(.middle)
                        .padding(Tok.Space.s3).frame(maxWidth: .infinity, alignment: .leading)
                        .background(Tok.ink100)
                        .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                    Button(copied ? "Copied" : "Copy") {
                        UIPasteboard.general.string = out
                        copied = true
                    }
                    .buttonStyle(PillStyle())
                    ShareLink(item: out) { Text("Share") }.buttonStyle(PillStyle())
                }
            } else {
                Text("Nothing to export: the tree is empty.").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
            }

            SectionLabel(text: "Import").padding(.top, Tok.Space.s2)
            Text("Paste a pathofexile.com passive tree link. It replaces the active tree.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            WellField(placeholder: "https://www.pathofexile.com/passive-skill-tree/…", text: $link)
            if let error { Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }
            HStack {
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
                Button("Import") {
                    error = model.importTreeLink(link)
                    if error == nil { dismiss() }
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 640)
        .background(Tok.ink300)
        .task(id: model.refreshCount) { exported = model.exportTreeLink() }
        .presentationBackground(Tok.ink300)
    }
}

/// The small mono status chips in the canvas's corner.
struct Chip: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(Tok.mono(13))
            .foregroundStyle(Tok.textPrimary).textLift()
            .padding(.horizontal, Tok.Space.s3)
            .frame(height: 36)
            .background(Tok.ink300.opacity(0.92))
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
    }
}

/// A square check with the cyan tick, as the canvas draws checkboxes.
struct CheckStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            HStack(spacing: Tok.Space.s2) {
                ZStack {
                    RoundedRectangle(cornerRadius: Tok.Radius.sm)
                        .fill(configuration.isOn ? Tok.cyan500 : Tok.ink100)
                    RoundedRectangle(cornerRadius: Tok.Radius.sm)
                        .stroke(configuration.isOn ? Tok.cyan400 : Tok.ink600, lineWidth: 1)
                    if configuration.isOn {
                        Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)).foregroundStyle(Tok.ink050)
                    }
                }
                .frame(width: 24, height: 24)
                configuration.label
            }
            .frame(height: Tok.Size.touchMin)
        }
        .buttonStyle(.plain)
    }
}

/// Lays children left to right, wrapping to a new line when the width runs
/// out. `HStack` cannot wrap and the canvas's toolbars rely on it.
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = layoutRows(width: proposal.width ?? .infinity, subviews: subviews)
        let height = rows.reduce(0) { $0 + $1.height } + CGFloat(max(0, rows.count - 1)) * spacing
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in layoutRows(width: bounds.width, subviews: subviews) {
            var x = bounds.minX
            for i in row.indices {
                let size = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y + (row.height - size.height) / 2), proposal: .unspecified)
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row { var indices: [Int] = []; var width: CGFloat = 0; var height: CGFloat = 0 }

    private func layoutRows(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = [Row()]
        for (i, v) in subviews.enumerated() {
            let size = v.sizeThatFits(.unspecified)
            let needed = rows[rows.count - 1].width + (rows[rows.count - 1].indices.isEmpty ? 0 : spacing) + size.width
            if needed > width, !rows[rows.count - 1].indices.isEmpty {
                rows.append(Row())
            }
            rows[rows.count - 1].indices.append(i)
            rows[rows.count - 1].width += (rows[rows.count - 1].indices.count > 1 ? spacing : 0) + size.width
            rows[rows.count - 1].height = max(rows[rows.count - 1].height, size.height)
        }
        return rows
    }
}

/// A single non-wrapping row that scrolls horizontally when it overflows,
/// the standard iPad idiom for a toolbar too wide for a narrow (portrait)
/// screen. On a wide screen the row is shorter than the viewport and does
/// not scroll, so it looks identical to a plain `HStack`.
///
/// A row that scrolls sideways gives no hint of what is past its edge, so
/// each edge with more content behind it fades out and carries a chevron;
/// both go away once that end is reached.
struct HScrollRow<Content: View>: View {
    var content: Content
    init(@ViewBuilder content: () -> Content) { self.content = content() }
    @State private var edges = ScrollEdges()

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            content
        }
        .modifier(TrackScrollEdges(edges: $edges))
        .mask {
            HStack(spacing: 0) {
                LinearGradient(colors: [.clear, .black], startPoint: .leading, endPoint: .trailing)
                    .frame(width: edges.moreBefore ? Self.fade : 0)
                Rectangle()
                LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing)
                    .frame(width: edges.moreAfter ? Self.fade : 0)
            }
        }
        .overlay(alignment: .leading) { chevron("chevron.left", shown: edges.moreBefore) }
        .overlay(alignment: .trailing) { chevron("chevron.right", shown: edges.moreAfter) }
        .animation(.easeOut(duration: 0.15), value: edges)
    }

    private static var fade: CGFloat { 44 }

    private func chevron(_ name: String, shown: Bool) -> some View {
        Image(systemName: name)
            .font(.system(size: 13, weight: .bold))
            .foregroundStyle(Tok.textSecondary)
            .padding(.horizontal, 4)
            .opacity(shown ? 1 : 0)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// Whether a horizontal scroll view has content past either edge.
struct ScrollEdges: Equatable {
    var moreBefore = false
    var moreAfter = false
}

private struct TrackScrollEdges: ViewModifier {
    @Binding var edges: ScrollEdges
    func body(content: Content) -> some View {
        if #available(iOS 18.0, *) {
            content.onScrollGeometryChange(for: ScrollEdges.self) { g in
                let x = g.contentOffset.x + g.contentInsets.leading
                return ScrollEdges(
                    moreBefore: x > 1,
                    moreAfter: x + g.containerSize.width < g.contentSize.width - 1
                )
            } action: { _, new in
                edges = new
            }
        } else {
            // iOS 17 has no scroll geometry: no hint, as before.
            content
        }
    }
}

/// The egui canvas, sized by SwiftUI rather than owning the screen.
struct EguiView: UIViewControllerRepresentable {
    /// Whether this canvas is the one `EguiBridge` talks to. The Tree tab's
    /// is; the Compare overlay's copy is not, or its arrival would replace
    /// the bridge's controller and its departure would leave it nil, which
    /// is how Tree search stopped highlighting after a visit to Compare.
    var primary = true
    func makeUIViewController(context: Context) -> PobViewController { PobViewController(primary: primary) }
    func updateUIViewController(_ controller: PobViewController, context: Context) {
        // The controller drives its own frames from the display link and the
        // Rust side owns what it draws; nothing to push.
    }
}

/// PoB's Power Report: Type, Node, the stat, Points, Per point. Filters as
/// PoB has them: masteries on or off, unallocated / allocated / all.
struct PowerReportSheet: View {
    @ObservedObject var model: BuildModel
    var onFocus: (UInt64) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var search = ""
    @State private var showMasteries = true
    @State private var which = Which.unallocated

    enum Which: String, CaseIterable, Identifiable {
        case unallocated = "Unallocated", allocated = "Allocated", all = "All"
        var id: String { rawValue }
    }

    private var rows: [PowerReportRow] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return model.nodePower.report.filter { r in
            (showMasteries || r.kind != "Mastery")
                && (which == .all || (which == .allocated) == r.allocated)
                && (q.isEmpty || r.name.lowercased().contains(q))
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                Text("Power report").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Text(model.nodePower.metric?.label ?? "").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
            }
            HStack(spacing: Tok.Space.s3) {
                WellField(placeholder: "Filter nodes", text: $search).frame(maxWidth: 320)
                Toggle(isOn: $showMasteries) {
                    Text("Show masteries").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                }
                .toggleStyle(CheckStyle())
                Picker("Show", selection: $which) {
                    ForEach(Which.allCases) { w in Text(w.rawValue).tag(w) }
                }
                .pickerStyle(.segmented)
                .frame(width: 300)
                Spacer()
                Text("\(rows.count) nodes").font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift()
            }

            VStack(spacing: 0) {
                HStack(spacing: Tok.Space.s3) {
                    SectionLabel(text: "Type").frame(width: 84, alignment: .leading)
                    SectionLabel(text: "Node").frame(maxWidth: .infinity, alignment: .leading)
                    SectionLabel(text: model.nodePower.metric?.label ?? "Power").frame(width: 120, alignment: .trailing)
                    SectionLabel(text: "Points").frame(width: 70, alignment: .trailing)
                    SectionLabel(text: "Per point").frame(width: 100, alignment: .trailing)
                    Color.clear.frame(width: 20)
                }
                .padding(.horizontal, Tok.Space.s3).frame(height: 36)
                Rectangle().fill(Tok.ink400).frame(height: 1)
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(Array(rows.prefix(500).enumerated()), id: \.offset) { _, r in
                            Button {
                                dismiss()
                                onFocus(r.id)
                            } label: {
                                HStack(spacing: Tok.Space.s3) {
                                    Text(r.kind).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().frame(width: 84, alignment: .leading)
                                    Text(r.name.isEmpty ? "#\(r.id)" : r.name)
                                        .font(Tok.ui(15, r.allocated ? .regular : .medium))
                                        .foregroundStyle(r.allocated ? Tok.textTertiary : Tok.textPrimary).textLift()
                                        .lineLimit(1).truncationMode(.tail)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                    Text(r.powerText).font(Tok.mono(15)).foregroundStyle(Self.tint(r.power, lowerIsBetter: r.lowerIsBetter)).frame(width: 120, alignment: .trailing)
                                    Text(r.pointsText).font(Tok.mono(15)).foregroundStyle(Tok.textSecondary).textLift().frame(width: 70, alignment: .trailing)
                                    Text(r.perPointText).font(Tok.mono(15)).foregroundStyle(Tok.textSecondary).textLift().frame(width: 100, alignment: .trailing)
                                    Image(systemName: "scope").font(.system(size: 12)).foregroundStyle(Tok.cyan700).frame(width: 20)
                                }
                                .padding(.horizontal, Tok.Space.s3)
                                .frame(height: 40)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("\(r.kind) \(r.name), \(r.powerText), \(r.pointsText) points. Show on tree")
                            Rectangle().fill(Tok.ink400).frame(height: 1)
                        }
                        if rows.count > 500 {
                            Text("Showing the top 500. Filter to see the rest.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s3)
                        }
                    }
                }
            }
            .plate()
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 760, minHeight: 640)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }

    /// PoB's colouring: green when the change is in the good direction.
    static func tint(_ v: Double, lowerIsBetter: Bool) -> Color {
        if v == 0 { return Tok.textSecondary }
        return (v > 0) != lowerIsBetter ? Tok.ok : Tok.coral300
    }
}

/// A jewel socket tapped on the tree.
struct JewelSocketTarget: Identifiable {
    let node: UInt64
    let slot: String
    var id: UInt64 { node }
}

struct MasteryTarget: Identifiable {
    let node: UInt64
    let effects: [MasteryEffect]
    let selected: UInt32?
    var id: UInt64 { node }
}

/// Pick the effect for a mastery passive, as PoB does on a mastery click.
/// Choosing one allocates the node if it is not already, and switches the
/// effect if it is. The one in use is ticked.
struct MasteryPickerSheet: View {
    @ObservedObject var model: BuildModel
    let target: MasteryTarget
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Mastery").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                    Text(target.selected == nil ? "Choose an effect" : "Choose an effect (changes the current one)")
                        .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
            }
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(target.effects, id: \.id) { effect in
                        Button {
                            model.setMasteryEffect(target.node, effect.id)
                            dismiss()
                        } label: {
                            HStack(alignment: .top, spacing: Tok.Space.s3) {
                                Image(systemName: effect.id == target.selected ? "checkmark.circle.fill" : "circle")
                                    .font(.system(size: 15))
                                    .foregroundStyle(effect.id == target.selected ? Tok.ok : Tok.textTertiary).textLift()
                                    .padding(.top, 2)
                                Text(effect.label)
                                    .font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            .padding(Tok.Space.s3)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .overlay(alignment: .bottom) { Rectangle().fill(Tok.ink400).frame(height: 1) }
                    }
                }
            }
            .plate()
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 460, minHeight: 420)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }
}

/// Fill a socket from the tree: the jewels this build owns that fit it, by
/// PoB's own rules (cluster sizes, abyss, ordinary), with a search box and
/// the one already in it ticked. Replaces the trip to the Items tab to
/// find the slot and match names.
struct JewelPickerSheet: View {
    @ObservedObject var model: BuildModel
    let target: JewelSocketTarget
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    @State private var candidates: [Item] = []
    private var allocated: Bool { model.nodeAllocated(target.node) }
    /// A Sinister socket the Voices jewel has not granted. No point buys it,
    /// so a jewel moved here would leave a socket where it works for one
    /// where it does nothing.
    private var closed: Bool { !allocated && model.socketGrantedOnly(target.node) }
    /// The item list knows tree sockets in `wornIn`; the candidate list
    /// does not, so the socket's current jewel comes from there.
    private var worn: Item? { model.items.first { $0.wornIn.contains(target.slot) } }
    private var shown: [Item] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return candidates.filter { q.isEmpty || $0.name.lowercased().contains(q) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Jewel socket").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                    Text(worn.map { "Holds \($0.name)" } ?? "Empty").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                    if closed {
                        Text("Granted by the Voices jewel, not bought with a point. This build's Voices does not grant this one, so a jewel here does nothing.")
                            .font(Tok.ui(13)).foregroundStyle(Tok.coral300)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if !allocated {
                        // A jewel in an unallocated socket does nothing and
                        // draws nothing; say so, and allocate on pick.
                        Text("Socket not allocated. Picking a jewel allocates it (1 point).").font(Tok.ui(13)).foregroundStyle(Tok.coral300)
                    }
                }
                Spacer()
                if worn != nil {
                    Button("Remove jewel") { model.equip(0, in: target.slot); EguiBridge.unpin(); dismiss() }.buttonStyle(PillStyle(danger: true))
                }
                Button("Done") { dismiss() }.buttonStyle(PillStyle())
            }
            if !closed {
                WellField(placeholder: "Search your jewels", text: $query)
            }
            if closed {
                Text(worn == nil
                     ? "Nothing to pick here. A Voices that grants more sockets opens it."
                     : "Remove the jewel to free it for a socket where it works.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    .frame(maxWidth: .infinity, minHeight: 120, alignment: .center)
            } else if candidates.isEmpty {
                Text("No jewel in this build fits here. Add one on the Items tab, or Find a Timeless Jewel for this socket.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    .frame(maxWidth: .infinity, minHeight: 120, alignment: .center)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(shown, id: \.id) { item in
                            let isWorn = item.id == worn?.id
                            Button {
                                if !isWorn { model.equip(item.id, in: target.slot) }
                                if !allocated { model.allocateNode(target.node) }
                                // Acting on the socket is the end of the tap;
                                // leaving it pinned kept its radius discs up.
                                EguiBridge.unpin()
                                dismiss()
                            } label: {
                                HStack(spacing: Tok.Space.s3) {
                                    Image(systemName: isWorn ? "checkmark.circle.fill" : "circle")
                                        .foregroundStyle(isWorn ? Tok.cyan300 : Tok.textTertiary).textLift()
                                    Text(item.name).font(Tok.ui(17)).foregroundStyle(Color(rrggbb: item.colour)).lineLimit(1)
                                    Spacer()
                                    if let elsewhere = model.items.first(where: { $0.id == item.id })?.wornIn.first, !isWorn {
                                        Text("in \(elsewhere)").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                                    }
                                }
                                .padding(.horizontal, Tok.Space.s3).frame(height: Tok.Size.touchMin)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("\(item.name)\(isWorn ? ", socketed here" : "")")
                            Rectangle().fill(Tok.ink400).frame(height: 1)
                        }
                    }
                }
                .plate()
                .frame(maxHeight: 420)
            }
        }
        .padding(Tok.Space.s6)
        // From the top: a sheet with little in it otherwise floats its few
        // lines in the middle of an empty panel.
        .frame(minWidth: 520, maxHeight: .infinity, alignment: .top)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .task(id: model.refreshCount) { candidates = model.candidates(for: target.slot) }
    }
}
