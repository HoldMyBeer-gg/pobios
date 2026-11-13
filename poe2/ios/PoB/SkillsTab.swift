import SwiftUI

/// A slot a socket group can live in, as PoB's Skills tab lists them.
struct SlotChoice: Hashable {
    let label: String
    let name: String
    /// PoB's `groupSlotDropList`, in PoB's order.
    static let all: [SlotChoice] = [
        SlotChoice(label: "Weapon 1", name: "Weapon 1"),
        SlotChoice(label: "Weapon 2", name: "Weapon 2"),
        SlotChoice(label: "Weapon 1 (Swap)", name: "Weapon 1 Swap"),
        SlotChoice(label: "Weapon 2 (Swap)", name: "Weapon 2 Swap"),
        SlotChoice(label: "Helmet", name: "Helmet"),
        SlotChoice(label: "Body Armour", name: "Body Armour"),
        SlotChoice(label: "Gloves", name: "Gloves"),
        SlotChoice(label: "Boots", name: "Boots"),
        SlotChoice(label: "Amulet", name: "Amulet"),
        SlotChoice(label: "Ring 1", name: "Ring 1"),
        SlotChoice(label: "Ring 2", name: "Ring 2"),
        SlotChoice(label: "Ring 3", name: "Ring 3"),
        SlotChoice(label: "Belt", name: "Belt")
    ]
}

/// Socket groups on the left, the selected group's gems on the right, as
/// the canvas lays it out. Indices into the bridge are 1-based, as PoB's
/// own are.
struct SkillsTab: View {
    @ObservedObject var model: BuildModel
    /// 0-based index into `model.skillGroups`.
    @State private var selected = 0
    @State private var manageSets = false

    private var group: SkillGroup? {
        model.skillGroups.indices.contains(selected) ? model.skillGroups[selected] : nil
    }

    var body: some View {
        // The width this tab actually gets, after the sidebar. Below about
        // 880pt (portrait with the sidebar open) the two columns cannot both
        // fit, and an HStack that overflows pushes the sidebar off screen.
        GeometryReader { geo in
            if geo.size.width >= 880 {
                HStack(spacing: 0) {
                    groupList
                        .frame(width: min(480, max(300, geo.size.width * 0.36)))
                    Rectangle().fill(Tok.ink500).frame(width: 1)
                    detail
                }
            } else {
                VStack(spacing: 0) {
                    groupStrip
                    Rectangle().fill(Tok.ink500).frame(height: 1)
                    detail
                }
            }
        }
        .onChange(of: model.skillGroups.count) { _, n in
            if selected >= n { selected = max(0, n - 1) }
        }
        .sheet(isPresented: $manageSets) {
            SetManageSheet(title: "Skill sets", sets: model.skillSets,
                           onNew: { model.newSkillSet($0) },
                           onRename: { model.renameSkillSet($0, to: $1) },
                           onDelete: { model.deleteSkillSet($0) },
                           onSelect: { model.switchSkillSet($0) })
        }
        .onAppear {
            // Open on the main skill, which is what the sidebar shows too.
            if let i = model.skillGroups.firstIndex(where: \.isMain) { selected = i }
        }
    }

    @ViewBuilder private var detail: some View {
        if let g = group {
            GroupDetail(model: model, group: g, index: selected + 1)
        } else {
            Placeholder(name: "No socket groups")
        }
    }

