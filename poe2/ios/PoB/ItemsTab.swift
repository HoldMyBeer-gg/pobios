import SwiftUI

/// Equipped slots on the left, the build's items on the right, as the
/// canvas lays it out. Tapping a slot opens its item in a popover with the
/// candidates for that slot and what each would change.
struct ItemsTab: View {
    @ObservedObject var model: BuildModel
    @State private var search = ""
    @State private var showCreate = false
    @State private var showCraft = false
    @State private var manageSets = false
    @State private var showTrade = false
    @State private var mode = LibraryMode.build
    @State private var typeFilter = "Any slot"
    @State private var uniquePick: DbItem?

    enum LibraryMode: String, CaseIterable, Identifiable {
        case build = "Build items", uniques = "Uniques"
        var id: String { rawValue }
    }

    var body: some View {
        // Two columns when they fit; stacked when they do not. Fixed minimum
        // widths in an HStack overflow in portrait and push the sidebar off
        // screen, so nothing here has a minimum the window cannot honour.
        GeometryReader { geo in
            if geo.size.width >= 880 {
                HStack(spacing: 0) {
                    equipped
                        // The flexible column: names are long and the canvas
                        // already truncates them at full width.
                        .frame(maxWidth: .infinity)
                    Rectangle().fill(Tok.ink500).frame(width: 1)
                    library
                        .frame(width: min(560, geo.size.width * 0.46))
                }
            } else {
                // Each half keeps its own scroller and takes half the height;
                // nested vertical scroll views collapse to nothing.
                VStack(spacing: 0) {
                    equipped.frame(maxHeight: .infinity)
                    Rectangle().fill(Tok.ink500).frame(height: 1)
                    library.frame(maxHeight: .infinity)
                }
            }
        }
        .sheet(isPresented: $showTrade) { TradeSheet(model: model) }
        .sheet(isPresented: $showCreate) { CreateItemSheet(model: model) }
        .sheet(isPresented: $showCraft) { CraftItemSheet(model: model) }
        .sheet(isPresented: $manageSets) {
            SetManageSheet(title: "Item sets", sets: model.itemSets,
                           onNew: { model.newItemSet($0) },
                           onRename: { model.renameItemSet($0, to: $1) },
                           onDelete: { model.deleteItemSet($0) },
                           onSelect: { model.switchItemSet($0) })
        }
        .sheet(item: $uniquePick) { u in UniqueSheet(model: model, item: u) }
    }

    // MARK: Equipped

    private var equipped: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                HStack {
                    SectionLabel(text: "Equipped")
                    Spacer()
                    Text("Weapon set").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                    // PoB's I / II: the main socket group follows when it
                    // is socketed in the other set.
                    HStack(spacing: 2) {
                        Button("I") { model.setWeaponSet(second: false) }
                            .frame(width: 40, height: 32)
                            .background(model.weaponSetSecond ? .clear : Tok.ink300)
                            .foregroundStyle(model.weaponSetSecond ? Tok.textTertiary : Tok.textPrimary).textLift()
                        Button("II") { model.setWeaponSet(second: true) }
                            .frame(width: 40, height: 32)
                            .background(model.weaponSetSecond ? Tok.ink300 : .clear)
                            .foregroundStyle(model.weaponSetSecond ? Tok.textPrimary : Tok.textTertiary).textLift()
                    }
                    .buttonStyle(.plain)
                    .font(Tok.mono(13, .bold))
                    .padding(2)
                    .background(Tok.ink100)
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                    .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel("Weapon set \(model.weaponSetSecond ? "II" : "I")")
                }

                VStack(spacing: 0) {
                    ForEach(Array(model.equipment.enumerated()), id: \.offset) { i, slot in
                        SlotRow(model: model, slot: slot)
                        if i < model.equipment.count - 1 {
                            Rectangle().fill(Tok.ink400).frame(height: 1)
                        }
                    }
                }
                .plate()

