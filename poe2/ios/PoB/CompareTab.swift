import SwiftUI

/// PoB's Compare tab, Summary: this build against another build in the
/// library, a pasted code, or another tree set of this build. Rows,
/// formatting and the better/worse call are PoB's.
struct CompareTab: View {
    @ObservedObject var model: BuildModel
    @ObservedObject var library: BuildLibrary
    private var source: CompareState.Source? {
        get { model.compare.source }
        nonmutating set { model.compare.source = newValue }
    }
    private var rows: [CompareRow] {
        get { model.compare.rows }
        nonmutating set { model.compare.rows = newValue }
    }
    @State private var working = false
    @State private var error: String?
    @State private var showPaste = false
    @State private var pasted = ""
    @State private var search = ""
    @State private var onlyChanged = false
    @State private var page = Page.summary
    /// The other build's XML, read for the Items / Skills / Config pages.
    private var other: OtherBuild? {
        get { model.compare.other }
        nonmutating set { model.compare.other = newValue }
    }

    enum Page: String, CaseIterable, Identifiable {
        case summary = "Summary", tree = "Tree", skills = "Skills", items = "Items", calcs = "Calcs", config = "Config"
        var id: String { rawValue }
    }
    /// The other build's XML, for the Calcs page's temporary load.
    private var otherXML: String? {
        get { model.compare.otherXML }
        nonmutating set { model.compare.otherXML = newValue }
    }
    var goToTree: () -> Void = {}

    private var visible: [CompareRow] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        var out: [CompareRow] = []
        for r in rows {
            if r.isGap { if let last = out.last, !last.isGap { out.append(r) }; continue }
            if onlyChanged && r.better == 0 { continue }
            if !q.isEmpty && !r.label.lowercased().contains(q) { continue }
            out.append(r)
        }
        while out.last?.isGap == true { out.removeLast() }
        return out
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            FlowRow(spacing: Tok.Space.s3) {
                Text("Compare with").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                DropPill(title: source?.name ?? "Choose…") {
                    let others = library.builds.filter { $0.url != model.currentURL }
                    if !others.isEmpty {
                        Section("Builds") {
                            ForEach(others) { b in Button(b.name) { pick(.file(b)) } }
                        }
                    }
                    let specs = model.treeSpecs.filter { $0.index != model.activeSpec }
                    if !specs.isEmpty {
                        Section("Tree sets of this build") {
                            ForEach(specs, id: \.index) { s in Button(s.title) { pick(.spec(s.index, title: s.title)) } }
                        }
                    }
                    // Copies taken before an import overwrote this build:
                    // the "where did my DPS go" answer, newest first.
                    let snaps = library.snapshots(of: model.currentURL)
                    if !snaps.isEmpty {
                        Section("This build before an import") {
                            ForEach(snaps) { b in Button(b.name) { pick(.file(b)) } }
                        }
                    }
                    Divider()
                    Button("Paste a code…") { showPaste = true }
                }
                if source != nil {
                    Button("Re-compare") { if let s = source { pick(s) } }.buttonStyle(PillStyle())
                    Button("Remove") { source = nil; rows = []; other = nil; otherXML = nil; page = .summary; model.clearTreeOverlay() }.buttonStyle(PillStyle(danger: true))
                }
                Spacer()
            }

            if let error { Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }

