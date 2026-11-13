import SwiftUI

/// The Builds home: the library, as the canvas draws it.
struct HomeView: View {
    @ObservedObject var library: BuildLibrary
    var open: (BuildFile) -> Void
    var newBuild: () -> Void
    var importCode: () -> Void
    var settings: () -> Void
    var about: () -> Void
    @State private var search = ""
    @AppStorage("home.sort") private var sort = Sort.recent
    @State private var renaming: BuildFile?
    @State private var renameTo = ""
    @State private var deleting: BuildFile?

    enum Sort: String, CaseIterable, Identifiable {
        case name = "Name", klass = "Class", recent = "Recent", dps = "DPS", ehp = "EHP"
        var id: String { rawValue }
    }

    private var rows: [BuildFile] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        let filtered = library.builds.filter { b in
            q.isEmpty || b.name.lowercased().contains(q) || b.subtitle.lowercased().contains(q)
                // PoB's "class:assassin" search form.
                || (q.hasPrefix("class:") && (b.className + b.ascendancy).lowercased().contains(q.dropFirst(6)))
        }
        switch sort {
        case .name: return filtered.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        case .klass: return filtered.sorted { ($0.className, $0.ascendancy, $0.name) < ($1.className, $1.ascendancy, $1.name) }
        case .recent: return filtered.sorted { $0.modified > $1.modified }
        case .dps: return filtered.sorted { ($0.dps ?? -1) > ($1.dps ?? -1) }
        case .ehp: return filtered.sorted { ($0.ehp ?? -1) > ($1.ehp ?? -1) }
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tok.Space.s4) {
                HStack {
                    Text("Builds").font(Tok.display(38)).foregroundStyle(Tok.textPrimary).textLift()
                    Spacer()
                    Button("New build", action: newBuild).buttonStyle(PillStyle(primary: true))
                    Button("Import…", action: importCode).buttonStyle(PillStyle())
                    Button(action: settings) { Image(systemName: "gearshape").font(.system(size: 17)) }
                        .buttonStyle(IconPillStyle()).accessibilityLabel("Settings")
                    Button(action: about) { Image(systemName: "info.circle").font(.system(size: 17)) }
                        .buttonStyle(IconPillStyle()).accessibilityLabel("About")
                }
                HStack(spacing: Tok.Space.s3) {
                    WellField(placeholder: "Search — e.g. class:occultist winter", text: $search)
                    HStack(spacing: 2) {
                        ForEach(Sort.allCases) { s in
                            Button(s.rawValue) { sort = s }
                                .font(Tok.ui(15, .medium))
                                .foregroundStyle(sort == s ? Tok.textPrimary : Tok.textSecondary).textLift()
                                .frame(height: 40).padding(.horizontal, Tok.Space.s3)
                                .background(sort == s ? Tok.ink500 : .clear)
                                .clipShape(RoundedRectangle(cornerRadius: 3))
                                .buttonStyle(.plain)
                                .accessibilityAddTraits(sort == s ? [.isSelected] : [])
                        }
                    }
                    .padding(2)
                    .background(Tok.ink100)
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                    .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
                }

                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.element.id) { i, b in
                        Button { open(b) } label: {
                            HStack(spacing: Tok.Space.s3) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(b.name).font(Tok.display(19)).foregroundStyle(Tok.textPrimary).textLift().lineLimit(1)
                                    Text(b.subtitle.isEmpty ? "Empty build" : b.subtitle).font(Tok.ui(15)).foregroundStyle(Tok.textSecondary).textLift().lineLimit(1)
                                }
                                Spacer()
                                if let dps = b.dps {
                                    VStack(alignment: .trailing, spacing: 2) {
                                        Text("\(BuildFile.abbrev(dps)) DPS").font(Tok.mono(15)).foregroundStyle(Tok.cyan300)
                                        if let ehp = b.ehp {
                                            Text("\(BuildFile.abbrev(ehp)) EHP").font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift()
                                        }
                                    }
                                    .frame(width: 110, alignment: .trailing)
                                }
                                Text("Lv \(b.level)").font(Tok.mono(15)).foregroundStyle(Tok.textPrimary).textLift()
                                Text(Self.when(b.modified)).font(Tok.mono(13)).foregroundStyle(Tok.textTertiary).textLift().frame(width: 80, alignment: .trailing)
                                Image(systemName: "chevron.right").font(.system(size: 12, weight: .bold)).foregroundStyle(Tok.textTertiary).textLift()
                            }
                            .padding(.horizontal, Tok.Space.s5).frame(height: 84).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(b.name), \(b.subtitle), level \(b.level)" + (b.dps.map { ", \(BuildFile.abbrev($0)) DPS" } ?? ""))
                        .contextMenu {
                            Button("Rename…") { renaming = b; renameTo = b.name }
                            Button("Duplicate") { library.duplicate(b) }
                            Button("Delete", role: .destructive) { deleting = b }
                        }
                        if i < rows.count - 1 { Rectangle().fill(Tok.ink400).frame(height: 1) }
                    }
                    if rows.isEmpty {
                        Text(library.builds.isEmpty ? "No builds yet. Import a code or a character, or start a new build." : "Nothing matches.")
                            .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift().padding(Tok.Space.s5)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .plate()

                Text(library.usingCloud ? "Stored in iCloud Drive. Works offline; syncs when it can." : "Stored on this device.")
                    .font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                Text("Long-press a build to rename, duplicate or delete it.")
                    .font(Tok.ui(13)).foregroundStyle(Tok.textDisabled)
            }
            .frame(maxWidth: 1280)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, Tok.Space.s6).padding(.top, Tok.Space.s6 * 2).padding(.bottom, Tok.Space.s6)
        }
        .alert("Rename build", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $renameTo)
            Button("Save") { if let r = renaming { _ = library.rename(r, to: renameTo) }; renaming = nil }
            Button("Cancel", role: .cancel) { renaming = nil }
        }
        .confirmationDialog("Delete \"\(deleting?.name ?? "")\"?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible) {
            Button("Delete build", role: .destructive) { if let d = deleting { library.delete(d) }; deleting = nil }
        }
    }

    static func when(_ d: Date) -> String {
        let s = Date().timeIntervalSince(d)
        if s < 60 { return "now" }
        if s < 3600 { return "\(Int(s / 60))m ago" }
        if s < 86400 { return "\(Int(s / 3600))h ago" }
        if s < 7 * 86400 { return "\(Int(s / 86400))d ago" }
        if s < 30 * 86400 { return "\(Int(s / (7 * 86400)))w ago" }
        let f = DateFormatter(); f.dateFormat = "MMM d"
        return f.string(from: d)
    }
}
