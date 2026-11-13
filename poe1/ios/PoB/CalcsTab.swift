import SwiftUI

/// PoB's Calcs tab: every section PoB shows for the selected skill, as
/// cards in PoB's three columns (offence, resources, defence), collapsing
/// to two or one as the width shrinks. Values are PoB's, filtered by PoB's
/// own CheckFlag, and tapping a value opens PoB's breakdown of it.
struct CalcsTab: View {
    @ObservedObject var model: BuildModel
    @State private var search = ""
    @AppStorage("calcs.full") private var full = false
    @State private var breakdown: BreakdownTarget?

    /// The canvas's "essentials": the eight sections most builds read.
    static let essentials: Set<String> = ["HitDamage", "Dot", "Speed", "Crit", "Life", "Mana", "EnergyShield", "DamageTaken", "EleAilments", "Resist"]

    private var sections: [CalcSection] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return model.calcSections.filter { s in
            (full || Self.essentials.contains(s.id))
                && (q.isEmpty || s.label.lowercased().contains(q) || s.rows.contains { $0.label.lowercased().contains(q) })
        }
    }

    var body: some View {
        GeometryReader { geo in
            let cols = geo.size.width >= 1100 ? 3 : geo.size.width >= 720 ? 2 : 1
            ScrollView {
                VStack(alignment: .leading, spacing: Tok.Space.s4) {
                    if let inputs = model.calcsInputs {
                        CalcsHeader(model: model, inputs: inputs)
                    }

                    HStack(spacing: Tok.Space.s3) {
                        WellField(placeholder: "Search calculations — e.g. crit, leech, recharge", text: $search)
                        Spacer()
                        Toggle(isOn: $full) {
                            Text("Show the full breakdown").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                        }
                        .tint(Tok.cyan500)
                        .fixedSize()
                    }
                    Text(full ? "Every section PoB shows for this skill." : "Showing the essentials. Flip the switch for every section.")
                        .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()

                    columns(cols)
                }
                .padding(Tok.Space.s4)
            }
        }
        .sheet(item: $breakdown) { t in BreakdownSheet(model: model, target: t) }
    }

    /// PoB's columns when there is room for three; otherwise sections fill
    /// columns in order, shortest first, so nothing towers.
    @ViewBuilder private func columns(_ n: Int) -> some View {
        let secs = sections
        let buckets: [[CalcSection]] = {
            if n == 3 {
                return [1, 2, 3].map { c in secs.filter { Int($0.column) == c } }
            }
            var b = Array(repeating: [CalcSection](), count: n)
            var h = Array(repeating: 0, count: n)
            for s in secs {
                let i = h.indices.min { h[$0] < h[$1] } ?? 0
                b[i].append(s); h[i] += s.rows.count + 3
            }
            return b
        }()
        HStack(alignment: .top, spacing: Tok.Space.s4) {
            ForEach(0..<n, id: \.self) { i in
                VStack(spacing: Tok.Space.s4) {
                    ForEach(buckets[i], id: \.stableId) { s in
                        CalcCard(section: s, search: search) { row, cell in
                            breakdown = BreakdownTarget(section: s, row: row, cell: cell)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .top)
            }
        }
    }
}

extension CalcSection {
    /// Sections share ids (Flasks has five subsections); the label makes it unique.
    var stableId: String { "\(id)/\(label)" }
}

struct BreakdownTarget: Identifiable {
    let section: CalcSection
    let row: Int
    let cell: Int
    var id: String { "\(section.stableId)/\(row)/\(cell)" }
}

/// PoB's "View Skill Details": the tab's own skill selection and mode.
struct CalcsHeader: View {
    @ObservedObject var model: BuildModel
    let inputs: CalcsInputs

    private static let modes: [(String, String)] = [("UNBUFFED", "Unbuffed"), ("BUFFED", "Buffed"), ("COMBAT", "In Combat"), ("EFFECTIVE", "Effective DPS")]

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            FlowRow(spacing: Tok.Space.s3) {
                SectionLabel(text: "View skill").frame(height: Tok.Size.touchMin)
                picker(inputs.groups.indices.contains(Int(inputs.groupIndex) - 1) ? inputs.groups[Int(inputs.groupIndex) - 1] : "Skill", inputs.groups, Int(inputs.groupIndex)) { model.setCalcsGroup($0) }
                let c = inputs.controls
                if c.activeSkills.count > 1 {
                    picker(c.activeSkills[Int(c.activeIndex) - 1], c.activeSkills, Int(c.activeIndex)) { model.setCalcsActiveSkill($0) }
                }
                if c.parts.count > 1 {
                    picker("Part: " + (c.parts.indices.contains(Int(c.partIndex) - 1) ? c.parts[Int(c.partIndex) - 1] : "—"), c.parts, Int(c.partIndex)) { model.setCalcsPart($0) }
                }
                if let st = c.stages { counter("Stages", st) { model.setCalcsStages($0) } }
                if let m = c.mines { counter("Mines", m) { model.setCalcsMines($0) } }
                if !c.minions.isEmpty {
                    picker("Minion: " + (c.minions.indices.contains(Int(c.minionIndex) - 1) ? c.minions[Int(c.minionIndex) - 1] : "—"), c.minions, Int(c.minionIndex)) { i in
                        let v = c.minionValues.indices.contains(i - 1) ? c.minionValues[i - 1] : ""
                        model.setCalcsMinion(v, byItemSet: c.minionByItemSet)
                    }
                }
                if !c.minionSkills.isEmpty {
                    picker("Minion skill: " + (c.minionSkills.indices.contains(Int(c.minionSkillIndex) - 1) ? c.minionSkills[Int(c.minionSkillIndex) - 1] : "—"), c.minionSkills, Int(c.minionSkillIndex)) { model.setCalcsMinionSkill($0) }
                }
                DropPill(title: "Mode: " + (Self.modes.first { $0.0 == inputs.buffMode }?.1 ?? inputs.buffMode)) {
                    ForEach(Self.modes, id: \.0) { m in
                        Button { model.setCalcsBuffMode(m.0) } label: {
                            if m.0 == inputs.buffMode { Label(m.1, systemImage: "checkmark") } else { Text(m.1) }
                        }
                    }
                }
                if inputs.hasMinion {
                    Toggle(isOn: Binding(get: { inputs.showMinion }, set: { model.setCalcsShowMinion($0) })) {
                        Text("Show minion stats").font(Tok.ui(15, .medium)).foregroundStyle(Tok.textPrimary).textLift()
                    }
                    .toggleStyle(CheckStyle())
                }
            }
            Text("The sidebar is always Effective DPS; this mode only changes the sections below, as in PoB.")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .plate()
    }

    private func picker(_ title: String, _ options: [String], _ sel: Int, _ set: @escaping (Int) -> Void) -> some View {
        DropPill(title: title) {
            ForEach(Array(options.enumerated()), id: \.offset) { i, o in
                Button { set(i + 1) } label: {
                    if i + 1 == sel { Label(o, systemImage: "checkmark") } else { Text(o) }
                }
            }
        }
    }

    private func counter(_ label: String, _ value: Int64, _ set: @escaping (Int64) -> Void) -> some View {
        HStack(spacing: Tok.Space.s2) {
            Text(label).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            NumStepper(value: value, editable: true, compact: true, set: set)
        }
    }
}

