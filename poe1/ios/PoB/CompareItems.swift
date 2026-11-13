import SwiftUI

/// PoB's Compare › Items: every slot side by side, and in expanded mode
/// both items' mods with PoB's + / - / > / < markers and headline stats.
struct CompareItemsPage: View {
    @ObservedObject var model: BuildModel
    let other: OtherBuild
    let otherName: String
    @AppStorage("compare.items.expanded") private var expanded = false

    private struct Row: Identifiable {
        let slot: String
        let mine: ItemDetail?
        let theirs: ItemDetail?
        /// The other build's item text, for the equip preview.
        var theirText: String?
        var id: String { slot }
        var status: String {
            switch (mine, theirs) {
            case (nil, nil): return "both empty"
            case (let a?, let b?): return a.name == b.name && a.base == b.base ? "match" : "different"
            default: return "different"
            }
        }
    }

    private var rows: [Row] {
        let theirSlots = other.activeSlots
        return model.equipment.map { s in
            let text = theirSlots[s.name].flatMap { other.items[$0] }
            let theirs = text.flatMap { model.parseItem($0) }
            return Row(slot: s.name, mine: model.itemDetail(in: s.name), theirs: theirs, theirText: text)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack(spacing: Tok.Space.s4) {
                Toggle(isOn: $expanded) {
                    Text("Expanded mode").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                }
                .toggleStyle(CheckStyle())
                Spacer()
                Text(model.buildName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.ok).frame(maxWidth: .infinity, alignment: .leading)
                Text(otherName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.violet400).frame(maxWidth: .infinity, alignment: .leading)
            }
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(rows) { r in
                        if expanded { expandedRow(r) } else { compactRow(r) }
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                }
                .plate()
            }
        }
    }

    private func compactRow(_ r: Row) -> some View {
        HStack(spacing: Tok.Space.s3) {
            Text(r.slot.uppercased()).font(Tok.label).tracking(1.4).foregroundStyle(Tok.ice700).frame(width: 110, alignment: .leading)
            ItemPeek(detail: r.mine, preview: nil) { name(r.mine) }
            Text("(\(r.status))").font(Tok.ui(13)).foregroundStyle(r.status == "match" ? Tok.ok : r.status == "different" ? Tok.coral300 : Tok.textTertiary).textLift()
                .frame(width: 96)
            ItemPeek(detail: r.theirs, preview: { r.theirText.flatMap { model.itemPreview(text: $0, in: r.slot) } }) { name(r.theirs) }
        }
        .padding(.horizontal, Tok.Space.s4).frame(height: 48)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(r.slot): \(r.mine?.name ?? "empty") versus \(r.theirs?.name ?? "empty"), \(r.status)")
    }

    private func name(_ d: ItemDetail?) -> some View {
        Text(d.map { $0.base.isEmpty || $0.base == $0.name ? $0.name : "\($0.name), \($0.base)" } ?? "(empty)")
            .font(Tok.ui(15)).foregroundStyle(d.map { Color(rrggbb: $0.colour) } ?? Tok.textTertiary).textLift()
            .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
    }

    private func expandedRow(_ r: Row) -> some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            HStack(spacing: Tok.Space.s2) {
                Text(r.slot).font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                Text("(\(r.status))").font(Tok.ui(13)).foregroundStyle(r.status == "match" ? Tok.ok : r.status == "different" ? Tok.coral300 : Tok.textTertiary).textLift()
            }
            HStack(alignment: .top, spacing: Tok.Space.s4) {
                ItemPeek(detail: r.mine, preview: nil) { side(r.mine, against: r.theirs) }
                ItemPeek(detail: r.theirs, preview: { r.theirText.flatMap { model.itemPreview(text: $0, in: r.slot) } }) { side(r.theirs, against: r.mine) }
            }
        }
        .padding(Tok.Space.s4)
    }

    /// One item's card with its mods marked against the other side's.
    private func side(_ d: ItemDetail?, against o: ItemDetail?) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            if let d {
                Text(d.base.isEmpty || d.base == d.name ? d.name : "\(d.name), \(d.base)")
                    .font(Tok.ui(17)).foregroundStyle(Color(rrggbb: d.colour))
                ForEach(Array(d.stats.enumerated()), id: \.offset) { _, s in
                    HStack(spacing: 4) {
                        Text("\(s.name):").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        Text(s.value).font(Tok.mono(13)).foregroundStyle(Tok.socketBlue)
                    }
                }
                ForEach(Array(d.mods.enumerated()), id: \.offset) { _, m in
                    let mark = Self.mark(m.text, against: o?.mods.map(\.text) ?? [])
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(mark.symbol).font(Tok.mono(13, .bold)).foregroundStyle(mark.colour).frame(width: 12)
                        Text(m.text).font(Tok.ui(13)).foregroundStyle(m.unsupported ? Tok.coral300 : m.kind == "implicit" ? Tok.textSecondary : Tok.socketBlue.opacity(0.9)).textLift()
                    }
                }
            } else {
                Text("(empty)").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// PoB's markers: `+` a mod the other item lacks, `>` / `<` the same
    /// mod with a higher / lower value, nothing when identical.
    static func mark(_ mod: String, against others: [String]) -> (symbol: String, colour: Color) {
        func shape(_ s: String) -> String { s.replacingOccurrences(of: #"[-+]?\d+(\.\d+)?"#, with: "#", options: .regularExpression) }
        func nums(_ s: String) -> [Double] {
            let re = try! NSRegularExpression(pattern: #"[-+]?\d+(\.\d+)?"#)
            return re.matches(in: s, range: NSRange(s.startIndex..., in: s)).compactMap { Range($0.range, in: s).flatMap { Double(s[$0]) } }
        }
        if others.contains(mod) { return ("", .clear) }
        let mine = shape(mod)
        guard let twin = others.first(where: { shape($0) == mine }) else { return ("+", Tok.ok) }
        let a = nums(mod).first ?? 0, b = nums(twin).first ?? 0
        if a > b { return (">", Tok.ok) }
        if a < b { return ("<", Tok.coral300) }
        return ("", .clear)
    }
}

/// Tap for the item's card, and PoB's "Equipping changes" when there is
/// a preview to compute (another build's item against this build).
struct ItemPeek<Label: View>: View {
    let detail: ItemDetail?
    var preview: (() -> ItemPreview?)?
    @ViewBuilder var label: Label
    @State private var open = false

    var body: some View {
        Button { if detail != nil { open = true } } label: { label.contentShape(Rectangle()) }
            .buttonStyle(.plain)
            .disabled(detail == nil)
            .popover(isPresented: $open) {
                ScrollView {
                    VStack(alignment: .leading, spacing: Tok.Space.s3) {
                        if let d = detail { ItemCard(detail: d) }
                        if let p = preview?() {
                            ChangesList(preview: p).padding(Tok.Space.s3).frame(maxWidth: .infinity, alignment: .leading).plate()
                        }
                    }
                    .padding(Tok.Space.s4)
                }
                .frame(width: 420).frame(minHeight: 160, maxHeight: 640)
                .presentationBackground(Tok.ink300)
            }
    }
}
