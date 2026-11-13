import SwiftUI

/// The PoB options that survive the move to an iPad, edited through the
/// same `main.*` fields desktop PoB uses. Connection, proxy, UI scaling,
/// save path and the like are the system's job here.
struct SettingsView: View {
    @ObservedObject var model: BuildModel
    @ObservedObject var library: BuildLibrary
    @ObservedObject private var theme = ThemeManager.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                HStack {
                    Text("Settings").font(Tok.display(27)).foregroundStyle(Tok.textPrimary).textLift()
                    Spacer()
                    Button("Done") { dismiss() }.buttonStyle(PillStyle(primary: true))
                }

                card("Appearance") {
                    HStack(spacing: Tok.Space.s2) {
                        ForEach(Theme.all) { t in
                            Button {
                                theme.select(t)
                            } label: {
                                Text(t.name)
                                    .font(Tok.ui(15, theme.theme.id == t.id ? .bold : .regular))
                            }
                            .buttonStyle(PillStyle(primary: theme.theme.id == t.id))
                        }
                        Spacer()
                    }
                    Text("Colours for the app. The passive tree keeps its in-game look in every theme.")
                        .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                card("Numbers") {
                    toggle("Show thousands separators", "showThousandsSeparators")
                }
                card("Defaults for new things") {
                    number("Default gem quality", "defaultGemQuality", 0...23)
                    number("Default character level", "defaultCharLevel", 1...100)
                    slider("Default item affix quality", "defaultItemAffixQuality")
                }
                card("Display") {
                    toggle("Show build warnings", "showWarnings")
                    toggle("Item tooltips only for affected slots", "slotOnlyTooltips")
                    toggle("Show search hits at the tree's edge", "edgeSearchHighlight")
                }
                card("Storage") {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("iCloud Drive").font(Tok.ui(17)).foregroundStyle(Tok.textPrimary).textLift()
                            Text(library.usingCloud ? "Builds are in iCloud Drive and sync between your devices." : "Builds are stored on this device. Syncing arrives with the iCloud entitlement.")
                                .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                        }
                        Spacer()
                        Image(systemName: library.usingCloud ? "checkmark.icloud" : "icloud.slash").foregroundStyle(Tok.textTertiary).textLift()
                    }
                    .frame(minHeight: 52)
                }
                Text("PoB's network options (connection protocol, proxy) are not here: the app uses the system's network settings, including any proxy configured on the iPad.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
            }
            .padding(Tok.Space.s6)
        }
        .frame(minWidth: 640, minHeight: 560)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }

    private func card<C: View>(_ title: String, @ViewBuilder _ c: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            SectionLabel(text: title).padding(Tok.Space.s4)
            Rectangle().fill(Tok.ink400).frame(height: 1)
            VStack(spacing: 0) { c() }.padding(.horizontal, Tok.Space.s4)
        }
        .plate()
    }

    private func toggle(_ label: String, _ key: String) -> some View {
        HStack {
            Text(label).font(Tok.ui(17)).foregroundStyle(Tok.textPrimary).textLift()
            Spacer()
            Toggle("", isOn: Binding(get: { model.option(key) == "true" }, set: { model.setOption(key, $0 ? "true" : "false") }))
                .labelsHidden().tint(Tok.cyan500)
                .accessibilityLabel(label)
        }
        .frame(minHeight: 52)
    }

    private func number(_ label: String, _ key: String, _ range: ClosedRange<Int64>) -> some View {
        let v = Int64(Double(model.option(key)) ?? 0)
        return HStack {
            Text(label).font(Tok.ui(17)).foregroundStyle(Tok.textPrimary).textLift()
            Spacer()
            NumStepper(value: v, editable: true, range: range) { model.setOption(key, String($0)) }
        }
        .frame(minHeight: 52)
        .accessibilityElement(children: .contain)
    }

    private func slider(_ label: String, _ key: String) -> some View {
        let v = Double(model.option(key)) ?? 0.5
        return HStack(spacing: Tok.Space.s3) {
            Text(label).font(Tok.ui(17)).foregroundStyle(Tok.textPrimary).textLift()
            Spacer()
            Slider(value: Binding(get: { v }, set: { model.setOption(key, String(format: "%.2f", $0)) }), in: 0...1, step: 0.05)
                .tint(Tok.cyan500).frame(width: 220)
                .accessibilityLabel(label)
            Text("\(Int((v * 100).rounded()))%").font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift().frame(width: 48, alignment: .trailing)
        }
        .frame(minHeight: 52)
    }
}

