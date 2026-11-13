import SwiftUI

/// PoB's stat column, styled as the canvas draws it: a main-skill card, then
/// stat rows in cards. PoB groups its rows with blank lines; the bridge
/// currently drops those gaps, so this shows one section until it passes
/// them through.
struct StatsSidebar: View {
    @ObservedObject var model: BuildModel
    var collapse: () -> Void

    private var mainSkill: (name: String, sub: String)? {
        // The group PoB marks as the main skill, else the first enabled one.
        guard let g = model.skillGroups.first(where: \.isMain)
                ?? model.skillGroups.first(where: { $0.enabled && !$0.granted })
                ?? model.skillGroups.first
        else { return nil }
        guard let active = g.gems.first(where: { !$0.isSupport }) ?? g.gems.first else {
            // An empty main group: say so rather than hide the card.
            return ("No gems yet", g.slot.map { "\($0) · add a gem on the Skills tab" } ?? "Add a gem on the Skills tab")
        }
        var sub = g.slot.map { "\($0) · \(g.gems.count) gems" } ?? "\(g.gems.count) gems"
        if let c = model.mainSkill {
            var bits: [String] = []
            if c.parts.indices.contains(Int(c.partIndex) - 1) { bits.append(c.parts[Int(c.partIndex) - 1]) }
            if let st = c.stages { bits.append("\(st) stages") }
            if !bits.isEmpty { sub = bits.joined(separator: " · ") }
        }
        let name = (model.mainSkill?.activeSkills).flatMap { list -> String? in
            let i = Int(model.mainSkill?.activeIndex ?? 1) - 1
            return list.indices.contains(i) ? list[i] : nil
        } ?? active.name
        return (name, sub)
    }

    /// PoB's sections carry no names, only blank lines between them. The
    /// canvas titles them by content, so the title comes from the first row.
    static func title(for sec: StatSection) -> String {
        guard let first = sec.rows.first?.label.lowercased() else { return "Stats" }
        if first.contains("strength") || first.contains("dexterity") || first.contains("intelligence") { return "Attributes" }
        if first.contains("hit pool") || first.contains("max hit") { return "Defence" }
        if first.hasPrefix("total life") || first.contains("life") { return "Life" }
        if first.contains("mana") { return "Mana" }
        if first.contains("energy shield") { return "Energy Shield" }
        if first.contains("resist") { return "Resistances" }
        if first.contains("evasion") || first.contains("armour") || first.contains("block") { return "Mitigation" }
        if first.contains("hit") || first.contains("dps") || first.contains("rate") || first.contains("crit") { return "Offence" }
        return "Stats"
    }

    // Broken into @ViewBuilder pieces on purpose: as one expression this body
    // overflowed the Swift 6.3 type-checker's constraint solver (a stack
    // overflow in SolverTrail::undo). Each sub-view now type-checks on its own.
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                skillHeader
                mainSkillMenu
                if let c = model.mainSkill {
                    MainSkillControlsView(model: model, c: c)
                }
                if let error = model.engineError {
                    Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300)
                }
                statsSection
                warningsView
            }
            .padding(.horizontal, Tok.Space.s3)
            .padding(.bottom, Tok.Space.s6)
        }
        .background(Tok.bar)
    }

    private var skillHeader: some View {
        HStack {
            SectionLabel(text: "Main skill")
            Spacer()
            Button(action: collapse) {
                Image(systemName: "chevron.left")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(Tok.textSecondary).textLift()
                    .frame(width: 40, height: 40)
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            }
            .accessibilityLabel("Hide build stats")
        }
        .padding(.top, Tok.Space.s2)
    }

    // PoB's main-skill dropdown: any socket group can be the one the numbers
    // are for.
    @ViewBuilder private var mainSkillMenu: some View {
        if let m = mainSkill {
            Menu {
                skillMenuItems
            } label: {
                skillMenuLabel(name: m.name, sub: m.sub)
            }
            .buttonStyle(.plain)
            .plate()
            .accessibilityLabel("Main skill: \(m.name)")
        }
    }

    @ViewBuilder private var skillMenuItems: some View {
        ForEach(Array(model.skillGroups.enumerated()), id: \.offset) { i, g in
            Button {
                model.setMainGroup(i + 1)
            } label: {
                if g.isMain { Label(g.label, systemImage: "checkmark") } else { Text(g.label) }
            }
            .disabled(g.granted)
        }
    }

    private func skillMenuLabel(name: String, sub: String) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Text(name).font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                    .multilineTextAlignment(.leading)
                Text(sub).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            }
            Spacer()
            Image(systemName: "chevron.down").font(.system(size: 11, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }

    @ViewBuilder private var statsSection: some View {
        if model.sections.isEmpty {
            StatCard(title: "Stats", rows: model.stats)
        } else {
            ForEach(Array(model.sections.enumerated()), id: \.offset) { _, sec in
                StatCard(title: Self.title(for: sec), rows: sec.rows)
            }
        }
    }

    @ViewBuilder private var warningsView: some View {
        if !model.buildWarnings.isEmpty {
            VStack(alignment: .leading, spacing: Tok.Space.s2) {
                Text("\(model.buildWarnings.count) warning\(model.buildWarnings.count == 1 ? "" : "s")")
                    .font(Tok.label).tracking(Tok.labelTracking).foregroundStyle(Tok.coral300)
                ForEach(model.buildWarnings, id: \.self) { w in
                    Text(w).font(Tok.ui(15)).foregroundStyle(Tok.coral300)
                }
            }
            .padding(Tok.Space.s4)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Tok.dangerTint.opacity(0.5))
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.coral500.opacity(0.5), lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
            .accessibilityElement(children: .combine)
        }
    }
}

