import Foundation

/// The parts of another build's XML the Compare tab shows side by side:
/// its sets, items, socket groups, config and trees. Read here rather
/// than through the engine, which holds one build; the engine only parses
/// the item texts, so the mods are still PoB's reading.
struct OtherBuild {
    struct Gem { let name: String; let level: Int; let quality: Int; let enabled: Bool; let support: Bool }
    struct Group { let slot: String?; let enabled: Bool; let gems: [Gem]; let main: Bool }
    struct Spec { let title: String; let nodes: [UInt64]; let version: String; let url: String? }
    struct NamedSet<T> { let id: String; let title: String; let value: T }

    var className = "", ascendancy = "", level = 1
    /// Item id → PoB item text.
    var items: [String: String] = [:]
    /// Item sets: slot name → item id, plus the second weapon set flag.
    var itemSets: [NamedSet<[String: String]>] = []
    var activeItemSet = "1"
    var skillSets: [NamedSet<[Group]>] = []
    var activeSkillSet = "1"
    /// Config sets: input name → value text ("true"/"false", number, string).
    var configSets: [NamedSet<[String: String]>] = []
    var activeConfigSet = "1"
    var specs: [Spec] = []
    var activeSpec = 1
    var mainSocketGroup = 1

    var activeSlots: [String: String] { itemSets.first { $0.id == activeItemSet }?.value ?? itemSets.first?.value ?? [:] }
    var activeGroups: [Group] { skillSets.first { $0.id == activeSkillSet }?.value ?? skillSets.first?.value ?? [] }
    var activeConfig: [String: String] { configSets.first { $0.id == activeConfigSet }?.value ?? configSets.first?.value ?? [:] }

    static func parse(_ xml: String) -> OtherBuild? {
        guard let data = xml.data(using: .utf8) else { return nil }
        let p = XMLParser(data: data)
        let d = Delegate()
        p.delegate = d
        guard p.parse() else { return nil }
        return d.build
    }

    private final class Delegate: NSObject, XMLParserDelegate {
        var build = OtherBuild()
        private var path: [String] = []
        private var itemId = "", itemText = ""
        private var setId = "", setTitle = "", slots: [String: String] = [:]
        private var groups: [Group] = [], gems: [Gem] = [], groupSlot: String?, groupEnabled = true, groupMain = false
        private var config: [String: String] = [:]
        private var specTitle = "", specNodes = "", specVersion = "", specURL = "", inURL = false

        func parser(_ parser: XMLParser, didStartElement n: String, namespaceURI: String?, qualifiedName: String?, attributes a: [String: String]) {
            path.append(n)
            switch n {
            case "Build":
                build.className = a["className"] ?? ""; build.ascendancy = a["ascendClassName"] ?? ""
                build.level = Int(a["level"] ?? "") ?? 1; build.mainSocketGroup = Int(a["mainSocketGroup"] ?? "") ?? 1
            case "Items": build.activeItemSet = a["activeItemSet"] ?? "1"
            case "Item": itemId = a["id"] ?? ""; itemText = ""
            case "ItemSet": setId = a["id"] ?? "1"; setTitle = a["title"] ?? "Default"; slots = [:]
            case "Slot":
                if path.contains("ItemSet"), let id = a["itemId"], id != "0", let name = a["name"] { slots[name] = id }
            case "Skills": build.activeSkillSet = a["activeSkillSet"] ?? "1"
            case "SkillSet": setId = a["id"] ?? "1"; setTitle = a["title"] ?? "Default"; groups = []
            case "Skill":
                gems = []; groupSlot = a["slot"].flatMap { $0.isEmpty ? nil : $0 }
                groupEnabled = a["enabled"] != "false"; groupMain = a["mainActiveSkill"] == "1"
            case "Gem":
                gems.append(Gem(name: a["nameSpec"] ?? "", level: Int(a["level"] ?? "") ?? 1, quality: Int(a["quality"] ?? "") ?? 0,
                                enabled: a["enabled"] != "false", support: (a["gemId"] ?? "").contains("Support")))
            case "Config": build.activeConfigSet = a["activeConfigSet"] ?? "1"
            case "ConfigSet": setId = a["id"] ?? "1"; setTitle = a["title"] ?? "Default"; config = [:]
            case "Input", "Placeholder":
                if path.contains("ConfigSet"), let name = a["name"] {
                    config[name] = a["boolean"] ?? a["number"] ?? a["string"] ?? ""
                }
            case "Tree": build.activeSpec = Int(a["activeSpec"] ?? "") ?? 1
            case "Spec": specTitle = a["title"] ?? "Default"; specNodes = a["nodes"] ?? ""; specVersion = a["treeVersion"] ?? ""; specURL = ""
            case "URL": inURL = path.contains("Spec")
            default: break
            }
        }

        func parser(_ parser: XMLParser, foundCharacters s: String) {
            if path.last == "Item" { itemText += s }
            if inURL { specURL += s }
        }

        func parser(_ parser: XMLParser, didEndElement n: String, namespaceURI: String?, qualifiedName: String?) {
            switch n {
            case "Item": build.items[itemId] = itemText.trimmingCharacters(in: .whitespacesAndNewlines)
            case "ItemSet": build.itemSets.append(NamedSet(id: setId, title: setTitle, value: slots))
            case "Skill": groups.append(Group(slot: groupSlot, enabled: groupEnabled, gems: gems, main: groupMain))
            case "SkillSet": build.skillSets.append(NamedSet(id: setId, title: setTitle, value: groups))
            case "ConfigSet": build.configSets.append(NamedSet(id: setId, title: setTitle, value: config))
            case "Spec":
                let nodes = specNodes.split(separator: ",").compactMap { UInt64($0) }
                build.specs.append(Spec(title: specTitle, nodes: nodes, version: specVersion, url: specURL.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty))
            case "URL": inURL = false
            default: break
            }
            path.removeLast()
        }
    }
}

extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