            if source == nil {
                VStack(alignment: .leading, spacing: Tok.Space.s2) {
                    Text("Pick another build from your library, another tree set of this build, a copy of this build from before an import, or paste a code.")
                        .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    Text("PoB runs the other build through the same calculation, so a comparison takes a moment.")
                        .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                .padding(Tok.Space.s4).frame(maxWidth: .infinity, alignment: .leading).plate()
            } else {
                HStack(spacing: 2) {
                    ForEach(Page.allCases) { p in
                        Button(p.rawValue) { page = p }.buttonStyle(PillStyle(primary: page == p))
                            .disabled(p != .summary && other == nil)
                    }
                    Spacer()
                    if working { ProgressView().tint(Tok.cyan500) }
                }
                switch page {
                case .summary:
                    HStack(spacing: Tok.Space.s3) {
                        WellField(placeholder: "Filter stats", text: $search).frame(maxWidth: 320)
                        Toggle(isOn: $onlyChanged) {
                            Text("Only what changed").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                        }
                        .toggleStyle(CheckStyle())
                        Spacer()
                    }
                    summary
                case .items:
                    if let o = other { CompareItemsPage(model: model, other: o, otherName: source?.name ?? "Compared") }
                case .skills:
                    if let o = other { CompareSkillsPage(model: model, other: o, otherName: source?.name ?? "Compared") }
                case .config:
                    if let o = other { CompareConfigPage(model: model, other: o, otherName: source?.name ?? "Compared") }
                case .calcs:
                    if let x = otherXML { CompareCalcsPage(model: model, otherXML: x, otherName: source?.name ?? "Compared").id(x.hashValue) }
                case .tree:
                    if let o = other { CompareTreePage(model: model, other: o, otherName: source?.name ?? "Compared", goToTree: goToTree) }
                }
            }
        }
        .padding(Tok.Space.s4)
        .sheet(isPresented: $showPaste) { pasteSheet }
    }

    /// The Stat column stays put; the value columns scroll as one, so a
    /// long build name or a big number never pushes the labels away.
    private var summary: some View {
        let list = visible
        return ScrollView {
            HStack(alignment: .top, spacing: 0) {
                VStack(spacing: 0) {
                    header(Text("Stat"), width: nil, align: .leading)
                    ForEach(Array(list.enumerated()), id: \.offset) { _, r in
                        if r.isGap {
                            Color.clear.frame(height: 14)
                        } else {
                            Text(r.label).font(Tok.ui(15))
                                .foregroundStyle(r.colour.isEmpty ? Tok.textSecondary : Color(rrggbb: r.colour)).textLift()
                                .lineLimit(1).frame(height: 40).frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.leading, Tok.Space.s4)
                        }
                    }
                }
                .frame(width: 240)
                .background(Tok.ink300)

                ScrollView(.horizontal, showsIndicators: true) {
                    VStack(spacing: 0) {
                        HStack(spacing: 0) {
                            header(Text(model.buildName.isEmpty ? "This build" : model.buildName).foregroundStyle(Tok.ok), width: 180, align: .trailing)
                            header(Text(source?.name ?? "Compared").foregroundStyle(Tok.violet400), width: 180, align: .trailing)
                            header(Text("Difference"), width: 150, align: .trailing)
                            header(Text("Change"), width: 110, align: .trailing)
                        }
                        ForEach(Array(list.enumerated()), id: \.offset) { _, r in
                            if r.isGap {
                                Color.clear.frame(height: 14)
                            } else {
                                HStack(spacing: 0) {
                                    cell(r.a, Tok.textPrimary, 180)
                                    cell(r.c, Tok.textPrimary, 180)
                                    cell(r.diff, tint(r), 150)
                                    cell(r.percent == 0 ? "" : String(format: "%@%.1f%%", r.percent > 0 ? "+" : "", r.percent), tint(r), 110)
                                }
                                .accessibilityElement(children: .combine)
                                .accessibilityLabel("\(r.label): \(r.a) versus \(r.c), \(r.diff.isEmpty ? "no change" : r.diff)")
                            }
                        }
                    }
                }
            }
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
        }
    }

    private func header(_ t: Text, width: CGFloat?, align: Alignment) -> some View {
        t.font(Tok.label).tracking(1.6)
            .lineLimit(1).truncationMode(.tail)
            .frame(width: width, height: 44, alignment: align)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: align)
            .padding(.horizontal, Tok.Space.s4)
            .background(Tok.ink200)
            .foregroundStyle(Tok.ice700)
    }

    private func cell(_ v: String, _ c: Color, _ w: CGFloat) -> some View {
        Text(v).font(Tok.mono(15)).foregroundStyle(c).lineLimit(1)
            .frame(width: w, height: 40, alignment: .trailing).padding(.horizontal, Tok.Space.s4)
    }

    private func tint(_ r: CompareRow) -> Color {
        r.better > 0 ? Tok.ok : r.better < 0 ? Tok.coral300 : Tok.textTertiary
    }

    private func pick(_ s: CompareState.Source) {
        source = s
        error = nil
        working = true
        // The comparison runs on the engine's own thread; the spinner
        // spins and the menu's dismissal animation finishes under it.
        Task {
            switch s {
            case .file(let f):
                if let xml = try? String(contentsOf: f.url, encoding: .utf8) {
                    let out = await model.compare(withXML: xml)
                    rows = out.rows; error = out.error
                    other = OtherBuild.parse(xml); otherXML = xml
                } else { rows = []; error = "Could not read \(f.name)." }
            case .code(let xml, _):
                let out = await model.compare(withXML: xml)
                rows = out.rows; error = out.error
                other = OtherBuild.parse(xml); otherXML = xml
            case .spec(let i, _):
                let out = await model.compare(withSpec: i)
                rows = out.rows; error = out.error
                // Another tree set of this build: the items, skills and
                // config are this build's own.
                otherXML = model.currentXML()
                other = otherXML.flatMap { OtherBuild.parse($0) }
            }
            if rows.isEmpty, error == nil { error = "PoB could not compare against that." }
            working = false
        }
    }

    private var pasteSheet: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Compare with a code").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            Text("Paste a PoB share code or raw XML. It is compared against, not added to the library.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            TextEditor(text: $pasted)
                .font(Tok.mono(13)).scrollContentBackground(.hidden).padding(Tok.Space.s2)
                .frame(minHeight: 200).background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            HStack {
                Spacer()
                Button("Cancel") { showPaste = false }.buttonStyle(PillStyle())
                Button("Compare") {
                    let t = pasted.trimmingCharacters(in: .whitespacesAndNewlines)
                    showPaste = false
                    // A share code is base64 of deflated XML; the engine's
                    // importer decodes it, so decode through it by loading
                    // into a scratch string via pob-import on the Rust side
                    // is not exposed. Raw XML compares directly; a code is
                    // decoded here.
                    if t.hasPrefix("<") {
                        pick(.code(t, name: "Pasted build"))
                    } else if let xml = ShareCode.decode(t) {
                        pick(.code(xml, name: "Pasted build"))
                    } else {
                        error = "That does not look like a PoB code or XML."
                    }
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(pasted.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Tok.Space.s6).frame(minWidth: 560)
        .background(Tok.ink300).presentationBackground(Tok.ink300)
    }
}

/// PoB share codes: base64url of zlib-deflated XML.
enum ShareCode {
    static func decode(_ code: String) -> String? {
        var s = code.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
            .filter { !$0.isWhitespace }
        while s.count % 4 != 0 { s += "=" }
        guard let data = Data(base64Encoded: s) else { return nil }
        // zlib stream: skip the 2-byte header and inflate the deflate body.
        guard data.count > 2, let raw = try? (data.dropFirst(2) as NSData).decompressed(using: .zlib) as Data else { return nil }
        return String(data: raw, encoding: .utf8)
    }
}
