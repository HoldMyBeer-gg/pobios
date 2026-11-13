import SwiftUI

/// PoB's Trade tab, offline tier: a "Find best" per slot that builds PoB's own
/// weighted search and opens it on pathofexile.com/trade in the browser.
///
/// The weighting, the query and the URL are all PoB's (via the headless
/// generator). This view is the controls around it: the sort-stat weights, the
/// league / realm / buyout, and the slot list. Live in-app results (Price Item)
/// need the trade API and OAuth; those land in a later pass.
struct TradeSheet: View {
    @ObservedObject var model: BuildModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL

    // Trade settings are the user's, not the build's; shared with the timeless
    // finder so a change in one place holds in the other.
    @AppStorage("timelessLeague") private var league = "Standard"
    @AppStorage("timelessRealm") private var realm = 0
    @AppStorage("timelessBuyout") private var buyout = 0

    /// The sort-stat weights, PoB's default of Full DPS + Effective Hit Pool.
    @State private var weights: [StatWeight] = [
        StatWeight(stat: "FullDPS", weightMult: 1.0),
        StatWeight(stat: "TotalEHP", weightMult: 0.5),
    ]
    @State private var editWeights = false

    /// The trade site's league list, fetched from the endpoint PoB uses.
    @State private var leagues: [(id: String, realm: String)] = []
    private var realmLeagues: [String] {
        let key = ["pc", "sony", "xbox"][min(realm, 2)]
        var seen = Set<String>()
        return leagues.filter { $0.realm == key || $0.realm.isEmpty }.map(\.id).filter { seen.insert($0).inserted }
    }

    /// The last result per slot: a URL to reopen, or PoB's "nothing to search"
    /// note. Keyed by slot name.
    @State private var results: [String: (url: String?, note: String?)] = [:]

    private static let realms = ["PC", "Sony", "Xbox"]
    private static let buyouts = [
        "Instant buyout", "Instant buyout and in person", "In person (online in league)",
        "In person (online)", "Any (includes offline)",
    ]

    /// PoB's `data.powerStatList` keys worth offering as a sort stat, label →
    /// key. Trimmed to the ones people actually weight an upgrade by.
    static let sortStats: [(label: String, stat: String)] = [
        ("Full DPS", "FullDPS"),
        ("Hit DPS", "TotalDPS"),
        ("Combined DPS", "CombinedDPS"),
        ("Average Hit", "AverageDamage"),
        ("DoT DPS", "TotalDot"),
        ("Effective Hit Pool", "TotalEHP"),
        ("Life", "Life"),
        ("Energy Shield", "EnergyShield"),
        ("Mana", "Mana"),
        ("Armour", "Armour"),
        ("Evasion", "Evasion"),
        ("Total Attributes", "TotalAttr"),
    ]

    /// The gear slots to offer, in PoB's order. Tree jewel sockets and grafts
    /// are excluded: an upgrade search there is a different thing.
    private var gearSlots: [Slot] {
        let order = ["Weapon 1", "Weapon 2", "Helmet", "Body Armour", "Gloves", "Boots",
                     "Amulet", "Ring 1", "Ring 2", "Belt",
                     "Flask 1", "Flask 2", "Flask 3", "Flask 4", "Flask 5"]
        let byName = Dictionary(model.equipment.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        return order.compactMap { byName[$0] }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            HStack {
                Text("Trade for an upgrade").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                Button("Close") { dismiss() }.buttonStyle(PillStyle())
            }

            settings

            Text("Find best runs PoB's weighted search for that slot and opens it on the trade site. It weights every possible mod by how much it moves your build, so the results are ranked upgrades, not a fixed filter.")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                .fixedSize(horizontal: false, vertical: true)

            Divider().overlay(Tok.ink500)

            ScrollView { slotList }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 560, minHeight: 640)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .task { await fetchLeagues() }
        .sheet(isPresented: $editWeights) {
            WeightEditor(weights: $weights)
        }
    }

    private var settings: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            // The weighted sort stats: a summary, edited in a sheet.
            HStack(spacing: Tok.Space.s2) {
                Text("SORT BY").font(Tok.label).tracking(Tok.labelTracking).foregroundStyle(Tok.ice700)
                    .frame(width: 84, alignment: .leading)
                Text(weightSummary).font(Tok.mono(13)).foregroundStyle(Tok.textSecondary).textLift()
                    .lineLimit(1).truncationMode(.tail)
                Spacer()
                Button("Adjust weights…") { editWeights = true }.buttonStyle(PillStyle())
            }

            HStack(spacing: Tok.Space.s2) {
                Text("LEAGUE").font(Tok.label).tracking(Tok.labelTracking).foregroundStyle(Tok.ice700)
                    .frame(width: 84, alignment: .leading)
                if realmLeagues.isEmpty {
                    WellField(placeholder: "League", text: $league).frame(maxWidth: 260)
                } else {
                    DropPill(title: league.isEmpty ? "Choose" : league, maxWidth: 260) {
                        ForEach(realmLeagues, id: \.self) { l in
                            Button(l) { league = l }
                        }
                    }
                }
                DropPill(title: "Realm: \(Self.realms[min(realm, 2)])") {
                    ForEach(Array(Self.realms.enumerated()), id: \.offset) { i, r in
                        Button(r) { realm = i }
                    }
                }
            }