                SectionLabel(text: "Shared items").padding(.top, Tok.Space.s2)
                Text("Items shared across builds arrive with the build library.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                    .padding(Tok.Space.s4).frame(maxWidth: .infinity, alignment: .leading).plate()
            }
            .padding(Tok.Space.s4)
        }
    }

    // MARK: Library

    private var filtered: [Item] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? model.items : model.items.filter { $0.name.lowercased().contains(q) }
    }

    private var uniqueTypes: [String] {
        ["Any slot"] + Array(Set(model.uniqueCatalogue.map(\.itemType))).sorted()
    }

    private var filteredUniques: [DbItem] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return model.uniqueCatalogue.filter { u in
            (typeFilter == "Any slot" || u.itemType == typeFilter)
                && (q.isEmpty || u.name.lowercased().contains(q) || u.base.lowercased().contains(q))
        }
    }

    private var library: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                FlowRow(spacing: Tok.Space.s3) {
                    DropPill(title: "Item set: \(model.itemSets.first(where: \.active)?.title ?? "Default")") {
                        ForEach(model.itemSets, id: \.id) { set in
                            Button(set.title) { model.switchItemSet(set.id) }
                        }
                    }
                    Button("Manage…") { manageSets = true }.buttonStyle(PillStyle())
                    // PoB's weighted "Find best" per slot. The query is built
                    // locally; it opens pathofexile.com/trade in the browser.
                    Button("Trade for an upgrade…") { showTrade = true }.buttonStyle(PillStyle())
                }

                // Two pills rather than a system segmented control, which
                // is near-invisible on this palette.
                HStack(spacing: Tok.Space.s2) {
                    ForEach(LibraryMode.allCases) { m in
                        Button(m.rawValue) { mode = m }
                            .buttonStyle(PillStyle(primary: mode == m))
                            .accessibilityAddTraits(mode == m ? [.isSelected] : [])
                    }
                    Spacer()
                }

                HStack(spacing: Tok.Space.s3) {
                    WellField(placeholder: mode == .build ? "Search items" : "Search uniques", text: $search)
                        .accessibilityLabel("Search")
                    if mode == .uniques {
                        DropPill(title: typeFilter) {
                            ForEach(uniqueTypes, id: \.self) { t in Button(t) { typeFilter = t } }
                        }
                    }
                }

                if mode == .build {
                    FlowRow(spacing: Tok.Space.s3) {
                        Button("Craft item…") { showCraft = true }.buttonStyle(PillStyle())
                        Button("Create from text…") { showCreate = true }.buttonStyle(PillStyle())
                        Button("Delete unused") {
                            for item in model.items where item.wornIn.isEmpty { model.delete(item: item.id) }
                        }
                        .buttonStyle(PillStyle(danger: true))
                        .disabled(!model.items.contains { $0.wornIn.isEmpty })
                    }

                    SectionLabel(text: "All items · \(filtered.count)").padding(.top, Tok.Space.s2)
                    VStack(spacing: 0) {
                        ForEach(Array(filtered.enumerated()), id: \.element.id) { i, item in
                            LibraryRow(model: model, item: item)
                            if i < filtered.count - 1 { Rectangle().fill(Tok.ink400).frame(height: 1) }
                        }
                        if filtered.isEmpty {
                            Text(search.isEmpty ? "No items in this build." : "Nothing matches.")
                                .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s4)
                        }
                    }
                    .plate()
                } else {
                    SectionLabel(text: "Uniques · \(filteredUniques.count)").padding(.top, Tok.Space.s2)
                    if model.uniqueCatalogue.isEmpty {
                        Text("PoB's unique database did not load.").font(Tok.ui(15)).foregroundStyle(Tok.coral300).padding(Tok.Space.s4)
                    }
                    LazyVStack(spacing: 0) {
                        ForEach(Array(filteredUniques.prefix(300).enumerated()), id: \.element.name) { i, u in
                            Button { uniquePick = u } label: {
                                HStack(spacing: Tok.Space.s3) {
                                    Text(u.name).font(Tok.ui(17)).foregroundStyle(Color(rrggbb: u.colour)).lineLimit(1)
                                    Spacer()
                                    Text(u.base).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().lineLimit(1)
                                }
                                .padding(.horizontal, Tok.Space.s4).frame(height: 48).contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("\(u.name), \(u.base)")
                            if i < min(300, filteredUniques.count) - 1 { Rectangle().fill(Tok.ink400).frame(height: 1) }
                        }
                        if filteredUniques.count > 300 {
                            Text("Showing 300. Search or filter to narrow it down.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s3)
                        }
                    }
                    .plate()
                }
            }
            .padding(Tok.Space.s4)
        }
    }
}

