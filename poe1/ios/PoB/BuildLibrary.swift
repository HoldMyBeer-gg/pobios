import SwiftUI
import Foundation

/// One build file in the library, with what the home list shows. Read
/// from the XML without the engine: the engine holds one build, and the
/// list needs a dozen.
struct BuildFile: Identifiable, Hashable {
    let url: URL
    var name: String { url.deletingPathExtension().lastPathComponent }
    var id: String { url.path }
    var className = ""
    var ascendancy = ""
    var level = 1
    var mainSkill = ""
    var dps: Double? = nil        // TotalDPS from the saved stat block
    var ehp: Double? = nil        // effective hit pool
    var modified = Date.distantPast

    /// The headline number for the card, abbreviated (4.3M, 812k).
    static func abbrev(_ v: Double) -> String {
        if v >= 1_000_000 { return String(format: "%.1fM", v / 1_000_000) }
        if v >= 1_000 { return String(format: "%.0fk", v / 1_000) }
        return String(format: "%.0f", v)
    }

    var subtitle: String {
        [className, ascendancy == "None" || ascendancy.isEmpty ? nil : ascendancy, mainSkill.isEmpty ? nil : mainSkill]
            .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// The build library: a folder of PoB XML files, named as PoB names them.
///
/// Lives in the iCloud Documents container when the app has one, else the
/// local Documents folder. Turning iCloud on later is an entitlement change
/// here, not a data migration: the folder layout is the same.
@MainActor
final class BuildLibrary: ObservableObject {
    @Published var builds: [BuildFile] = []
    @Published var usingCloud = false

    let root: URL

    init() {
        let fm = FileManager.default
        if let cloud = fm.url(forUbiquityContainerIdentifier: nil)?.appendingPathComponent("Documents") {
            root = cloud.appendingPathComponent("Builds")
            usingCloud = true
        } else {
            root = fm.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Builds")
        }
        try? fm.createDirectory(at: root, withIntermediateDirectories: true)
        migrateSandboxBuild()
        reload()
    }

    /// Builds made before the library existed lived at Application
    /// Support/build.xml. Bring one in as "Imported build" once.
    private func migrateSandboxBuild() {
        let fm = FileManager.default
        let old = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("build.xml")
        guard fm.fileExists(atPath: old.path) else { return }
        let dest = uniqueURL(for: "Imported build")
        try? fm.moveItem(at: old, to: dest)
    }

    func reload() {
        let fm = FileManager.default
        let urls = (try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey], options: [.skipsHiddenFiles])) ?? []
        builds = urls.filter { $0.pathExtension.lowercased() == "xml" }.map { url in
            var b = BuildFile(url: url)
            b.modified = (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            if let data = fm.contents(atPath: url.path) { Self.readMeta(data, into: &b) }
            return b
        }
    }

    /// A free filename in the library for `name`, adding " 2", " 3" as PoB does.
    func uniqueURL(for name: String) -> URL {
        // No path separators, no leading dot (hidden files never list).
        let clean = name.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
            .trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "."))
        let base = clean.isEmpty ? "New build" : clean
        var candidate = root.appendingPathComponent(base).appendingPathExtension("xml")
        var n = 2
        while FileManager.default.fileExists(atPath: candidate.path) {
            candidate = root.appendingPathComponent("\(base) \(n)").appendingPathExtension("xml")
            n += 1
        }
        return candidate
    }

    /// Write XML to a new library file and return it.
    @discardableResult
    func create(named name: String, xml: String) -> BuildFile? {
        let url = uniqueURL(for: name)
        guard (try? xml.write(to: url, atomically: true, encoding: .utf8)) != nil else { return nil }
        reload()
        // Match by filename: reload() rebuilds URLs from the directory
        // listing, which standardises the path (/private/var vs /var), so
        // == against the URL we wrote can miss though the file is there.
        return builds.first { $0.url.lastPathComponent == url.lastPathComponent }
    }

    func rename(_ b: BuildFile, to name: String) -> BuildFile? {
        // Renaming to its own name is not a rename; uniqueURL would see the
        // file and hand back "Name 2".
        if name.trimmingCharacters(in: .whitespaces) == b.name { return b }
        let dest = uniqueURL(for: name)
        guard (try? FileManager.default.moveItem(at: b.url, to: dest)) != nil else { return nil }
        reload()
        return builds.first { $0.url.lastPathComponent == dest.lastPathComponent }
    }

    func duplicate(_ b: BuildFile) {
        let dest = uniqueURL(for: b.name + " copy")
        try? FileManager.default.copyItem(at: b.url, to: dest)
        reload()
    }

    func delete(_ b: BuildFile) {
        try? FileManager.default.removeItem(at: b.url)
        try? FileManager.default.removeItem(at: snapshotFolder(for: b.url))
        reload()
    }

    // MARK: Snapshots

    /// Where a build's snapshots live: beside the library, not in it, so
    /// the home list never shows them. `reload()` lists only the top level.
    private func snapshotFolder(for url: URL) -> URL {
        root.appendingPathComponent("Snapshots").appendingPathComponent(url.deletingPathExtension().lastPathComponent)
    }

    /// How many snapshots a build keeps; older ones go as new ones arrive.
    static let snapshotLimit = 10

    /// Keep a copy of a build as it is now, before something overwrites it.
    /// An import from the account replaces the tree, items and skills in
    /// one go, and the question "where did my DPS go" needs the before.
    func snapshot(of url: URL, xml: String, reason: String) {
        let fm = FileManager.default
        let folder = snapshotFolder(for: url)
        try? fm.createDirectory(at: folder, withIntermediateDirectories: true)
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH.mm"
        let dest = folder.appendingPathComponent("\(f.string(from: Date())) \(reason)").appendingPathExtension("xml")
        guard (try? xml.write(to: dest, atomically: true, encoding: .utf8)) != nil else { return }
        let extra = snapshots(of: url).dropFirst(Self.snapshotLimit)
        for old in extra { try? fm.removeItem(at: old.url) }
    }

    /// A build's snapshots, newest first.
    func snapshots(of url: URL?) -> [BuildFile] {
        guard let url else { return [] }
        let fm = FileManager.default
        let urls = (try? fm.contentsOfDirectory(at: snapshotFolder(for: url), includingPropertiesForKeys: [.contentModificationDateKey], options: [.skipsHiddenFiles])) ?? []
        return urls.filter { $0.pathExtension.lowercased() == "xml" }.map { u in
            var b = BuildFile(url: u)
            b.modified = (try? u.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            if let data = fm.contents(atPath: u.path) { Self.readMeta(data, into: &b) }
            return b
        }.sorted { $0.modified > $1.modified }
    }

    /// PoB's own empty build, as its New button makes one.
    static let emptyBuildXML = """
    <?xml version="1.0" encoding="UTF-8"?>
    <PathOfBuilding>
    \t<Build level="1" targetVersion="3_0" bandit="None" className="Scion" ascendClassName="None" mainSocketGroup="1" viewMode="TREE"/>
    \t<Skills activeSkillSet="1"><SkillSet id="1"/></Skills>
    \t<Items activeItemSet="1"><ItemSet id="1"/></Items>
    \t<Tree activeSpec="1"><Spec classId="0" ascendClassId="0" treeVersion="3_29" nodes=""/></Tree>
    \t<Config activeConfigSet="1"><ConfigSet id="1"/></Config>
    \t<Notes></Notes>
    </PathOfBuilding>
    """

    // MARK: Metadata

    /// Class, ascendancy, level and the main skill's first active gem, from
    /// the XML alone.
    static func readMeta(_ data: Data, into b: inout BuildFile) {
        let p = XMLParser(data: data)
        let d = MetaDelegate()
        p.delegate = d
        p.parse()
        b.className = d.className
        b.ascendancy = d.ascendancy
        b.level = d.level
        b.mainSkill = d.mainSkill
        // A DPS of 0.06 is PoB's "bare character, never calculated" value;
        // treat it as no number rather than a real zero.
        b.dps = (d.dps ?? 0) > 1 ? d.dps : nil
        b.ehp = (d.ehp ?? 0) > 1 ? d.ehp : nil
    }

    private final class MetaDelegate: NSObject, XMLParserDelegate {
        var className = "", ascendancy = "", level = 1, mainSkill = ""
        var dps: Double? = nil, ehp: Double? = nil
        private var mainGroup = 1, activeSet = "1"
        private var inActiveSet = false, groupIndex = 0, inMainGroup = false

        func parser(_ parser: XMLParser, didStartElement name: String, namespaceURI: String?, qualifiedName: String?, attributes a: [String: String]) {
            switch name {
            case "Build":
                className = a["className"] ?? ""
                ascendancy = a["ascendClassName"] ?? ""
                level = Int(a["level"] ?? "") ?? 1
                mainGroup = Int(a["mainSocketGroup"] ?? "") ?? 1
            case "Skills":
                activeSet = a["activeSkillSet"] ?? "1"
            case "SkillSet":
                inActiveSet = (a["id"] ?? "1") == activeSet
                groupIndex = 0
            case "Skill":
                guard inActiveSet else { return }
                groupIndex += 1
                inMainGroup = groupIndex == mainGroup
            case "Gem":
                guard inMainGroup, mainSkill.isEmpty else { return }
                let id = a["gemId"] ?? ""
                if !id.contains("Support"), let n = a["nameSpec"], !n.isEmpty { mainSkill = n }
            case "PlayerStat":
                // The saved headline stats; the library reads them without
                // loading the engine.
                switch a["stat"] {
                case "TotalDPS": dps = Double(a["value"] ?? "")
                case "TotalEHP", "TotalEffectiveHP": ehp = Double(a["value"] ?? "")
                default: break
                }
            default: break
            }
        }

        func parser(_ parser: XMLParser, didEndElement name: String, namespaceURI: String?, qualifiedName: String?) {
            if name == "Skill" { inMainGroup = false }
            if name == "SkillSet" { inActiveSet = false }
        }
    }
}