    /// The narrow layout: set picker, then the groups as a horizontal strip.
    private var groupStrip: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            HStack(spacing: Tok.Space.s3) {
                DropPill(title: "Skill set: \(model.skillSets.first(where: \.active)?.title ?? "Default")") {
                    ForEach(model.skillSets, id: \.id) { set in
                        Button(set.title) { model.switchSkillSet(set.id) }
                    }
                }
                Spacer()
                SectionLabel(text: "Socket groups")
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: Tok.Space.s2) {
                    ForEach(Array(model.skillGroups.enumerated()), id: \.offset) { i, g in
                        GroupCard(group: g, selected: i == selected) { selected = i }
                            .frame(width: 240)
                    }
                }
            }
        }
        .padding(Tok.Space.s4)
    }

    private var groupList: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                HStack(spacing: Tok.Space.s3) {
                    DropPill(title: "Skill set: \(model.skillSets.first(where: \.active)?.title ?? "Default")") {
                        ForEach(model.skillSets, id: \.id) { set in
                            Button(set.title) { model.switchSkillSet(set.id) }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Button("Manage…") { manageSets = true }.buttonStyle(PillStyle())
                }

                HStack {
                    SectionLabel(text: "Socket groups")
                    Spacer()
                    Button {
                        if let i = model.newSocketGroup() { selected = i - 1 }
                    } label: {
                        HStack(spacing: 6) { Image(systemName: "plus").font(.system(size: 13, weight: .bold)); Text("New") }
                    }
                    .buttonStyle(PillStyle())
                }
                .padding(.top, Tok.Space.s2)

                ForEach(Array(model.skillGroups.enumerated()), id: \.offset) { i, g in
                    GroupCard(group: g, selected: i == selected) { selected = i }
                }
            }
            .padding(Tok.Space.s4)
        }
    }
}

/// One socket group in the list: its label, socket pips, and slot.
struct GroupCard: View {
    let group: SkillGroup
    let selected: Bool
    let pick: () -> Void