/// "WEAPON 1   Foulborn Tulfall, Opal Wand" with the flask check where PoB
/// has one. Tap for the item and the slot's candidates.
struct SlotRow: View {
    @ObservedObject var model: BuildModel
    let slot: Slot
    @State private var open = false

    var body: some View {
        Button { open = true } label: {
            HStack(spacing: Tok.Space.s3) {
                Text(slot.label.uppercased())
                    .font(Tok.label).tracking(1.6).foregroundStyle(Tok.ice700)
                    .lineLimit(2).minimumScaleFactor(0.8)
                    .frame(width: 110, alignment: .leading)
                Text(slot.item ?? "None")
                    .font(Tok.ui(17))
                    .foregroundStyle(slot.item == nil ? Tok.textTertiary : Color(rrggbb: slot.colour)).textLift()
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: Tok.Space.s2)
                if let active = slot.flaskActive {
                    Toggle("", isOn: Binding(get: { active }, set: { model.setFlask(slot.name, active: $0) }))
                        .toggleStyle(CheckStyle()).labelsHidden()
                        .disabled(slot.item == nil)
                        .accessibilityLabel("\(slot.name) active")
                }
            }
            .padding(.horizontal, Tok.Space.s4)
            .frame(height: 56)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(slot.name): \(slot.item ?? "empty")")
        .popover(isPresented: $open, arrowEdge: .leading) {
            ItemPopover(model: model, slot: slot)
        }
    }
}

/// The item card plus the slot's candidates, each with its preview deltas.
struct ItemPopover: View {
    @ObservedObject var model: BuildModel
    let slot: Slot
    @Environment(\.dismiss) private var dismiss
    @State private var previewFor: Int64?
    @State private var editing: Int64?
    /// Read off the engine in `.task`, not `body`: the preview is a full
    /// recalculation and the popover re-renders on every publish.
    @State private var detail: ItemDetail?
    @State private var candidates: [Item] = []
    @State private var preview: ItemPreview?

    /// The id of what is worn here, from the build's item list.
    private var equippedId: Int64? {
        model.items.first { $0.wornIn.contains(slot.name) }?.id
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                if let d = detail {
                    ItemCard(detail: d)
                    if let id = model.equipment.first(where: { $0.name == slot.name }).flatMap({ _ in equippedId }) {
                        Button("Edit item text…") { editing = id }.buttonStyle(PillStyle())
                    }
                } else {
                    Text("Nothing equipped in \(slot.name).").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                }

                SectionLabel(text: "Equip instead")
                VStack(spacing: 0) {
                    if slot.item != nil {
                        candidateRow(name: "None", colour: Tok.textTertiary, id: 0)
                        Rectangle().fill(Tok.ink400).frame(height: 1)
                    }
                    ForEach(Array(candidates.enumerated()), id: \.element.id) { i, c in
                        candidateRow(name: c.name, colour: Color(rrggbb: c.colour), id: c.id)
                        if i < candidates.count - 1 { Rectangle().fill(Tok.ink400).frame(height: 1) }
                    }
                    if candidates.isEmpty && slot.item == nil {
                        Text("No items in the build fit this slot.").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s3)
                    }
                }
                .plate()
            }
            .padding(Tok.Space.s5)
        }
        .frame(width: 440)
        .frame(minHeight: 200, maxHeight: 700)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .task(id: model.refreshCount) {
            detail = model.itemDetail(in: slot.name)
            candidates = model.candidates(for: slot.name)
        }
        .task(id: "\(previewFor ?? -1)/\(model.refreshCount)") {
            preview = previewFor.flatMap { $0 == 0 ? nil : model.itemPreview($0, in: slot.name) }
        }
        .sheet(item: Binding(get: { editing.map { EditTarget(id: $0) } }, set: { editing = $0?.id })) { t in
            EditItemSheet(model: model, itemId: t.id)
        }
    }

    private func candidateRow(name: String, colour: Color, id: Int64) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Button {
                model.equip(id, in: slot.name)
                dismiss()
            } label: {
                HStack {
                    Text(name).font(Tok.ui(15, .medium)).foregroundStyle(colour).lineLimit(1)
                    Spacer()
                    if id != 0 {
                        Button { previewFor = previewFor == id ? nil : id } label: {
                            Text(previewFor == id ? "Hide" : "Compare")
                                .font(Tok.ui(13, .bold)).foregroundStyle(Tok.cyan500)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Compare \(name)")
                    }
                }
                .padding(.horizontal, Tok.Space.s3).frame(height: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Equip \(name)")

            if previewFor == id, id != 0, let p = preview {
                ChangesList(preview: p).padding(.horizontal, Tok.Space.s3).padding(.bottom, Tok.Space.s2)
            }
        }
    }
}

