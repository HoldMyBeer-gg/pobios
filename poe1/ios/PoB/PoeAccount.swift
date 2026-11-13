import AuthenticationServices
import CryptoKit
import Foundation
import Network
import Security

/// Signing in to pathofexile.com and reading characters, the way desktop
/// PoB does it: OAuth with PKCE against GGG's public `pob` client, which
/// redirects to http://localhost on one of three ports PoB registered. The
/// app listens on that port itself, so the sign-in page lands back here.
///
/// The Steam app is the reason this is not a plain Safari tab: given the
/// chance, iOS hands Steam's sign-in link to the app, which cannot finish a
/// pathofexile.com login. An authentication session keeps it in the page,
/// and shares Safari's cookies, so a login already done there carries over.
@MainActor
final class PoeAccount: NSObject, ObservableObject {
    static let shared = PoeAccount()

    struct Character: Identifiable, Hashable {
        let name: String
        let league: String
        let `class`: String
        let level: Int
        var id: String { name }
    }

    @Published private(set) var signedIn = false
    @Published private(set) var busy = false
    @Published var error: String?
    @Published private(set) var characters: [Character] = []

    private static let scopes = ["account:profile", "account:leagues", "account:characters", "account:trade"]
    private static let ports: [UInt16] = [49082, 49083, 49084]
    private static let userAgent = "Path of Building (iPad)"

    private var session: ASWebAuthenticationSession?
    private var listener: NWListener?

    override init() {
        super.init()
        signedIn = Keychain.read("poe.access") != nil
    }

    // MARK: Sign in