/// One PoB section as a card: title, PoB's header value, rows; tables get
/// their damage-type header and scroll sideways when they must.
struct CalcCard: View {
    let section: CalcSection
    let search: String
    var onCell: (Int, Int) -> Void
    @AppStorage private var closed: Bool

    init(section: CalcSection, search: String, onCell: @escaping (Int, Int) -> Void) {
        self.section = section
        self.search = search
        self.onCell = onCell
        _closed = AppStorage(wrappedValue: section.collapsedDefault, "calcs.closed.\(section.stableId)")
    }

    private var tint: Color { section.colour.isEmpty ? Tok.ice500 : Color(rrggbb: section.colour) }
    private var isTable: Bool { section.rows.contains { $0.cells.count > 1 } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button { closed.toggle() } label: {
                HStack(alignment: .firstTextBaseline, spacing: Tok.Space.s2) {
                    Text(section.label).font(Tok.display(19)).foregroundStyle(tint).lineLimit(1)
                    Spacer(minLength: Tok.Space.s2)
                    if !section.extra.isEmpty {
                        Text(section.extra).font(Tok.mono(17, .bold)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1).minimumScaleFactor(0.7)
                    }
                    Image(systemName: "chevron.down").font(.system(size: 11, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
                        .rotationEffect(.degrees(closed ? -90 : 0))
                }
                .padding(Tok.Space.s4)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(section.label)\(section.extra.isEmpty ? "" : ", \(section.extra)"), \(closed ? "collapsed" : "expanded")")

            if !closed {
                Rectangle().fill(Tok.ink400).frame(height: 1)
                if isTable {
                    ScrollView(.horizontal, showsIndicators: true) { table }
                } else {
                    list
                }
            }
        }
        .background(Tok.ink300)
        .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
        .overlay(alignment: .leading) { Rectangle().fill(tint.opacity(0.7)).frame(width: 2) }
        .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
    }

    private var list: some View {
        VStack(spacing: 0) {
            ForEach(Array(section.rows.enumerated()), id: \.offset) { i, r in
                HStack(alignment: .firstTextBaseline, spacing: Tok.Space.s3) {
                    Text(r.label).font(Tok.ui(15)).foregroundStyle(matches(r.label) ? Tok.cyan300 : Tok.textSecondary).textLift()
                        .lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
                    ForEach(Array(r.cells.enumerated()), id: \.offset) { j, c in
                        cellView(c, row: i + 1, cell: j + 1)
                    }
                }
                .padding(.horizontal, Tok.Space.s4).padding(.vertical, 6)
                .accessibilityElement(children: .combine)
                if i < section.rows.count - 1 { Rectangle().fill(Tok.ink400.opacity(0.6)).frame(height: 1) }
            }
        }
        .padding(.vertical, Tok.Space.s2)
    }

    /// Label column pinned by being first in each row; the row scrolls as one.
    private var table: some View {
        Grid(alignment: .trailing, horizontalSpacing: Tok.Space.s3, verticalSpacing: 6) {
            ForEach(Array(section.rows.enumerated()), id: \.offset) { i, r in
                GridRow {
                    Text(r.label).font(Tok.ui(15)).foregroundStyle(matches(r.label) ? Tok.cyan300 : Tok.textSecondary).textLift()
                        .gridColumnAlignment(.leading).lineLimit(1).frame(minWidth: 120, alignment: .leading)
                    ForEach(Array(r.cells.enumerated()), id: \.offset) { j, c in
                        if r.isHeader {
                            Text(c.value.trimmingCharacters(in: CharacterSet(charactersIn: ":")))
                                .font(Tok.label).tracking(1).foregroundStyle(Self.typeTint(c.value))
                                .frame(minWidth: 72, alignment: .trailing)
                        } else {
                            cellView(c, row: i + 1, cell: j + 1).frame(minWidth: 72, alignment: .trailing)
                        }
                    }
                }
                if i < section.rows.count - 1 { Divider().overlay(Tok.ink400.opacity(0.6)) }
            }
        }
        .padding(Tok.Space.s4)
    }

    private func cellView(_ c: CalcCell, row: Int, cell: Int) -> some View {
        Group {
            if c.hasBreakdown, !c.value.isEmpty {
                Button { onCell(row, cell) } label: {
                    Text(c.value).font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift()
                        .underline(true, pattern: .dot, color: Tok.cyan700)
                }
                .buttonStyle(.plain)
                .accessibilityHint("Show breakdown")
            } else {
                Text(c.value.isEmpty ? "–" : c.value).font(Tok.mono(15)).foregroundStyle(c.value.isEmpty ? Tok.textDisabled : Tok.textPrimary).textLift()
            }
        }
        .lineLimit(1)
    }

    private func matches(_ label: String) -> Bool {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return !q.isEmpty && label.lowercased().contains(q)
    }

    /// PoB tints damage-type headers by type.
    static func typeTint(_ s: String) -> Color {
        switch s.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ": ")) {
        case "physical": return Color(hex: 0xC8C8C8)
        case "lightning": return Color(hex: 0xFFE066)
        case "cold": return Color(hex: 0x4D9BFF)
        case "fire": return Color(hex: 0xFF7A3D)
        case "chaos": return Color(hex: 0xD26CFF)
        default: return Tok.ice700
        }
    }
}

