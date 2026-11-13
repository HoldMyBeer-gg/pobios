import Foundation

/// PoB's build-sharing sites, from Modules/BuildSiteTools.lua: which link
/// belongs to which site, and where its code is downloaded from. A
/// `pob://<siteId>/<code>` link maps through the same table, as it does on
/// the desktop.
enum BuildSites {
    struct Site {
        let id: String
        let label: String
        /// Captures the site's build id from a pasted link.
        let match: NSRegularExpression
        /// Download URL, with `$1` for the captured id. Always https.
        let download: String
    }

    static let all: [Site] = [
        Site(id: "maxroll", label: "Maxroll", match: rx(#"maxroll\.gg/poe/pob/(.+?)\s*$"#), download: "https://maxroll.gg/poe/api/pob/$1"),
        Site(id: "pobcodes", label: "pob.codes", match: rx(#"pob\.codes/b/(.+?)\s*$"#), download: "https://api.pob.codes/$1/raw"),
        Site(id: "pobbin", label: "pobb.in", match: rx(#"pobb\.in/(.+?)\s*$"#), download: "https://pobb.in/pob/$1"),
        Site(id: "poeninja", label: "poe.ninja", match: rx(#"poe\.ninja/(?:poe1/)?pob/(\w+)\s*$"#), download: "https://poe.ninja/poe1/pob/raw/$1"),
        Site(id: "pastebin", label: "Pastebin", match: rx(#"pastebin\.com/(\w+)\s*$"#), download: "https://pastebin.com/raw/$1"),
        Site(id: "pastebinproxy", label: "PastebinP", match: rx(#"pastebinp\.com/(\w+)\s*$"#), download: "https://pastebinp.com/raw/$1"),
        Site(id: "rentry", label: "Rentry", match: rx(#"rentry\.co/(\w+)\s*$"#), download: "https://rentry.co/paste/$1/raw"),
        Site(id: "poedb", label: "poedb.tw", match: rx(#"poedb\.tw/pob/(.+?)\s*$"#), download: "https://poedb.tw/pob/$1/raw"),
    ]

    private static func rx(_ p: String) -> NSRegularExpression {
        try! NSRegularExpression(pattern: p, options: [.caseInsensitive])
    }

    /// The site and download URL for a pasted link or a pob:// URI, or nil
    /// when the text is not a link to a known site.
    static func resolve(_ text: String) -> (site: Site, download: URL)? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        // pob://pobbin/<id>, as PoB's protocol handler takes it. Also
        // tolerates pob:/ and backslashes, as PoB's matcher does.
        if let m = try? NSRegularExpression(pattern: #"^pob:[/\\]*([A-Za-z]+)[/\\]+(.+)$"#, options: [.caseInsensitive]),
           let r = m.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)),
           let idR = Range(r.range(at: 1), in: t), let codeR = Range(r.range(at: 2), in: t) {
            let id = t[idR].lowercased()
            let code = String(t[codeR])
            if let site = all.first(where: { $0.id == id }), let url = URL(string: site.download.replacingOccurrences(of: "$1", with: code)) {
                return (site, url)
            }
            return nil
        }
        for site in all {
            if let r = site.match.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)), let idR = Range(r.range(at: 1), in: t) {
                let id = String(t[idR])
                if let url = URL(string: site.download.replacingOccurrences(of: "$1", with: id)) { return (site, url) }
            }
        }
        return nil
    }

    /// Download a build code (or raw XML) from a site. URLSession honours
    /// the system proxy, which is why there is no proxy setting here.
    static func fetch(_ url: URL) async throws -> String {
        var req = URLRequest(url: url)
        req.setValue("Path of Building (iPad)", forHTTPHeaderField: "User-Agent")
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw NSError(domain: "BuildSites", code: 1, userInfo: [NSLocalizedDescriptionKey: "The site returned an error for that link."])
        }
        // A build code is under a megabyte; a paste site can serve anything.
        if data.count > 4 * 1024 * 1024 {
            throw NSError(domain: "BuildSites", code: 3, userInfo: [NSLocalizedDescriptionKey: "That link is far too large to be a build."])
        }
        guard let text = String(data: data, encoding: .utf8), !text.isEmpty else {
            throw NSError(domain: "BuildSites", code: 2, userInfo: [NSLocalizedDescriptionKey: "The site sent nothing usable."])
        }
        return text
    }
}

/// Links opened from outside the app (Safari, another app) land here and
/// the root view picks them up.
@MainActor
final class LinkRouter: ObservableObject {
    static let shared = LinkRouter()

    /// A bump each time a link arrives; the view reacts to the change and
    /// then takes the URL with `consume()`, which hands it out exactly once.
    /// Cold launch delivers the same URL through both scene callbacks, so
    /// setting must be idempotent and reading must be single-shot.
    @Published private(set) var tick = 0
    private var url: URL?

    func deliver(_ url: URL) {
        self.url = url
        tick += 1
    }

    /// The pending URL, cleared so it is never returned twice.
    func consume() -> URL? {
        defer { url = nil }
        return url
    }
}