    func signIn() {
        // Tear down any previous attempt so a hung one recovers instead of
        // leaving the button on "Signing in..." with a dead listener.
        session?.cancel(); session = nil
        listener?.cancel(); listener = nil
        error = nil
        busy = true
        let verifier = Self.random(32)
        let challenge = Data(SHA256.hash(data: Data(verifier.utf8))).base64URL()
        let state = Self.random(12)
        guard let (listener, port) = Self.listen() else {
            busy = false
            error = "Could not open the sign-in callback port."
            return
        }
        self.listener = listener
        let scope = Self.scopes.joined(separator: "%20")
        // redirect_uri names the port the listener actually got, as PoB's
        // LaunchServer appends it once bound.
        let redirect = "http%3A%2F%2Flocalhost%3A\(port)"
        let url = URL(string: "https://www.pathofexile.com/oauth/authorize?client_id=pob&response_type=code&scope=\(scope)&state=\(state)&code_challenge=\(challenge)&code_challenge_method=S256&redirect_uri=\(redirect)")!

        listener.newConnectionHandler = { [weak self] conn in
            conn.start(queue: .main)
            // The request can arrive in pieces; answer once the request line
            // is complete, or the browser sits on a half-read connection.
            var buffer = Data()
            func readMore() {
                conn.receive(minimumIncompleteLength: 1, maximumLength: 16384) { data, _, done, _ in
                    if let data { buffer.append(data) }
                    let text = String(data: buffer, encoding: .utf8) ?? ""
                    if !text.contains("\r\n") && !done && buffer.count < 65536 { readMore(); return }
                    respond(text)
                }
            }
            func respond(_ request: String) {
                let query = Self.query(ofRequestLine: request)
                // Only a request carrying our state is the sign-in. Anything
                // else on this port (another app, a web page poking
                // localhost) gets a 400 and the listener stays up, so it
                // can neither end the sign-in nor choose what the page says.
                guard query["state"] == state else {
                    let body = "Bad request."
                    let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
                    conn.send(content: Data(response.utf8), completion: .contentProcessed { _ in conn.cancel() })
                    return
                }
                let ok = query["code"] != nil
                // Nothing from the request is echoed into the page.
                let body = ok
                    ? "<html><body style='font-family:-apple-system;padding:2em'><h2>Signed in.</h2><p>You can return to Path of Building.</p></body></html>"
                    : "<html><body style='font-family:-apple-system;padding:2em'><h2>Sign-in failed.</h2><p>Path of Exile did not authorise the app. You can close this and try again.</p></body></html>"
                let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
                conn.send(content: Data(response.utf8), completion: .contentProcessed { _ in
                    conn.cancel()
                    Task { @MainActor in
                        listener.cancel()
                        self?.listener = nil
                        self?.session?.cancel()
                        self?.session = nil
                        if ok, let code = query["code"] {
                            await self?.exchange(code: code, verifier: verifier, port: port)
                        } else {
                            self?.busy = false
                            // GGG's wording is not shown: it is a query string
                            // anyone could have written.
                            self?.error = query["error"] == "access_denied" ? "You declined the authorisation." : "Path of Exile did not complete the sign-in."
                        }
                    }
                })
            }
            readMore()
        }

        // A callback scheme is required by the API, but the real callback is
        // the localhost listener above; this one never fires.
        let session = ASWebAuthenticationSession(url: url, callbackURLScheme: "pob") { [weak self] _, err in
            Task { @MainActor in
                guard let self, self.busy, self.listener != nil else { return }
                self.listener?.cancel(); self.listener = nil
                self.busy = false
                if let err, (err as? ASWebAuthenticationSessionError)?.code == .canceledLogin {
                    self.error = nil
                } else {
                    self.error = err?.localizedDescription
                }
            }
        }
        session.prefersEphemeralWebBrowserSession = false
        session.presentationContextProvider = self
        self.session = session
        // Open the page only once the port is actually listening; a
        // redirect to a port that is not yet bound spins in the browser.
        var started = false
        listener.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                if !started { started = true; session.start() }
            case .failed(let e):
                Task { @MainActor in
                    self?.listener = nil; self?.session = nil; self?.busy = false
                    self?.error = "Could not listen for the sign-in: \(e.localizedDescription)"
                }
            default: break
            }
        }
        listener.start(queue: .main)
    }

    func signOut() {
        Keychain.delete("poe.access"); Keychain.delete("poe.refresh"); Keychain.delete("poe.expiry")
        signedIn = false
        characters = []
    }

    private func exchange(code: String, verifier: String, port: UInt16) async {
        await token(form: [
            ("client_id", "pob"), ("grant_type", "authorization_code"), ("code", code),
            ("redirect_uri", "http://localhost:\(port)"), ("scope", Self.scopes.joined(separator: " ")),
            ("code_verifier", verifier),
        ])
    }

    private func refresh() async -> Bool {
        guard let refresh = Keychain.read("poe.refresh") else { return false }
        await token(form: [("client_id", "pob"), ("grant_type", "refresh_token"), ("refresh_token", refresh)])
        return signedIn && error == nil
    }

    private func token(form: [(String, String)]) async {
        var req = URLRequest(url: URL(string: "https://www.pathofexile.com/oauth/token")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.setValue(Self.userAgent, forHTTPHeaderField: "User-Agent")
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-._~")
        req.httpBody = Data(form.map { "\($0.0)=\($0.1.addingPercentEncoding(withAllowedCharacters: allowed) ?? $0.1)" }.joined(separator: "&").utf8)
        do {
            let (data, resp) = try await URLSession.shared.data(for: req)
            guard let http = resp as? HTTPURLResponse, http.statusCode == 200,
                  let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let access = json["access_token"] as? String else {
                error = "Sign-in failed (\((resp as? HTTPURLResponse)?.statusCode ?? 0)). Try again."
                busy = false
                return
            }
            Keychain.write("poe.access", access)
            if let r = json["refresh_token"] as? String { Keychain.write("poe.refresh", r) }
            let expires = (json["expires_in"] as? Double) ?? 3600
            Keychain.write("poe.expiry", String(Date().timeIntervalSince1970 + expires))
            signedIn = true
            error = nil
            busy = false
        } catch {
            self.error = error.localizedDescription
            busy = false
        }
    }

    // MARK: API

    /// The account's characters on a realm ("pc", "sony", "xbox").
    func loadCharacters(realm: String) async {
        busy = true; error = nil
        defer { busy = false }
        guard let data = await get("/character" + (realm == "pc" ? "" : "/\(realm)")) else { return }
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let list = json["characters"] as? [[String: Any]] else {
            error = "Unexpected reply from the character list."
            return
        }
        characters = list.compactMap { c in
            guard let name = c["name"] as? String else { return nil }
            return Character(name: name, league: c["league"] as? String ?? "", class: c["class"] as? String ?? "", level: c["level"] as? Int ?? 0)
        }
    }

    /// The full character, tree and items, as the API's JSON text.
    func loadCharacter(realm: String, name: String) async -> String? {
        busy = true; error = nil
        defer { busy = false }
        let encoded = name.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? name
        guard let data = await get("/character" + (realm == "pc" ? "" : "/\(realm)") + "/" + encoded) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private func get(_ path: String, retried: Bool = false) async -> Data? {
        guard let access = Keychain.read("poe.access") else { error = "Not signed in."; return nil }
        if let expiry = Keychain.read("poe.expiry").flatMap(Double.init), Date().timeIntervalSince1970 > expiry - 60, !retried {
            if await refresh() { return await get(path, retried: true) }
            signOut(); error = "Your sign-in expired. Sign in again."; return nil
        }
        var req = URLRequest(url: URL(string: "https://api.pathofexile.com" + path)!)
        req.setValue("Bearer \(access)", forHTTPHeaderField: "Authorization")
        req.setValue(Self.userAgent, forHTTPHeaderField: "User-Agent")
        do {
            let (data, resp) = try await URLSession.shared.data(for: req)
            let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
            switch status {
            case 200: return data
            case 401 where !retried:
                if await refresh() { return await get(path, retried: true) }
                signOut(); error = "Your sign-in expired. Sign in again."; return nil
            case 429:
                let wait = (resp as? HTTPURLResponse)?.value(forHTTPHeaderField: "Retry-After") ?? "a moment"
                error = "The API is rate-limiting; try again in \(wait)s."
                return nil
            default:
                error = "The API answered \(status)."
                return nil
            }
        } catch {
            self.error = error.localizedDescription
            return nil
        }
    }

    // MARK: Helpers

    /// A listener bound to a loopback port, NOT yet started: the caller
    /// attaches its state handler first, then starts it, so `.ready` is
    /// never missed in the gap between creation and the handler.
    private static func listen() -> (NWListener, UInt16)? {
        for port in ports {
            let params = NWParameters.tcp
            params.requiredInterfaceType = .loopback
            if let l = try? NWListener(using: params, on: NWEndpoint.Port(rawValue: port)!) {
                return (l, port)
            }
        }
        return nil
    }

    private static func random(_ bytes: Int) -> String {
        var b = [UInt8](repeating: 0, count: bytes)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes, &b)
        return Data(b).base64URL()
    }

    /// Query parameters of an HTTP request's first line, "GET /?code=…&state=… HTTP/1.1".
    nonisolated private static func query(ofRequestLine request: String) -> [String: String] {
        guard let line = request.split(separator: "\r\n", omittingEmptySubsequences: true).first,
              let path = line.split(separator: " ").dropFirst().first,
              let q = path.split(separator: "?", maxSplits: 1).dropFirst().first else { return [:] }
        var out: [String: String] = [:]
        for pair in q.split(separator: "&") {
            let kv = pair.split(separator: "=", maxSplits: 1)
            guard let k = kv.first else { continue }
            out[String(k)] = String(kv.dropFirst().first ?? "").removingPercentEncoding ?? ""
        }
        return out
    }
}

extension PoeAccount: ASWebAuthenticationPresentationContextProviding {
    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated {
            UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.keyWindow }.first ?? ASPresentationAnchor()
        }
    }
}

private extension Data {
    func base64URL() -> String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}

/// Tokens live in the Keychain, not UserDefaults.
enum Keychain {
    private static func query(_ key: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "com.pobios.app", kSecAttrAccount as String: key]
    }
    static func read(_ key: String) -> String? {
        var q = query(key)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: AnyObject?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
    static func write(_ key: String, _ value: String) {
        delete(key)
        var q = query(key)
        q[kSecValueData as String] = Data(value.utf8)
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(q as CFDictionary, nil)
    }
    static func delete(_ key: String) { SecItemDelete(query(key) as CFDictionary) }
}