/// PoB's stat breakdown for a tapped value: its text and tables, as PoB's
/// own breakdown control builds them.
struct BreakdownSheet: View {
    @ObservedObject var model: BuildModel
    let target: BreakdownTarget
    @Environment(\.dismiss) private var dismiss
    @State private var parts: [BreakdownPart] = []

    var body: some View {
        let rowLabel = target.section.rows.indices.contains(target.row - 1) ? target.section.rows[target.row - 1].label : ""
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(rowLabel.isEmpty ? target.section.label : rowLabel).font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                        Text(target.section.label).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                    }
                    Spacer()
                    Button("Close") { dismiss() }.buttonStyle(PillStyle())
                }
                if parts.isEmpty {
                    Text("PoB has no breakdown for this value.").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                }
                ForEach(Array(parts.enumerated()), id: \.offset) { _, p in
                    VStack(alignment: .leading, spacing: Tok.Space.s2) {
                        if let l = p.label { SectionLabel(text: l) }
                        if !p.lines.isEmpty {
                            ForEach(Array(p.lines.enumerated()), id: \.offset) { _, line in
                                Text(line).font(Tok.mono(14)).foregroundStyle(Tok.textPrimary).textLift()
                            }
                        }
                        if !p.columns.isEmpty {
                            ScrollView(.horizontal) {
                                Grid(alignment: .leading, horizontalSpacing: Tok.Space.s4, verticalSpacing: 4) {
                                    GridRow {
                                        ForEach(Array(p.columns.enumerated()), id: \.offset) { _, c in
                                            Text(c).font(Tok.label).tracking(1).foregroundStyle(Tok.ice700)
                                        }
                                    }
                                    Divider().overlay(Tok.ink400)
                                    ForEach(Array(p.rows.enumerated()), id: \.offset) { _, r in
                                        GridRow {
                                            ForEach(Array(r.enumerated()), id: \.offset) { _, v in
                                                Text(v).font(Tok.mono(13)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let f = p.footer { Text(f).font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift() }
                    }
                    .padding(Tok.Space.s4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .plate()
                }
            }
            .padding(Tok.Space.s6)
        }
        .frame(minWidth: 640, minHeight: 360)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .task(id: model.refreshCount) {
            parts = model.calcBreakdown(target.section, row: target.row, cell: target.cell)
        }
    }
}
