import SwiftUI

/// PoB's Configuration tab: every option from ConfigOptions.lua, grouped
/// into PoB's sections, showing only what PoB would show for this build
/// unless the switch is flipped. Custom modifier blocks at the end.
struct ConfigTab: View {
    @ObservedObject var model: BuildModel
    @State private var search = ""
    @AppStorage("config.showAll") private var showAll = false
    @State private var manageSets = false

    /// PoB's section order, from ConfigOptions.lua.
    private static let order = ["General", "Skill Options", "Map Modifiers and Player Debuffs", "When In Combat", "For Effective DPS", "Enemy Stats"]

    private var sections: [(String, [ConfigOption])] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        let shown = model.configOptions.filter { o in
            (showAll || o.relevant || o.isSet) && (q.isEmpty || o.label.lowercased().contains(q) || o.section.lowercased().contains(q))
        }
        var by: [String: [ConfigOption]] = [:]
        for o in shown { by[o.section, default: []].append(o) }
        let known = Self.order.filter { by[$0] != nil }.map { ($0, by[$0]!) }
        let rest = by.keys.filter { !Self.order.contains($0) }.sorted().map { ($0, by[$0]!) }
        return known + rest
    }

    var body: some View {
        GeometryReader { geo in
            let cols = geo.size.width >= 1100 ? 3 : geo.size.width >= 720 ? 2 : 1
            ScrollView {
                VStack(alignment: .leading, spacing: Tok.Space.s4) {
                    FlowRow(spacing: Tok.Space.s3) {
                        DropPill(title: "Config set: \(model.configSets.first(where: \.active)?.title ?? "Default")") {
                            ForEach(model.configSets, id: \.id) { set in
                                Button(set.title) { model.switchConfigSet(set.id) }
                            }
                        }
                        Button("Manage…") { manageSets = true }.buttonStyle(PillStyle())
                        WellField(placeholder: "Search configuration — e.g. chilled, power charges, boss", text: $search)
                            .frame(width: min(520, geo.size.width - 40))
                        Toggle(isOn: $showAll) {
                            Text(showAll ? "Showing every option" : "Showing what affects this build").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                        }
                        .tint(Tok.cyan500)
                        .fixedSize()
                    }

                    columns(cols)

                    CustomModsCard(model: model)
                }
                .padding(Tok.Space.s4)
            }
        }
        .sheet(isPresented: $manageSets) {
            SetManageSheet(title: "Config sets", sets: model.configSets,
                           onNew: { model.newConfigSet($0) },
                           onRename: { model.renameConfigSet($0, to: $1) },
                           onDelete: { model.deleteConfigSet($0) },
                           onSelect: { model.switchConfigSet($0) })
        }
    }

    @ViewBuilder private func columns(_ n: Int) -> some View {
        let secs = sections
        let buckets: [[(String, [ConfigOption])]] = {
            var b = Array(repeating: [(String, [ConfigOption])](), count: n)
            var h = Array(repeating: 0, count: n)
            for s in secs {
                let i = h.indices.min { h[$0] < h[$1] } ?? 0
                b[i].append(s); h[i] += s.1.count + 3
            }
            return b
        }()
        HStack(alignment: .top, spacing: Tok.Space.s4) {
            ForEach(0..<n, id: \.self) { i in
                LazyVStack(spacing: Tok.Space.s4) {
                    ForEach(buckets[i], id: \.0) { sec in
                        ConfigCard(model: model, title: sec.0, options: sec.1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .top)
            }
        }
    }
}

extension ConfigOption {
    /// Set to something other than PoB's default, which is worth keeping
    /// visible even when PoB would hide the option.
    var isSet: Bool {
        switch kind {
        case .check: return value == "true"
        case .number: return !(value.isEmpty || value == "0")
        case .list: return !value.isEmpty
        }
    }
}

/// One PoB section: label / control rows.
struct ConfigCard: View {
    // Not observed: the tab hands each card a fresh option list when the
    // model changes, and SwiftUI then skips the rows whose option is equal
    // instead of re-rendering all 130-odd of them on every tap.
    let model: BuildModel
    let title: String
    let options: [ConfigOption]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SectionLabel(text: title).padding(Tok.Space.s4)
            Rectangle().fill(Tok.ink400).frame(height: 1)
            ForEach(Array(options.enumerated()), id: \.element.var) { i, o in
                ConfigRow(model: model, option: o)
                if i < options.count - 1 { Rectangle().fill(Tok.ink400.opacity(0.6)).frame(height: 1) }
            }
        }
        .plate()
    }
}

