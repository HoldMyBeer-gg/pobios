import SwiftUI

/// PoB's Compare › Calcs: each section with both builds' values side by
/// side, optionally only the rows that differ.
struct CompareCalcsPage: View {
    @ObservedObject var model: BuildModel
    let otherXML: String
    let otherName: String
    @State private var theirs: [CalcSection] = []
    @State private var loaded = false
    @AppStorage("compare.calcs.onlyDiff") private var onlyDiff = true

    private struct Pair: Identifiable {
        let mine: CalcSection
        let theirs: CalcSection?
        var id: String { mine.stableId }
    }

    private var pairs: [Pair] {
        model.calcSections.map { m in Pair(mine: m, theirs: theirs.first { $0.stableId == m.stableId }) }
    }

    var body: some View {
        GeometryReader { geo in
            let cols = geo.size.width >= 1000 ? 2 : 1
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                HStack {
                    Toggle(isOn: $onlyDiff) { Text("Show only differences").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                    Spacer()
                    if !loaded { ProgressView().tint(Tok.cyan500) }
                }
                ScrollView {
                    let ps = pairs.filter { p in !onlyDiff || hasDiff(p) }
                    let buckets: [[Pair]] = {
                        var b = Array(repeating: [Pair](), count: cols); var h = Array(repeating: 0, count: cols)
                        for p in ps { let i = h.indices.min { h[$0] < h[$1] } ?? 0; b[i].append(p); h[i] += p.mine.rows.count + 3 }
                        return b
                    }()
                    HStack(alignment: .top, spacing: Tok.Space.s4) {
                        ForEach(0..<cols, id: \.self) { i in
                            VStack(spacing: Tok.Space.s4) { ForEach(buckets[i]) { card($0) } }.frame(maxWidth: .infinity, alignment: .top)
                        }
                    }
                }
            }
        }
        .onAppear {
            guard !loaded else { return }
            // The other build is loaded in place of this one for the read,
            // on the engine's thread; the page draws meanwhile.
            Task {
                theirs = await model.calcSections(for: otherXML)
                loaded = true
            }
        }
    }

    private func hasDiff(_ p: Pair) -> Bool {
        guard let t = p.theirs else { return true }
        for r in p.mine.rows where !r.isHeader {
            let o = t.rows.first { $0.label == r.label }
            if o == nil || o!.cells.map(\.value) != r.cells.map(\.value) { return true }
        }
        return false
    }

    private func card(_ p: Pair) -> some View {
        let tint = p.mine.colour.isEmpty ? Tok.ice500 : Color(rrggbb: p.mine.colour)
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Tok.Space.s2) {
                Text(p.mine.label).font(Tok.display(17)).foregroundStyle(tint).lineLimit(1)
                Spacer()
                if !p.mine.extra.isEmpty {
                    Text(p.mine.extra).font(Tok.mono(15, .bold)).foregroundStyle(Tok.ok)
                    Text("|").foregroundStyle(Tok.textTertiary).textLift()
                    Text(p.theirs?.extra ?? "—").font(Tok.mono(15, .bold)).foregroundStyle(Tok.violet400)
                }
            }
            .padding(Tok.Space.s3)
            Rectangle().fill(Tok.ink400).frame(height: 1)
            VStack(spacing: 0) {
                ForEach(Array(p.mine.rows.enumerated()), id: \.offset) { _, r in
                    if !r.isHeader {
                        let o = p.theirs?.rows.first { $0.label == r.label }
                        let mineText = r.cells.map(\.value).filter { !$0.isEmpty }.joined(separator: " · ")
                        let theirText = o?.cells.map(\.value).filter { !$0.isEmpty }.joined(separator: " · ") ?? "—"
                        if !onlyDiff || mineText != theirText {
                            HStack(spacing: Tok.Space.s3) {
                                Text(r.label).font(Tok.ui(14)).foregroundStyle(Tok.textSecondary).textLift().lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                                Text(mineText.isEmpty ? "–" : mineText).font(Tok.mono(13)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1).frame(width: 150, alignment: .trailing)
                                Text(theirText).font(Tok.mono(13)).foregroundStyle(mineText == theirText ? Tok.textTertiary : Tok.textPrimary).textLift().lineLimit(1).frame(width: 150, alignment: .trailing)
                            }
                            .padding(.horizontal, Tok.Space.s3).padding(.vertical, 5)
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
            }
            .padding(.vertical, Tok.Space.s1)
        }
        .background(Tok.ink300)
        .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
        .overlay(alignment: .leading) { Rectangle().fill(tint.opacity(0.7)).frame(width: 2) }
        .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
    }
}

/// PoB's Compare › Tree: the other build's tree sets, an overlay on the
/// Tree tab's canvas, and Copy tree / Copy and use.
struct CompareTreePage: View {
    @ObservedObject var model: BuildModel
    let other: OtherBuild
    let otherName: String
    var goToTree: () -> Void
    @State private var specIndex = 0
    @State private var error: String?