/// Version, FAQ, and the licences of everything the app is made of. PoB's
/// changelog is not shown: the app's own release notes are on the App
/// Store, and the About page links PoB's releases.
struct AboutView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var page = Page.about
    enum Page: String, CaseIterable, Identifiable { case about = "About", faq = "FAQ", licences = "Licences"; var id: String { rawValue } }

    private static func resource(_ name: String, _ ext: String) -> String {
        Bundle.main.url(forResource: name, withExtension: ext).flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
    }
    private static var pobVersion: String {
        let m = resource("manifest", "xml")
        return m.range(of: #"number="([^"]+)""#, options: .regularExpression).map { String(m[$0]).replacingOccurrences(of: #"number="|""#, with: "", options: .regularExpression) } ?? "?"
    }
    private static var appVersion: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        let b = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        return "\(v) (\(b))"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s4) {
            HStack {
                Text("Path of Building").font(Tok.display(27)).foregroundStyle(Tok.textPrimary).textLift()
                Spacer()
                HStack(spacing: 2) {
                    ForEach(Page.allCases) { p in
                        Button(p.rawValue) { page = p }.buttonStyle(PillStyle(primary: page == p))
                    }
                }
                Button("Done") { dismiss() }.buttonStyle(PillStyle())
            }
            switch page {
            case .about: about
            case .faq: faq
            case .licences: mono(Self.resource("LICENSE", "md") + "\n\n\n" + Self.resource("THIRD-PARTY", "txt"))
            }
        }
        .padding(Tok.Space.s6)
        .frame(minWidth: 720, minHeight: 620)
        .background(Tok.ink300)
        .presentationBackground(Tok.ink300)
    }

    private var about: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Path of Building for iPad \(Self.appVersion)").font(Tok.ui(17, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                    Text("Built on Path of Building Community Fork v\(Self.pobVersion), based on Openarl's Path of Building.")
                        .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    Text("Every calculation is PoB's own code, running on this iPad. The tree, items, skills and configuration are read and written in PoB's file format, so builds move between here and the desktop unchanged.")
                        .font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                }
                .padding(Tok.Space.s4).frame(maxWidth: .infinity, alignment: .leading).plate()

                VStack(alignment: .leading, spacing: Tok.Space.s2) {
                    SectionLabel(text: "Credits")
                    Text("Path of Building: Openarl, and the Path of Building Community contributors.").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    Text("Path of Exile and its data: Grinding Gear Games. This app is not affiliated with Grinding Gear Games.").font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift()
                    Text("Licences for Path of Building, LuaJIT, the Rust crates and the fonts are on the Licences page.").font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                .padding(Tok.Space.s4).frame(maxWidth: .infinity, alignment: .leading).plate()
            }
        }
    }

    /// Questions that cost real people real time. Keep each answer to what
    /// is true today; when the app grows the feature, change the answer.
    private static let faqEntries: [(q: String, a: String)] = [
        ("Signing in to pathofexile.com through Steam fails on iPad.",
         "If the Steam app is installed, iOS hands the Steam sign-in link to it, and the app cannot finish the pathofexile.com login, so you land back where you started. Remove the Steam app (or use a browser that is not the default one), sign in on pathofexile.com, then reinstall Steam. The login stays in the web page and completes normally."),
        ("Where are my builds stored?",
         "In the app's Documents folder, as the same XML files desktop Path of Building writes. The Files app shows them under On My iPad › Path of Building, so they can be copied to a Mac or PC and opened there unchanged. iCloud sync is not on yet."),
        ("How do I get a build in from the desktop, or from a guide?",
         "Paste a share code, raw XML, or a link from pobb.in, poe.ninja, Maxroll, pob.codes, Pastebin, Rentry or poedb into New build. Links of the form pob://pobbin/… open the app directly from Safari or Discord. Getting a build out to the desktop is the XML file in the Files app."),
        ("Can I import a character from my account?",
         "Not yet. Export the character from desktop Path of Building, or from a site like pobb.in, and paste the code here."),
        ("How do I test an item before I buy it?",
         "Copy the item's text, then in the Items tab open \"Create item from text\" and tap Paste. On the trade site, hover an item and use its copy button (or select the item text and copy). In game, hover the item and press Ctrl+C. The item goes into your list unequipped; tap it in a slot to see the exact DPS and defence change before you commit."),
        ("Are the numbers the same as desktop?",
         "Yes. The app runs Path of Building's own calculation code on the iPad; nothing is re-implemented. If a number disagrees with the desktop on the same build and the same PoB version, that is a bug: send the build code."),
        ("Why is the app so large?",
         "It bundles every passive tree version Path of Building knows, so builds from old leagues open without conversion. The trees are most of the download."),
        ("Does it work offline?",
         "Everything except downloading a build from a link and opening trade searches. Calculations, the tree, items, skills and the timeless jewel finder need no connection."),
        ("The tree is slow to redraw right after a change.",
         "Path of Building recalculates the whole build on every allocation. On older iPads that takes a moment; the numbers in the sidebar update when it finishes."),
    ]

    private var faq: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s3) {
                ForEach(Array(Self.faqEntries.enumerated()), id: \.offset) { _, e in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(e.q).font(Tok.ui(15, .bold)).foregroundStyle(Tok.textPrimary).textLift()
                        Text(e.a).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift().textSelection(.enabled)
                    }
                    .padding(Tok.Space.s4).frame(maxWidth: .infinity, alignment: .leading).plate()
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    /// A long text as lazily laid-out chunks. PoB's changelog is 700 KB and
    /// 7,600 lines; as one Text it took the app down on the first tap.
    private func mono(_ text: String) -> some View {
        let chunks: [String] = {
            if text.isEmpty { return ["Not bundled."] }
            var out: [String] = []
            var current: [Substring] = []
            for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
                current.append(line)
                if current.count >= 40 {
                    out.append(current.joined(separator: "\n"))
                    current.removeAll()
                }
            }
            if !current.isEmpty { out.append(current.joined(separator: "\n")) }
            return out
        }()
        return ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(chunks.enumerated()), id: \.offset) { _, chunk in
                    Text(chunk)
                        .font(Tok.mono(12)).foregroundStyle(Tok.textSecondary).textLift()
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(Tok.Space.s4)
        }
        .plate()
    }
}