/// PoB's item tooltip, left half: name, base, requirements, mods, flavour.
struct ItemCard: View {
    let detail: ItemDetail

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s2) {
            Text(detail.name).font(Tok.display(21)).foregroundStyle(Color(rrggbb: detail.colour))
            if detail.base != detail.name, !detail.base.isEmpty {
                Text(detail.base).font(Tok.ui(15)).foregroundStyle(Color(rrggbb: detail.colour).opacity(0.8))
            }
            HStack(spacing: Tok.Space.s3) {
                Text(detail.rarity).font(Tok.label).tracking(1.2).foregroundStyle(Tok.textTertiary).textLift()
                if detail.itemLevel > 0 { meta("ilvl", detail.itemLevel) }
                if detail.quality > 0 { meta("quality", detail.quality, suffix: "%") }
            }
            if let lvl = detail.reqLevel {
                HStack(spacing: Tok.Space.s2) {
                    Text("Requires level \(lvl)").font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift()
                    if let v = detail.reqStr { Text("\(v) Str").font(Tok.mono(13)).foregroundStyle(Tok.socketRed) }
                    if let v = detail.reqDex { Text("\(v) Dex").font(Tok.mono(13)).foregroundStyle(Tok.socketGreen) }
                    if let v = detail.reqInt { Text("\(v) Int").font(Tok.mono(13)).foregroundStyle(Tok.socketBlue) }
                }
            }
            Rectangle().fill(Tok.ink400).frame(height: 1).padding(.vertical, 4)
            ForEach(Array(detail.mods.enumerated()), id: \.offset) { _, m in
                HStack(alignment: .firstTextBaseline, spacing: Tok.Space.s2) {
                    Text(m.text)
                        .font(Tok.ui(15))
                        .foregroundStyle(m.unsupported ? Tok.coral300 : m.kind == "implicit" ? Tok.textSecondary : Tok.socketBlue.opacity(0.9)).textLift()
                    if m.kind == "crafted" || m.kind == "enchant" || m.kind == "scourge" || m.kind == "crucible" {
                        Text(m.kind).font(Tok.label).tracking(1).foregroundStyle(Tok.textTertiary).textLift()
                    }
                    if m.unsupported {
                        Text("not supported in PoB yet").font(Tok.ui(13)).foregroundStyle(Tok.coral300)
                    }
                }
                .accessibilityElement(children: .combine)
            }
            if let f = detail.flavour {
                Text(f).font(Tok.ui(15).italic()).foregroundStyle(Tok.violet400.opacity(0.8)).padding(.top, 4)
            }
        }
        .padding(Tok.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .plate()
    }

    private func meta(_ k: String, _ v: Int64, suffix: String = "") -> some View {
        HStack(spacing: 4) {
            Text(k).font(Tok.label).tracking(1.2).foregroundStyle(Tok.textTertiary).textLift()
            Text("\(v)\(suffix)").font(Tok.mono(13)).foregroundStyle(Tok.textSecondary).textLift()
        }
    }
}

