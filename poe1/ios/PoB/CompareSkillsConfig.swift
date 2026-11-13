import SwiftUI

/// PoB's Compare › Skills: socket groups side by side, gems added or
/// removed in green and red, levels and quality shown as level/quality.
struct CompareSkillsPage: View {
    @ObservedObject var model: BuildModel
    let other: OtherBuild
    let otherName: String

    /// Groups are matched by slot first, then by their active gems.
    private var pairs: [(SkillGroup?, OtherBuild.Group?)] {
        var theirs = other.activeGroups
        var out: [(SkillGroup?, OtherBuild.Group?)] = []
        for g in model.skillGroups {
            let mineActive = Set(g.gems.filter { !$0.isSupport }.map(\.name))
            let idx = theirs.firstIndex { t in
                (g.slot != nil && t.slot == g.slot) || !Set(t.gems.filter { !$0.support }.map(\.name)).isDisjoint(with: mineActive)
            }
            if let i = idx { out.append((g, theirs.remove(at: i))) } else { out.append((g, nil)) }
        }
        for t in theirs { out.append((nil, t)) }
        return out
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                Text(model.buildName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.ok).frame(maxWidth: .infinity, alignment: .leading)
                Text(otherName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.violet400).frame(maxWidth: .infinity, alignment: .leading)
            }
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(Array(pairs.enumerated()), id: \.offset) { _, p in
                        HStack(alignment: .top, spacing: Tok.Space.s4) {
                            mine(p.0, against: p.1)
                            theirs(p.1, against: p.0)
                        }
                        .padding(Tok.Space.s4)
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                }
                .plate()
            }
        }
    }

    private func title(_ label: String, _ slot: String?) -> some View {
        Text(label + (slot.map { " (\($0))" } ?? "")).font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
    }

    private func mine(_ g: SkillGroup?, against t: OtherBuild.Group?) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            if let g {
                title(g.label, g.slot)
                let otherNames = Set(t?.gems.map(\.name) ?? [])
                ForEach(Array(g.gems.enumerated()), id: \.offset) { _, gem in
                    gemLine(gem.name, gem.level, gem.quality, colour: Color(rrggbb: gem.colour), added: !otherNames.contains(gem.name), removed: false)
                }
            } else {
                Text("(not in this build)").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func theirs(_ t: OtherBuild.Group?, against g: SkillGroup?) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            if let t {
                let active = t.gems.filter { !$0.support }.map(\.name)
                title(active.isEmpty ? "(empty)" : active.joined(separator: ", "), t.slot)
                let myNames = Set(g?.gems.map(\.name) ?? [])
                ForEach(Array(t.gems.enumerated()), id: \.offset) { _, gem in
                    gemLine(gem.name, Int64(gem.level), Int64(gem.quality), colour: Tok.textPrimary, added: !myNames.contains(gem.name), removed: false)
                }
                // Gems this build has that theirs lacks, as PoB lists them in red.
                ForEach((g?.gems ?? []).filter { !Set(t.gems.map(\.name)).contains($0.name) }, id: \.name) { gem in
                    gemLine(gem.name, nil, nil, colour: Tok.coral300, added: false, removed: true)
                }
            } else {
                Text("(not in that build)").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func gemLine(_ name: String, _ level: Int64?, _ quality: Int64?, colour: Color, added: Bool, removed: Bool) -> some View {
        HStack(spacing: 6) {
            if added { Text("+").font(Tok.mono(13, .bold)).foregroundStyle(Tok.ok) }
            if removed { Text("-").font(Tok.mono(13, .bold)).foregroundStyle(Tok.coral300) }
            Text(name).font(Tok.ui(15)).foregroundStyle(removed ? Tok.coral300 : added ? Tok.ok : colour)
            if let level, let quality { Text("\(level)/\(quality)").font(Tok.mono(13)).foregroundStyle(Tok.textSecondary).textLift() }
        }
    }
}

/// PoB's Compare › Config: every option side by side, with diff counts
/// per section, and Copy Config from the compared build.
struct CompareConfigPage: View {
    @ObservedObject var model: BuildModel
    let other: OtherBuild
    let otherName: String
    @State private var search = ""
    @State private var showAll = false
    @State private var confirmCopy = false

    private static let order = ["General", "Skill Options", "Map Modifiers and Player Debuffs", "When In Combat", "For Effective DPS", "Enemy Stats"]

    private func theirValue(_ o: ConfigOption) -> String {
        let raw = other.activeConfig[o.var] ?? ""
        switch o.kind {
        case .check: return raw == "true" ? "true" : "false"
        case .number: return raw
        case .list: return raw.isEmpty ? (o.choices.first?.key ?? "") : raw
        }
    }

    private func differs(_ o: ConfigOption) -> Bool {
        let mine = o.kind == .check ? (o.value == "true" ? "true" : "false") : o.kind == .list ? (o.value.isEmpty ? (o.choices.first?.key ?? "") : o.value) : (o.value.isEmpty ? "0" : o.value)
        let theirs = o.kind == .number ? (theirValue(o).isEmpty ? "0" : theirValue(o)) : theirValue(o)
        return mine != theirs
    }

    private var sections: [(String, [ConfigOption], Int)] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        var by: [String: [ConfigOption]] = [:]
        for o in model.configOptions where showAll || differs(o) || o.relevant {
            if q.isEmpty || o.label.lowercased().contains(q) { by[o.section, default: []].append(o) }
        }
        let keys = Self.order.filter { by[$0] != nil } + by.keys.filter { !Self.order.contains($0) }.sorted()
        return keys.map { ($0, by[$0]!, by[$0]!.filter(differs).count) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            FlowRow(spacing: Tok.Space.s3) {
                WellField(placeholder: "Search configuration", text: $search).frame(width: 320)
                Toggle(isOn: $showAll) { Text("Show all configurations").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift() }.toggleStyle(CheckStyle())
                Button("Copy config from \(otherName)") { confirmCopy = true }.buttonStyle(PillStyle())
            }
            HStack {
                Spacer().frame(maxWidth: .infinity)
                Text(model.buildName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.ok).frame(width: 220, alignment: .leading)
                Text(otherName).font(Tok.ui(15, .bold)).foregroundStyle(Tok.violet400).frame(width: 220, alignment: .leading)
            }
            ScrollView {
                VStack(spacing: Tok.Space.s4) {
                    ForEach(sections, id: \.0) { sec in
                        VStack(alignment: .leading, spacing: 0) {
                            HStack(spacing: Tok.Space.s2) {
                                SectionLabel(text: sec.0)
                                if sec.2 > 0 { Text("(\(sec.2) diff)").font(Tok.ui(13)).foregroundStyle(Tok.coral300) }
                            }
                            .padding(Tok.Space.s4)
                            Rectangle().fill(Tok.ink400).frame(height: 1)
                            ForEach(sec.1, id: \.var) { o in
                                HStack(spacing: Tok.Space.s3) {
                                    Text(o.label).font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift().frame(maxWidth: .infinity, alignment: .leading)
                                    value(o, o.kind == .list ? (o.value.isEmpty ? (o.choices.first?.key ?? "") : o.value) : o.value)
                                    value(o, theirValue(o))
                                }
                                .padding(.horizontal, Tok.Space.s4).frame(minHeight: 44)
                                .background(differs(o) ? Tok.dangerTint.opacity(0.25) : .clear)
                                .accessibilityElement(children: .combine)
                                Rectangle().fill(Tok.ink400.opacity(0.6)).frame(height: 1)
                            }
                        }
                        .plate()
                    }
                }
            }
        }
        .confirmationDialog("Copy configuration from \(otherName)?", isPresented: $confirmCopy, titleVisibility: .visible) {
            Button("Copy every option") {
                for o in model.configOptions where differs(o) {
                    model.setConfig(o, value: theirValue(o))
                }
            }
        } message: { Text("Every option that differs is set to the compared build's value. Undo on the Config tab reverses it one by one.") }
    }

    private func value(_ o: ConfigOption, _ v: String) -> some View {
        Group {
            switch o.kind {
            case .check:
                Image(systemName: v == "true" ? "checkmark.square.fill" : "square").foregroundStyle(v == "true" ? Tok.cyan500 : Tok.textDisabled)
            case .number:
                Text(v.isEmpty ? "0" : v).font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift()
            case .list:
                Text(o.choices.first { $0.key == v }?.label ?? (v.isEmpty ? "—" : v)).font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1)
            }
        }
        .frame(width: 220, alignment: .leading)
    }
}