    var body: some View {
        Button(action: pick) {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                Text(group.label + (group.isMain ? " (Active)" : ""))
                    .font(Tok.display(19))
                    .foregroundStyle(group.enabled ? Tok.textPrimary : Tok.textTertiary).textLift()
                    .multilineTextAlignment(.leading)
                HStack(spacing: Tok.Space.s2) {
                    HStack(spacing: 6) {
                        ForEach(Array(group.gems.enumerated()), id: \.offset) { _, gem in
                            Circle()
                                .fill(Color(rrggbb: gem.colour))
                                .frame(width: 14, height: 14)
                                .opacity(gem.enabled ? 1 : 0.35)
                        }
                    }
                    if let slot = group.slot {
                        Text(slot).font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                    } else if group.granted {
                        Text("granted").font(Tok.ui(15)).foregroundStyle(Tok.violet400)
                    }
                }
            }
            .padding(Tok.Space.s4)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(selected ? Tok.ink400 : Tok.ink300)
            .overlay(alignment: .leading) {
                Rectangle().fill(selected ? Tok.cyan500 : .clear).frame(width: 2)
            }
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(selected ? Tok.cyan700 : Tok.ink500, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(group.label), \(group.slot ?? "granted"), \(group.gems.count) gems\(group.isMain ? ", main skill" : "")")
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

/// The selected group: header, toggles, gem table, add-gem search, options.
struct GroupDetail: View {
    @ObservedObject var model: BuildModel
    let group: SkillGroup
    /// 1-based, for the bridge.
    let index: Int
    @State private var confirmDelete = false
    @State private var addingGem = false

    private var sockets: String {
        // The item's own socket colours, as PoB shows them. Falls back to
        // the gem colours only when the group has no socketed item.
        if !group.itemSockets.isEmpty { return group.itemSockets }
        return group.gems.map { letter(for: $0.colour) }.joined(separator: "-")
    }
    private func letter(for hex: String) -> String {
        switch hex.lowercased() {
        case "#ff5d78", "#e04040", "#ff4040": return "R"
        case "#43d6a3", "#40e040", "#40ff40": return "G"
        case "#4d9bff", "#4040ff", "#6060ff": return "B"
        default: return "W"
        }
    }

    var body: some View {
        GeometryReader { outer in
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                let stacked = outer.size.width < 640
                HStack(alignment: .top, spacing: Tok.Space.s3) {
                    VStack(alignment: .leading, spacing: Tok.Space.s2) {
                        Text(group.label + (group.isMain ? " (Active)" : ""))
                            .font(Tok.display(27)).foregroundStyle(Tok.textPrimary).textLift()
                            .lineLimit(2).minimumScaleFactor(0.7)
                        if let slot = group.slot {
                            Text("\(slot) · \(sockets)")
                                .font(Tok.mono(13)).foregroundStyle(Tok.textSecondary).textLift()
                                .padding(.horizontal, Tok.Space.s3).frame(height: 28)
                                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
                        }
                    }
                    Spacer(minLength: Tok.Space.s2)
                    DropPill(title: "Socketed in: \(SlotChoice.all.first { $0.name == group.slot }?.label ?? group.slot ?? "None")", enabled: !group.granted) {
                        Button {
                            model.setGroupSlot(index, nil)
                        } label: {
                            if group.slot == nil { Label("None", systemImage: "checkmark") } else { Text("None") }
                        }
                        Divider()
                        ForEach(SlotChoice.all, id: \.self) { c in
                            Button {
                                model.setGroupSlot(index, c.name)
                            } label: {
                                if c.name == group.slot { Label(c.label, systemImage: "checkmark") } else { Text(c.label) }
                            }
                        }
                    }
                    if !group.granted {
                        Button { confirmDelete = true } label: {
                            Image(systemName: "trash").font(.system(size: 15))
                        }
                        .buttonStyle(IconPillStyle())
                        .accessibilityLabel("Delete socket group")
                        .confirmationDialog("Delete this socket group?", isPresented: $confirmDelete, titleVisibility: .visible) {
                            Button("Delete \(group.gems.count) gem\(group.gems.count == 1 ? "" : "s")", role: .destructive) {
                                model.deleteSocketGroup(index)
                            }
                        }
                    }
                }

                HStack(spacing: Tok.Space.s5) {
                    Toggle(isOn: Binding(get: { group.fullDps }, set: { model.setGroupFullDps(index, $0) })) {
                        Text("Full DPS").font(Tok.ui(17, .medium)).foregroundStyle(Tok.textPrimary).textLift()
                    }
                    .toggleStyle(CheckStyle())
                    .disabled(group.granted)
                    Toggle(isOn: Binding(get: { group.enabled }, set: { model.setGroupEnabled(index, $0) })) {
                        Text("Enabled").font(Tok.ui(17, .medium)).foregroundStyle(Tok.textPrimary).textLift()
                    }
                    .tint(Tok.cyan500)
                    .fixedSize()
                    .disabled(group.granted)
                    if !group.isMain {
                        Button("Set as main skill") { model.setMainGroup(index) }
                            .buttonStyle(PillStyle())
                            .disabled(group.granted)
                    }
                }

                gemTable(stacked: stacked)

                if !group.granted {
                    Button {
                        addingGem = true
                    } label: {
                        HStack(spacing: Tok.Space.s2) {
                            Image(systemName: "plus").font(.system(size: 13, weight: .bold))
                            Text("Add gem")
                        }
                    }
                    .buttonStyle(PillStyle())
                    .accessibilityHint("Pick a gem for this group, ranked by the DPS it adds")
                    .sheet(isPresented: $addingGem) {
                        GemPickerSheet(model: model, group: index, gem: group.gems.count + 1)
                    }
                }

                HStack(alignment: .top, spacing: Tok.Space.s4) {
                    optionsCard
                    tipsCard
                }
            }
            .padding(Tok.Space.s4)
        }
        }
    }

    /// PoB's count is for spectres and a few minions; the column only earns
    /// its width when a gem in the group uses it.
    private var showCount: Bool { group.gems.contains { $0.count > 1 } }

    private func gemTable(stacked: Bool) -> some View {
            VStack(spacing: 0) {
                if !stacked {
                    HStack(spacing: Tok.Space.s2) {
                        SectionLabel(text: "Gem").frame(maxWidth: .infinity, alignment: .leading)
                        SectionLabel(text: "Level").frame(width: 96)
                        SectionLabel(text: "Quality").frame(width: 96)
                        SectionLabel(text: "On").frame(width: 44)
                        if showCount { SectionLabel(text: "Count").frame(width: 56) }
                        Color.clear.frame(width: 36)
                    }
                    .padding(.horizontal, Tok.Space.s4).padding(.vertical, Tok.Space.s3)
                    Rectangle().fill(Tok.ink400).frame(height: 1)
                }
                ForEach(Array(group.gems.enumerated()), id: \.offset) { j, gem in
                    GemRow(model: model, group: index, gem: j + 1, data: gem, editable: !group.granted,
                           stacked: stacked, showCount: showCount)
                    if j < group.gems.count - 1 {
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                }
            }
            .plate()
    }

    /// PoB's per-build gem defaults, written to the Skills tab's fields.
    private var optionsCard: some View {
        let o = model.skillsOptions
        let levelNames = ["normalMaximum": "Normal maximum", "corruptedMaximum": "Corrupted maximum", "awakenedMaximum": "Awakened maximum", "characterLevel": "Match character level"]
        let supportNames = ["ALL": "All", "NORMAL": "Normal only", "AWAKENED": "Awakened only"]
        return VStack(alignment: .leading, spacing: Tok.Space.s3) {
            SectionLabel(text: "Gem options")
            optionPick("Default gem level", levelNames[o["defaultGemLevel"] ?? ""] ?? (o["defaultGemLevel"] ?? "")) {
                ForEach(["normalMaximum", "corruptedMaximum", "awakenedMaximum", "characterLevel"], id: \.self) { k in
                    Button(levelNames[k]!) { model.setSkillsOption("defaultGemLevel", k) }
                }
            }
            HStack {
                Text("Default gem quality").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Spacer()
                let q = Int64(o["defaultGemQuality"] ?? "") ?? 0
                NumStepper(value: q, editable: true, compact: true, range: 0...23) {
                    model.setSkillsOption("defaultGemQuality", String($0))
                }
            }
            HStack {
                Text("Sort gems by DPS").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Spacer()
                Toggle("", isOn: Binding(get: { o["sortGemsByDPS"] == "true" }, set: { model.setSkillsOption("sortGemsByDPS", $0 ? "true" : "false") }))
                    .toggleStyle(CheckStyle()).labelsHidden().accessibilityLabel("Sort gems by DPS")
            }
            optionPick("Show support gems", supportNames[o["showSupportGemTypes"] ?? ""] ?? (o["showSupportGemTypes"] ?? "")) {
                ForEach(["ALL", "NORMAL", "AWAKENED"], id: \.self) { k in
                    Button(supportNames[k]!) { model.setSkillsOption("showSupportGemTypes", k) }
                }
            }
            HStack {
                Text("Show legacy gems").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Spacer()
                Toggle("", isOn: Binding(get: { o["showLegacyGems"] == "true" }, set: { model.setSkillsOption("showLegacyGems", $0 ? "true" : "false") }))
                    .toggleStyle(CheckStyle()).labelsHidden().accessibilityLabel("Show legacy gems")
            }
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .plate()
    }

    private func optionPick<C: View>(_ k: String, _ current: String, @ViewBuilder _ c: () -> C) -> some View {
        HStack {
            Text(k).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            Spacer()
            DropPill(title: current, content: c)
        }
    }

    private var tipsCard: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            SectionLabel(text: "Tips")
            Text("Tap a group to edit it. Use the toggles to enable it, count it in Full DPS, or make it the main skill.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .plate()
    }
}

/// One gem: dot, name, level stepper, quality, enabled, count, remove.
struct GemRow: View {
    @ObservedObject var model: BuildModel
    let group: Int
    let gem: Int
    let data: Gem
    let editable: Bool
    /// Name on its own line, controls underneath: for narrow widths.
    var stacked = false
    var showCount = true
    @State private var showDesc = false
    @State private var desc = ""
    @State private var swapping = false

    private var name: some View {
        // The name is the swap affordance, as it is in PoB: tap it to pick a
        // different gem for this slot, ranked by DPS. A chevron marks it.
        Button { swapping = true } label: {
            HStack(spacing: Tok.Space.s3) {
                Circle().fill(Color(rrggbb: data.colour)).frame(width: 12, height: 12)
                Text(data.name)
                    .font(Tok.ui(17, data.isSupport ? .regular : .medium))
                    .foregroundStyle(data.enabled ? Tok.textPrimary : Tok.textTertiary).textLift()
                    .lineLimit(1).truncationMode(.tail)
                if editable {
                    Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!editable)
        .accessibilityLabel("Change \(data.name)")
    }

    private var controls: some View {
        HStack(spacing: Tok.Space.s2) {
            if stacked { Text("Lv").font(Tok.label).foregroundStyle(Tok.textTertiary).textLift() }
            NumStepper(value: data.level, editable: editable, compact: true, range: 1...Int64.max) {
                model.setGem(group, gem, level: $0)
            }
                .frame(width: 96)
            if stacked { Text("Q").font(Tok.label).foregroundStyle(Tok.textTertiary).textLift() }
            NumStepper(value: data.quality, editable: editable, compact: true, range: 0...23) {
                model.setGem(group, gem, quality: $0)
            }
                .frame(width: 96)
            // The socket/item bonus is applied automatically; show it after
            // the stepper so the base is not mistaken for the total.
            Text(data.effectiveQuality > data.quality ? "+\(data.effectiveQuality - data.quality)" : "")
                .font(Tok.mono(13)).foregroundStyle(Tok.cyan300)
                .frame(width: 28, alignment: .leading)
            Toggle("", isOn: Binding(get: { data.enabled }, set: { model.setGem(group, gem, enabled: $0) }))
                .toggleStyle(CheckStyle()).labelsHidden().disabled(!editable)
                .frame(width: 44)
                .accessibilityLabel("\(data.name) enabled")
            if showCount {
                Text(data.count > 0 ? "\(data.count)" : "–").font(Tok.mono(15)).foregroundStyle(Tok.textSecondary).textLift().frame(width: 56)
            }
            Button { model.removeGem(group, gem) } label: {
                Image(systemName: "xmark").font(.system(size: 12, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
            }
            .frame(width: 36, height: 44)
            .disabled(!editable)
            .accessibilityLabel("Remove \(data.name)")
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
        Group {
            if stacked {
                VStack(alignment: .leading, spacing: 4) {
                    name
                    controls
                }
                .padding(.vertical, Tok.Space.s2)
            } else {
                HStack(spacing: Tok.Space.s2) {
                    name.frame(maxWidth: .infinity, alignment: .leading)
                    controls
                }
                .frame(height: 56)
            }
        }
        .padding(.horizontal, Tok.Space.s4)
        .accessibilityElement(children: .contain)
        // Tap the row to see what the gem does; PoB forgets too.
        .contentShape(Rectangle())
        .onTapGesture {
            if !showDesc && desc.isEmpty { desc = model.gemDescription(group, gem) }
            showDesc.toggle()
        }
        if showDesc, !desc.isEmpty {
            Text(desc)
                .font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, Tok.Space.s4).padding(.bottom, Tok.Space.s2)
        }
        }
        .sheet(isPresented: $swapping) {
            GemPickerSheet(model: model, group: group, gem: gem, current: data.name)
        }
    }
}

/// Pick a gem for a slot, ranked by the DPS each would add here (PoB's own
/// per-gem recalc). Opening it runs the ranking, as PoB does on click. This
/// backs both swapping a socketed gem and adding a new one.
struct GemPickerSheet: View {
    @ObservedObject var model: BuildModel
    let group: Int
    /// The gem slot to fill: an existing index to replace, or the next index
    /// to append.
    let gem: Int
    /// The gem being replaced, for the title; empty when adding.
    var current: String = ""
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var ranked: [RankedGem] = []
    @State private var ranking = false

    private var sortByDps: Bool { model.skillsOptions["sortGemsByDPS"] == "true" }

    private var matches: [GemChoice] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard q.count >= 2 else { return [] }
        return Array(model.gemCatalogue.filter { $0.name.lowercased().contains(q) }.prefix(20))
    }

    private func delta(_ id: String) -> Double? { ranked.first { $0.id == id }?.delta }

    /// Ranked supports, filtered by the search box.
    private var rows: [RankedGem] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        if q.isEmpty { return Array(ranked.prefix(60)) }
        return ranked.filter { $0.name.lowercased().contains(q) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                Text(current.isEmpty ? "Add gem" : "Change \(current)")
                    .font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
            }
            WellField(placeholder: "Search all gems — e.g. inspiration, frostbolt", text: $query)

            if ranking {
                HStack(spacing: Tok.Space.s2) {
                    ProgressView().controlSize(.small)
                    Text("Sorting supports by DPS…").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
            }

            // When the user is searching all gems (actives included), show the
            // raw name matches; otherwise the DPS-ranked support list.
            if !query.trimmingCharacters(in: .whitespaces).isEmpty && rows.isEmpty && !matches.isEmpty {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(matches, id: \.id) { g in
                            pickRow(name: g.name, colour: g.colour, socketed: false, delta: delta(g.id)) {
                                model.setGemId(group, gem, g.id); dismiss()
                            }
                        }
                    }
                }.plate()
            } else if sortByDps {
                if !rows.isEmpty {
                    Text("Best first. Green is the DPS it would add; a tick marks one already in this group.")
                        .font(Tok.ui(12)).foregroundStyle(Tok.textTertiary).textLift()
                }
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(rows, id: \.self) { g in
                            pickRow(name: g.name, colour: g.colour, socketed: g.socketed, delta: g.delta) {
                                model.setGemId(group, gem, g.id); dismiss()
                            }
                        }
                    }
                }.plate()
            } else {
                Text("Turn on \"Sort gems by DPS\" in Gem options to rank supports, or search above.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
            Spacer(minLength: 0)
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 520, minHeight: 560)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear { if sortByDps { rank() } }
    }

    private func pickRow(name: String, colour: String, socketed: Bool, delta: Double?, tap: @escaping () -> Void) -> some View {
        Button(action: tap) {
            HStack(spacing: Tok.Space.s2) {
                Circle().fill(Color(rrggbb: colour)).frame(width: 10, height: 10)
                Text(name).font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1)
                if socketed {
                    Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Tok.ok)
                }
                Spacer()
                if let d = delta, abs(d) >= 0.5 {
                    Text("\(d > 0 ? "+" : "")\(GemPickerSheet.fmt(d)) DPS")
                        .font(Tok.mono(13))
                        .foregroundStyle(d > 0 ? Tok.ok : Tok.coral300)
                }
            }
            .padding(.horizontal, Tok.Space.s3).frame(height: 40)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .overlay(alignment: .bottom) { Rectangle().fill(Tok.ink400).frame(height: 1) }
    }

    private func rank() {
        guard sortByDps else { ranked = []; return }
        ranking = true
        let g = group
        Task {
            ranked = await model.rankSupports(for: g)
            ranking = false
        }
    }

    /// A DPS delta, abbreviated: 12.3k, 4.5m.
    static func fmt(_ v: Double) -> String {
        let a = abs(v)
        if a >= 1_000_000 { return String(format: "%.1fm", v / 1_000_000) }
        if a >= 1_000 { return String(format: "%.1fk", v / 1_000) }
        return String(format: "%.0f", v)
    }
}

/// "− 20 +", the canvas's numeric stepper. (SwiftUI already has a `Stepper`.)
/// A "− n +" control whose taps coalesce: the number moves at once, the
/// build is told once the tapping stops. Every value it sets costs a full
/// recalculation, so five quick taps used to queue five of them; now they
/// queue one.
struct NumStepper: View {
    let value: Int64
    let editable: Bool
    var compact = false
    var range: ClosedRange<Int64> = 0...Int64.max
    let set: (Int64) -> Void
    @State private var pending: Int64?
    @State private var commit: Task<Void, Never>?