/// "Equipping changes" rows: green up, coral down.
struct ChangesList: View {
    let preview: ItemPreview
    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(preview.isEquipped ? "Removing changes" : "Equipping changes" + (preview.replacing.map { " (replacing \($0))" } ?? ""))
                .font(Tok.label).tracking(1.2).foregroundStyle(Tok.textTertiary).textLift()
            if preview.changes.isEmpty {
                Text("No change.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
            ForEach(Array(preview.changes.enumerated()), id: \.offset) { _, c in
                HStack(spacing: Tok.Space.s2) {
                    Text(c.value).font(Tok.mono(13)).foregroundStyle(c.value.hasPrefix("-") ? Tok.coral300 : Tok.ok)
                    Text(c.label).font(Tok.ui(13)).foregroundStyle(Tok.textSecondary).textLift()
                    if let p = c.percent {
                        Text(String(format: "(%@%.1f%%)", p >= 0 ? "+" : "", p)).font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift()
                    }
                }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// One item in the library: name, where it is worn, delete.
struct LibraryRow: View {
    @ObservedObject var model: BuildModel
    let item: Item

    var body: some View {
        HStack {
            Text(item.name).font(Tok.ui(17)).foregroundStyle(Color(rrggbb: item.colour)).lineLimit(1)
            Spacer()
            Text(item.wornIn.isEmpty ? "unused" : item.wornIn.joined(separator: ", "))
                .font(Tok.ui(13)).foregroundStyle(item.wornIn.isEmpty ? Tok.textDisabled : Tok.textTertiary).textLift()
                .lineLimit(1)
            Button { model.delete(item: item.id) } label: {
                Image(systemName: "xmark").font(.system(size: 12, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
            }
            .frame(width: 44, height: 44)
            .accessibilityLabel("Delete \(item.name)")
        }
        .padding(.leading, Tok.Space.s4)
        .frame(height: 52)
        .accessibilityElement(children: .contain)
    }
}

/// Paste an item's text from the game or a trade site.
struct CreateItemSheet: View {
    @ObservedObject var model: BuildModel
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Create item from text").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            // Spell out where the text comes from: this is the step people get
            // stuck on, and there is no obvious answer from the field alone.
            VStack(alignment: .leading, spacing: Tok.Space.s2) {
                Text("Paste an item's text to test it. Where to copy it from:")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                Label("Trade site: hover the item and use its copy button, or select the item text and copy.",
                      systemImage: "1.circle").font(Tok.ui(14)).foregroundStyle(Tok.textTertiary).textLift()
                Label("In game: hover the item and press Ctrl+C.",
                      systemImage: "2.circle").font(Tok.ui(14)).foregroundStyle(Tok.textTertiary).textLift()
                Label("Then tap Paste below. It joins your item list unequipped; tap it in a slot to see the DPS change.",
                      systemImage: "3.circle").font(Tok.ui(14)).foregroundStyle(Tok.textTertiary).textLift()
            }
            HStack(spacing: Tok.Space.s2) {
                Button {
                    if let s = UIPasteboard.general.string, !s.isEmpty { text = s; error = nil }
                } label: {
                    Label("Paste from clipboard", systemImage: "doc.on.clipboard")
                }
                .buttonStyle(PillStyle())
                if !text.isEmpty {
                    Button("Clear") { text = ""; error = nil }.buttonStyle(PillStyle())
                }
                Spacer()
            }
            TextEditor(text: $text)
                .font(Tok.mono(13))
                .scrollContentBackground(.hidden)
                .padding(Tok.Space.s2)
                .frame(minHeight: 240)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(error == nil ? Tok.ink500 : Tok.coral500, lineWidth: 1))
                .overlay(alignment: .topLeading) {
                    if text.isEmpty {
                        Text("Item text appears here…")
                            .font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift()
                            .padding(Tok.Space.s3).allowsHitTesting(false)
                    }
                }
            if let error { Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Add to build") {
                    error = model.addItem(text: text)
                    if error == nil { dismiss() }
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 560)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }
}

struct EditTarget: Identifiable { let id: Int64 }

/// Unique names are unique in PoB's database, which is what keys it.
extension DbItem: Identifiable {
    public var id: String { name }
}

/// PoB's edit-as-text: the item as PoB writes it, saved back in place.
struct EditItemSheet: View {
    @ObservedObject var model: BuildModel
    let itemId: Int64
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Edit item text").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            Text("This is the item as PoB stores it. Change a mod line, a value, the quality, anything; it keeps its slot.")
                .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
            TextEditor(text: $text)
                .font(Tok.mono(13))
                .scrollContentBackground(.hidden)
                .padding(Tok.Space.s2)
                .frame(minHeight: 320)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(error == nil ? Tok.ink500 : Tok.coral500, lineWidth: 1))
                .autocorrectionDisabled()
            if let error { Text(error).font(Tok.ui(13)).foregroundStyle(Tok.coral300) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Save") {
                    error = model.replaceItem(itemId, text: text)
                    if error == nil { dismiss() }
                }
                .buttonStyle(PillStyle(primary: true))
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 600)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear { text = model.itemRaw(itemId) ?? "" }
    }
}

/// A database unique: its card, and Add / Add & equip.
struct UniqueSheet: View {
    @ObservedObject var model: BuildModel
    let item: DbItem
    @Environment(\.dismiss) private var dismiss
    @State private var detail: ItemDetail?
    @State private var previewSlot: String?
    @State private var preview: ItemPreview?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                if let d = detail {
                    ItemCard(detail: d)
                } else {
                    Text(item.name).font(Tok.display(21)).foregroundStyle(Color(rrggbb: item.colour))
                }
                // PoB's tooltip for a database item: what equipping it in
                // its primary slot would change.
                if let slot = previewSlot, let p = preview {
                    VStack(alignment: .leading, spacing: Tok.Space.s2) {
                        Text("In \(slot)").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        ChangesList(preview: p)
                    }
                    .padding(Tok.Space.s3).frame(maxWidth: .infinity, alignment: .leading).plate()
                }
                HStack(spacing: Tok.Space.s3) {
                    if let l = item.league { Text(l).font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift() }
                    Spacer()
                    Button("Close") { dismiss() }.buttonStyle(PillStyle())
                    Button("Add to build") { model.addUnique(item.name, equip: false); dismiss() }.buttonStyle(PillStyle())
                    Button("Add & equip") { model.addUnique(item.name, equip: true); dismiss() }.buttonStyle(PillStyle(primary: true))
                }
            }
            .padding(Tok.Space.s6)
        }
        .frame(minWidth: 520, minHeight: 320)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .task(id: model.refreshCount) {
            detail = model.uniqueDetail(item.name)
            if let raw = model.uniqueRaw(item.name), let slot = model.primarySlot(of: raw) {
                previewSlot = slot
                preview = model.itemPreview(text: raw, in: slot)
            }
        }
    }
}