    private var spec: OtherBuild.Spec? { other.specs.indices.contains(specIndex) ? other.specs[specIndex] : nil }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            HStack(alignment: .top, spacing: Tok.Space.s4) {
                VStack(alignment: .leading, spacing: Tok.Space.s2) {
                    Text(model.buildName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.ok)
                    Text("\(model.activeSpecTitle) · \(model.treeVersionLabel) · \(model.info.pointsUsed) points")
                        .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                VStack(alignment: .leading, spacing: Tok.Space.s2) {
                    Text(otherName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.violet400)
                    DropPill(title: spec.map { "\($0.title) · \($0.version.replacingOccurrences(of: "_", with: ".")) · \($0.nodes.count) nodes" } ?? "No tree sets") {
                        ForEach(Array(other.specs.enumerated()), id: \.offset) { i, s in
                            Button("\(s.title) (\(s.nodes.count) nodes)") { specIndex = i }
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            let overlayOn = model.compareOverlaySpec != nil
            FlowRow(spacing: Tok.Space.s3) {
                Button(overlayOn ? "Hide overlay" : "Overlay comparison") {
                    if overlayOn { model.clearTreeOverlay() } else { overlay() }
                }
                .buttonStyle(PillStyle(primary: !overlayOn))
                .disabled(spec?.url == nil)
                Button("Copy tree") { copy(use: false) }.buttonStyle(PillStyle()).disabled(spec?.url == nil)
                Button("Copy and use") { copy(use: true) }.buttonStyle(PillStyle()).disabled(spec?.url == nil)
            }
            if spec?.url == nil, spec != nil {
                Text("That tree set has no passive tree link saved in the build, so it cannot be imported. Open the build in PoB and save it once to add one.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.coral300)
            }
            if let error { Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }

            // The same canvas as the Tree tab, drawing the overlay: green
            // nodes are only in the other build, red only here.
            ZStack(alignment: .bottomLeading) {
                EguiView(primary: false)
                    .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.lg))
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.lg).stroke(Tok.ink500, lineWidth: 1))
                HStack(spacing: Tok.Space.s2) {
                    Chip("Green: only in \(otherName)")
                    Chip("Red: only here")
                }
                .padding(Tok.Space.s4)
                .allowsHitTesting(false)
            }
        }
        .onAppear {
            // PoB opens this page with the overlay on...
            if model.compareOverlaySpec == nil, spec?.url != nil { overlay() }
        }
        // ...and the overlay belongs to this page: PoB's Tree tab has its
        // own compare, against the build's own sets, and never shows this one.
        .onDisappear { model.clearTreeOverlay() }
        .onChange(of: specIndex) { _, _ in if spec?.url != nil { overlay() } }
    }

    private func overlay() {
        guard let s = spec, let url = s.url else { return }
        error = model.overlayTree(link: url, title: "\(otherName) · \(s.title)")
    }

    private func copy(use: Bool) {
        guard let s = spec, let url = s.url else { return }
        error = model.copyTree(link: url, title: "\(otherName) · \(s.title)", use: use)
    }
}