            HStack(spacing: Tok.Space.s2) {
                Text("LISTED").font(Tok.label).tracking(Tok.labelTracking).foregroundStyle(Tok.ice700)
                    .frame(width: 84, alignment: .leading)
                DropPill(title: Self.buyouts[min(buyout, Self.buyouts.count - 1)], maxWidth: 300) {
                    ForEach(Array(Self.buyouts.enumerated()), id: \.offset) { i, b in
                        Button(b) { buyout = i }
                    }
                }
            }
        }
    }

    private var slotList: some View {
        VStack(spacing: 0) {
            ForEach(gearSlots, id: \.name) { slot in
                slotRow(slot)
                if slot.name != gearSlots.last?.name {
                    Divider().overlay(Tok.ink500.opacity(0.5))
                }
            }
        }
    }

    private func slotRow(_ slot: Slot) -> some View {
        let r = results[slot.name]
        return HStack(spacing: Tok.Space.s3) {
            VStack(alignment: .leading, spacing: 2) {
                Text(slot.name).font(Tok.ui(15, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                if let item = slot.item {
                    Text(item).font(Tok.ui(13)).foregroundStyle(Color(rrggbb: slot.colour))
                        .lineLimit(1).truncationMode(.tail)
                } else {
                    Text("empty").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                if let note = r?.note {
                    Text(note).font(Tok.ui(12)).foregroundStyle(Tok.coral300)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer()
            if let url = r?.url {
                Button("Copy") { UIPasteboard.general.string = url }.buttonStyle(PillStyle())
                Button("Open") { if let u = URL(string: url) { openURL(u) } }.buttonStyle(PillStyle())
            }
            Button("Find best") { findBest(slot) }
                .buttonStyle(PillStyle(primary: true))
                .disabled(slot.item == nil)
        }
        .padding(.vertical, Tok.Space.s2)
    }

    private var weightSummary: String {
        let labelFor: (String) -> String = { key in
            Self.sortStats.first { $0.stat == key }?.label ?? key
        }
        return weights.map { "\(labelFor($0.stat)) ×\(TradeSheet.fmt($0.weightMult))" }.joined(separator: ", ")
    }

    private func findBest(_ slot: Slot) {
        Task {
        let (url, error) = await model.tradeQuery(
            slot: slot.name, weights: weights, buyout: buyout,
            league: league.trimmingCharacters(in: .whitespaces), realm: realm
        )
        if let url {
            results[slot.name] = (url, nil)
            UIPasteboard.general.string = url
            if let u = URL(string: url) { openURL(u) }
        } else {
            results[slot.name] = (nil, error ?? "No results for this slot.")
        }
        }
    }

    private func fetchLeagues() async {
        guard let url = URL(string: "https://www.pathofexile.com/api/trade/data/leagues"),
              let text = try? await BuildSites.fetch(url),
              let data = text.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = json["result"] as? [[String: Any]] else { return }
        let all = result.compactMap { entry -> (id: String, realm: String)? in
            guard let id = entry["id"] as? String else { return nil }
            return (id, (entry["realm"] as? String ?? "").lowercased())
        }
        guard !all.isEmpty else { return }
        leagues = all
        let ids = realmLeagues
        if !ids.isEmpty && !ids.contains(league) { league = ids[0] }
    }

    static func fmt(_ v: Double) -> String {
        v == v.rounded() ? String(Int(v)) : String(format: "%.2f", v)
    }
}

/// Edit the weighted sort stats: a slider per offered stat, 0 disables it.
/// Mirrors PoB's "Adjust search weights", trimmed to the stats worth weighting.
private struct WeightEditor: View {
    @Binding var weights: [StatWeight]
    @Environment(\.dismiss) private var dismiss

    /// Working copy: every offered stat with its current multiplier (0 = off).
    @State private var values: [String: Double] = [:]

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            HStack {
                Text("Sort weights").font(Tok.display(21)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                Button("Reset") {
                    values = ["FullDPS": 1.0, "TotalEHP": 0.5]
                }.buttonStyle(PillStyle())
            }
            Text("A higher weight makes that stat count for more in the ranking. Zero leaves it out.")
                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()

            ScrollView {
                VStack(spacing: Tok.Space.s3) {
                    ForEach(TradeSheet.sortStats, id: \.stat) { s in
                        HStack(spacing: Tok.Space.s3) {
                            Text(s.label).font(Tok.ui(14)).foregroundStyle(Tok.textPrimary).textLift()
                                .frame(width: 150, alignment: .leading)
                            Slider(value: binding(for: s.stat), in: 0...1, step: 0.05)
                            Text(TradeSheet.fmt(values[s.stat] ?? 0))
                                .font(Tok.mono(13)).foregroundStyle(Tok.textSecondary).textLift()
                                .frame(width: 44, alignment: .trailing)
                        }
                    }
                }
            }

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(PillStyle())
                Button("Apply") {
                    let picked = TradeSheet.sortStats.compactMap { s -> StatWeight? in
                        let v = values[s.stat] ?? 0
                        return v > 0 ? StatWeight(stat: s.stat, weightMult: v) : nil
                    }
                    // Never apply an empty list; fall back to Full DPS.
                    weights = picked.isEmpty ? [StatWeight(stat: "FullDPS", weightMult: 1.0)] : picked
                    dismiss()
                }
                .buttonStyle(PillStyle(primary: true))
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 460, minHeight: 520)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
        .onAppear {
            for w in weights { values[w.stat] = w.weightMult }
        }
    }

    private func binding(for stat: String) -> Binding<Double> {
        Binding(get: { values[stat] ?? 0 }, set: { values[stat] = $0 })
    }
}