/// Label, an info button when PoB has help text, and the control PoB uses.
struct ConfigRow: View {
    let model: BuildModel
    let option: ConfigOption
    @State private var showTip = false
    @State private var numberText = ""
    // A stepper tap recalculates the whole build (a quarter second on a
    // Mac, longer on the iPad) and that ran on every tap, so a second tap
    // during the recalc was dropped or landed late. Taps now change the
    // number on screen at once and commit to the engine only after they
    // stop, so a run of taps costs one recalculation.
    @State private var pending: Double?
    @State private var commit: Task<Void, Never>?
    @State private var editing = false
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: Tok.Space.s3) {
            Text(option.label)
                .font(Tok.ui(15)).foregroundStyle(option.relevant ? Tok.textPrimary : Tok.textTertiary).textLift()
                .frame(maxWidth: .infinity, alignment: .leading)
            // PoB's help text behind an (i), tapped for a popover, as the
            // desktop does: keeps the tab clean, the mechanic a tap away.
            if let tip = option.tooltip {
                // The popover modifier is attached only while it is up:
                // a presentation host on every row was part of what made
                // the tab heavy to build and leave.
                if showTip {
                    tipButton(tip).popover(isPresented: $showTip) {
                        // Size to the text so a long tip is never clipped; a
                        // ScrollView guards the rare very long one on a small
                        // popover height.
                        ScrollView {
                            Text(tip)
                                .font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                                .fixedSize(horizontal: false, vertical: true)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(Tok.Space.s4)
                        }
                        .frame(width: 360)
                        .frame(maxHeight: 500)
                        .presentationBackground(Tok.ink300)
                        .presentationCompactAdaptation(.popover)
                    }
                } else {
                    tipButton(tip)
                }
            }
            control
        }
        .padding(.horizontal, Tok.Space.s4)
        .frame(minHeight: 52)
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private var control: some View {
        switch option.kind {
        case .check:
            Toggle("", isOn: Binding(get: { option.value == "true" }, set: { model.setConfig(option, value: $0 ? "true" : "false") }))
                .toggleStyle(CheckStyle()).labelsHidden()
                .accessibilityLabel(option.label)
        case .number:
            let v = Double(option.value) ?? 0
            HStack(spacing: Tok.Space.s2) {
                // Typed entry for big numbers (enemy armour, evasion), the
                // stepper for the small counts most options are. The text
                // field exists only while editing: fifty live text fields
                // made the tab slow to build, scroll and leave.
                Group {
                    if editing {
                        TextField("0", text: $numberText)
                            .focused($focused)
                            .keyboardType(.decimalPad)
                            .multilineTextAlignment(.trailing)
                            .onSubmit { commitNumber(); editing = false }
                            .onChange(of: focused) { _, f in if !f { commitNumber(); editing = false } }
                            .onAppear { focused = true }
                    } else {
                        Button {
                            numberText = option.value.isEmpty ? "" : Self.trim(v)
                            editing = true
                        } label: {
                            Text(displayNumber(v).isEmpty ? " " : displayNumber(v))
                                .frame(maxWidth: .infinity, alignment: .trailing)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift()
                .padding(.horizontal, 6)
                .frame(width: 84, height: 32)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
                .accessibilityLabel(option.label)
                .accessibilityValue(displayNumber(v))
                HStack(spacing: 6) {
                    // The box is part of the label, not a frame around the
                    // button: a frame outside leaves only the glyph tappable.
                    Button { step(from: v, by: -1) } label: { stepGlyph("minus") }
                        .accessibilityLabel("Decrease \(option.label)")
                    Button { step(from: v, by: 1) } label: { stepGlyph("plus") }
                        .accessibilityLabel("Increase \(option.label)")
                }
                .buttonStyle(.plain).foregroundStyle(Tok.textSecondary).textLift()
            }
        case .list:
            let current = option.choices.first { $0.key == option.value }?.label
                ?? option.choices.first?.label ?? "—"
            DropPill(title: current) {
                ForEach(option.choices, id: \.key) { c in
                    Button {
                        model.setConfig(option, value: c.key)
                    } label: {
                        if c.key == option.value { Label(c.label, systemImage: "checkmark") } else { Text(c.label) }
                    }
                }
            }
        }
    }

    private func tipButton(_ tip: String) -> some View {
        Button { showTip = true } label: {
            Image(systemName: "info.circle").font(.system(size: 15)).foregroundStyle(Tok.textTertiary).textLift()
                .frame(width: 32, height: 32).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("About \(option.label): \(tip)")
    }

    /// What the number box shows: a tap in flight, else PoB's value.
    private func displayNumber(_ v: Double) -> String {
        if let p = pending { return Self.trim(p) }
        return option.value.isEmpty ? "" : Self.trim(v)
    }

    private func commitNumber() {
        commit?.cancel(); commit = nil; pending = nil
        let t = numberText.trimmingCharacters(in: .whitespaces)
        if t.isEmpty { if !option.value.isEmpty { model.setConfig(option, value: "") }; return }
        if let d = Double(t), Self.trim(d) != option.value { model.setConfig(option, value: Self.trim(d)) }
    }

    private func stepGlyph(_ name: String) -> some View {
        Image(systemName: name).font(.system(size: 13, weight: .bold))
            .frame(width: 32, height: 32)
            .contentShape(Rectangle())
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
    }

    /// One stepper tap: show the new number now, recalculate once the taps
    /// stop. PoB's counts do not go below zero, so a minus at zero is a no-op
    /// rather than a recalculation that changes nothing.
    private func step(from current: Double, by delta: Double) {
        let next = max(0, (pending ?? current) + delta)
        pending = next
        commit?.cancel()
        commit = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled, let value = pending else { return }
            pending = nil
            if Self.trim(value) != option.value { model.setConfig(option, value: Self.trim(value)) }
        }
    }

    static func trim(_ d: Double) -> String {
        d == d.rounded() ? String(Int(d)) : String(d)
    }
}

/// PoB's custom modifier blocks: title, enabled, one mod per line.
struct CustomModsCard: View {
    @ObservedObject var model: BuildModel

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack {
                SectionLabel(text: "Custom modifiers")
                Spacer()
                Button("Add mod group") { model.addCustomModBlock() }.buttonStyle(PillStyle())
            }
            Text("One modifier per line, as PoB reads them: \"+10% to all Elemental Resistances\", \"20% increased Spell Damage\".")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            ForEach(Array(model.customMods.enumerated()), id: \.offset) { i, blk in
                CustomModBlockView(model: model, index: i + 1, block: blk, canRemove: model.customMods.count > 1)
            }
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .plate()
    }
}

struct CustomModBlockView: View {
    @ObservedObject var model: BuildModel
    let index: Int
    let block: CustomModBlock
    let canRemove: Bool
    @State private var title = ""
    @State private var text = ""
    @FocusState private var editing: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            HStack(spacing: Tok.Space.s3) {
                TextField("Group name", text: $title, onEditingChanged: { if !$0 { commit() } })
                    .font(Tok.ui(15, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                    .frame(maxWidth: 260)
                Toggle(isOn: Binding(get: { block.enabled }, set: { model.setCustomModBlock(index, title: title, enabled: $0, text: text) })) {
                    Text("Enabled").font(Tok.ui(15)).foregroundStyle(Tok.textPrimary).textLift()
                }
                .toggleStyle(CheckStyle())
                Spacer()
                if canRemove {
                    Button { model.removeCustomModBlock(index) } label: { Image(systemName: "trash") }
                        .buttonStyle(IconPillStyle())
                        .accessibilityLabel("Remove \(block.title)")
                }
            }
            TextEditor(text: $text)
                .focused($editing)
                .font(Tok.mono(13))
                .scrollContentBackground(.hidden)
                .padding(Tok.Space.s2)
                .frame(minHeight: 80)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(editing ? Tok.cyan700 : Tok.ink500, lineWidth: 1))
                .autocorrectionDisabled()
                .onChange(of: editing) { _, e in if !e { commit() } }
        }
        .padding(Tok.Space.s3)
        .background(Tok.ink200)
        .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
        .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
        .onAppear { title = block.title; text = block.text }
        .onChange(of: block.text) { _, t in if !editing { text = t } }
        .onChange(of: block.title) { _, t in title = t }
    }

    /// Written when focus leaves, not per keystroke: every write re-runs
    /// the calculation.
    private func commit() {
        if title != block.title || text != block.text {
            model.setCustomModBlock(index, title: title, enabled: block.enabled, text: text)
        }
    }
}