/// PoB's Craft item popup: type, base, rarity, name.
struct CraftItemSheet: View {
    @ObservedObject var model: BuildModel
    @Environment(\.dismiss) private var dismiss
    @State private var type = ""
    @State private var base = ""
    @State private var rarity = "RARE"
    @State private var title = ""

    private var types: [String] {
        var seen: [String] = []
        for b in model.craftBases where !seen.contains(b.itemType) { seen.append(b.itemType) }
        return seen
    }
    private var bases: [String] { model.craftBases.filter { $0.itemType == type }.map(\.name) }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            Text("Craft item").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
            row("Rarity") {
                Picker("Rarity", selection: $rarity) {
                    Text("Normal").tag("NORMAL"); Text("Magic").tag("MAGIC"); Text("Rare").tag("RARE")
                }
                .pickerStyle(.segmented).frame(width: 300)
            }
            if rarity == "RARE" {
                row("Name") { WellField(placeholder: "New Item", text: $title) }
            }
            row("Type") {
                DropPill(title: type.isEmpty ? "Choose…" : type) {
                    ForEach(types, id: \.self) { t in Button(t) { type = t; base = bases.first ?? "" } }
                }
            }
            row("Base") {
                DropPill(title: base.isEmpty ? "Choose…" : base, enabled: !type.isEmpty) {
                    ForEach(bases, id: \.self) { b in Button(b) { base = b } }
                }
            }
            Text("The item is created with its implicit and no explicit mods. Edit its text afterwards to add mods, as PoB's editor would.")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Create") {
                    model.craftItem(type: type, base: base, rarity: rarity, title: title)
                    dismiss()
                }
                .buttonStyle(PillStyle(primary: true))
                .disabled(type.isEmpty || base.isEmpty)
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 560)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear { if type.isEmpty, let t = types.first { type = t; base = bases.first ?? "" } }
    }

    private func row<C: View>(_ label: String, @ViewBuilder _ c: () -> C) -> some View {
        HStack(spacing: Tok.Space.s3) {
            Text(label).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift().frame(width: 70, alignment: .leading)
            c()
            Spacer()
        }
    }
}