    var body: some View {
        let b: CGFloat = compact ? 28 : 32
        let w: CGFloat = compact ? 36 : 44
        let shown = pending ?? value
        HStack(spacing: 0) {
            Button("−") { step(-1) }.frame(width: b, height: b)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
            Text("\(shown)").font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift().frame(width: w)
            Button("+") { step(1) }.frame(width: b, height: b)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
        }
        .font(Tok.ui(15, .bold))
        .foregroundStyle(editable ? Tok.textSecondary : Tok.textDisabled).textLift()
        .buttonStyle(.plain)
        .disabled(!editable)
        .accessibilityElement(children: .combine)
        .accessibilityValue("\(shown)")
        .accessibilityAdjustableAction { d in step(d == .increment ? 1 : -1) }
    }

    private func step(_ delta: Int64) {
        let next = min(range.upperBound, max(range.lowerBound, (pending ?? value) + delta))
        guard next != (pending ?? value) else { return }
        pending = next
        commit?.cancel()
        commit = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled, let v = pending else { return }
            pending = nil
            if v != value { set(v) }
        }
    }
}

/// Manage a family of sets (skill, item, config): rename, add, delete, pick.
struct SetManageSheet: View {
    let title: String
    let sets: [NamedSet]
    var onNew: (String) -> Void
    var onRename: (Int64, String) -> Void
    var onDelete: (Int64) -> Void
    var onSelect: (Int64) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var newName = ""
    @State private var renaming: NamedSet?
    @State private var renameTo = ""

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            HStack {
                Text(title).font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(PillStyle(primary: true))
            }
            VStack(spacing: 0) {
                ForEach(Array(sets.enumerated()), id: \.element.id) { i, set in
                    HStack(spacing: Tok.Space.s3) {
                        Button {
                            onSelect(set.id)
                        } label: {
                            HStack(spacing: Tok.Space.s2) {
                                Image(systemName: set.active ? "checkmark.circle.fill" : "circle")
                                    .foregroundStyle(set.active ? Tok.cyan500 : Tok.textTertiary).textLift()
                                Text(set.title).font(Tok.ui(17, set.active ? .bold : .regular)).foregroundStyle(Tok.textPrimary).textLift()
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(set.title)\(set.active ? ", active" : "")")
                        Spacer()
                        Button("Rename") { renaming = set; renameTo = set.title }.buttonStyle(PillStyle())
                        Button { onDelete(set.id) } label: { Image(systemName: "trash") }
                            .buttonStyle(IconPillStyle())
                            .disabled(sets.count < 2)
                            .accessibilityLabel("Delete \(set.title)")
                    }
                    .padding(.horizontal, Tok.Space.s3).frame(height: 56)
                    if i < sets.count - 1 { Rectangle().fill(Tok.ink400).frame(height: 1) }
                }
            }
            .plate()
            HStack(spacing: Tok.Space.s3) {
                WellField(placeholder: "New set name", text: $newName)
                Button("Add") {
                    onNew(newName.trimmingCharacters(in: .whitespaces))
                    newName = ""
                }
                .buttonStyle(PillStyle())
                .disabled(newName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 520)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .alert("Rename set", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $renameTo)
            Button("Save") {
                if let r = renaming { onRename(r.id, renameTo.trimmingCharacters(in: .whitespaces)) }
                renaming = nil
            }
            Button("Cancel", role: .cancel) { renaming = nil }
        }
    }
}