/// "OFFENCE ──── ▾" then a card of label/value rows. Collapsible.
struct StatCard: View {
    let title: String
    let rows: [Stat]
    @AppStorage private var closed: Bool
    private var open: Bool { !closed }

    init(title: String, rows: [Stat]) {
        self.title = title
        self.rows = rows
        _closed = AppStorage(wrappedValue: false, "statSection.closed.\(title)")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            Button { closed.toggle() } label: {
                HStack(spacing: Tok.Space.s2) {
                    SectionLabel(text: title)
                    Rectangle().fill(Tok.ink400).frame(height: 1)
                    Image(systemName: "chevron.down")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(Tok.ice700)
                        .rotationEffect(.degrees(open ? 0 : -90))
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(title) section, \(open ? "expanded" : "collapsed")")

            if open {
                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { i, s in
                        HStack(alignment: .firstTextBaseline) {
                            Text(s.label).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                            Spacer(minLength: Tok.Space.s2)
                            Text(s.value)
                                .font(Tok.mono(15))
                                .foregroundStyle(Color(rrggbb: s.colour))
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                        }
                        .padding(.vertical, 7)
                        .accessibilityElement(children: .combine)
                        if i < rows.count - 1 {
                            Rectangle().fill(Tok.ink400).frame(height: 1)
                        }
                    }
                }
                .padding(.horizontal, Tok.Space.s4)
                .padding(.vertical, Tok.Space.s2)
                .plate()
            }
        }
    }
}

extension View {
    /// The card surface: dark navy plate, hairline border, no shadow.
    func plate() -> some View {
        background(Tok.ink300)
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
    }
}

/// The rows PoB draws under its main-skill dropdown, each shown only when
/// PoB would show it: active skill, part, stages, mines, minion, minion skill.
struct MainSkillControlsView: View {
    @ObservedObject var model: BuildModel
    let c: MainSkillControls

    var body: some View {
        VStack(spacing: Tok.Space.s2) {
            if c.activeSkills.count > 1 {
                pick("Skill", c.activeSkills, Int(c.activeIndex)) { model.setMainActiveSkill($0) }
            }
            if c.parts.count > 1 {
                pick("Part", c.parts, Int(c.partIndex)) { model.setMainSkillPart($0) }
            }
            if let st = c.stages {
                count("Stages", st) { model.setMainSkillStages($0) }
            }
            if let m = c.mines {
                count("Active mines", m) { model.setMainSkillMines($0) }
            }
            if !c.minions.isEmpty {
                pick("Minion", c.minions, Int(c.minionIndex)) { i in
                    let v = c.minionValues.indices.contains(i - 1) ? c.minionValues[i - 1] : ""
                    model.setMainSkillMinion(v, byItemSet: c.minionByItemSet)
                }
            }
            if !c.minionSkills.isEmpty {
                pick("Minion skill", c.minionSkills, Int(c.minionSkillIndex)) { model.setMainSkillMinionSkill($0) }
            }
        }
    }

    /// A labelled dropdown row. `sel` is 1-based.
    private func pick(_ label: String, _ options: [String], _ sel: Int, _ set: @escaping (Int) -> Void) -> some View {
        HStack {
            Text(label).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().frame(width: 84, alignment: .leading)
            Menu {
                ForEach(Array(options.enumerated()), id: \.offset) { i, o in
                    Button { set(i + 1) } label: {
                        if i + 1 == sel { Label(o, systemImage: "checkmark") } else { Text(o) }
                    }
                }
            } label: {
                HStack {
                    Text(options.indices.contains(sel - 1) ? options[sel - 1] : "—")
                        .font(Tok.ui(15, .medium)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1)
                    Spacer()
                    Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
                }
                .padding(.horizontal, Tok.Space.s3).frame(height: 40)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(label): \(options.indices.contains(sel - 1) ? options[sel - 1] : "none")")
        }
    }

    /// A labelled "− n +" row.
    private func count(_ label: String, _ value: Int64, _ set: @escaping (Int64) -> Void) -> some View {
        HStack {
            Text(label).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().frame(width: 84, alignment: .leading)
            NumStepper(value: value, editable: true, set: set)
            Spacer()
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
}
