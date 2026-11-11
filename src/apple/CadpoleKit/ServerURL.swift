// Getting connected (docs/apple-apps.md).
//
// The primary path is pasting the URL `cadpole serve` prints:
//
//     http://cadpole-host:7331/#token=<t>
//
// which carries host and token together, and whose fragment is never sent to a
// server or logged by a proxy (docs/security.md). The secondary path is typing
// a host name and pasting the token separately. Bonjour discovery is
// deliberately not offered: VPN peers (a Tailscale tailnet, say) do not
// advertise on the local link, and enumerating them would need credentials
// this app has no business holding.

import Foundation

public struct ServerAddress: Sendable, Equatable {
    /// Scheme, host and port only -- the path the API is mounted under.
    public var baseURL: URL
    /// The bearer token, when the pasted URL carried one.
    public var token: String?

    public init(baseURL: URL, token: String? = nil) {
        self.baseURL = baseURL
        self.token = token
    }

    /// What the Keychain item is keyed by, and what the window subtitle shows.
    /// Not a URL: it has no scheme and omits ports 80 and 443, so never parse
    /// it back -- `RecentServers` keeps the whole `baseURL` for that.
    public var host: String {
        var name = baseURL.host ?? baseURL.absoluteString
        // An IPv6 literal keeps its brackets, or "fd7a::1:7331" is ambiguous.
        if name.contains(":"), !name.hasPrefix("[") { name = "[\(name)]" }
        if let port = baseURL.port, port != 80, port != 443 {
            name += ":\(port)"
        }
        return name
    }

    /// Parse what the user pasted.
    ///
    /// Accepts the full printed URL, a bare host ("cadpole-host"), a host and port
    /// ("cadpole-host:7331") and anything with an explicit scheme. Defaults to
    /// `http` and port 7331, which is what `cadpole serve` binds; an explicit
    /// `https://` without a port is a TLS front on 443, not `cadpole serve`
    /// itself, so it keeps the scheme's own port.
    public static func parse(_ text: String) -> ServerAddress? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        let withScheme = trimmed.contains("://") ? trimmed : "http://" + trimmed
        guard var components = URLComponents(string: withScheme), let host = components.host, !host.isEmpty else {
            return nil
        }

        // The token rides in the fragment so it never reaches the server.
        var token: String?
        if let fragment = components.fragment {
            token = Self.bearerToken(inFragment: fragment)
        }
        // A `?token=` query is what the browser's EventSource needs; accept it
        // on paste, but never send it -- RemoteEngine uses the header.
        if token == nil, let queryToken = components.queryItems?.first(where: { $0.name == "token" })?.value {
            token = queryToken
        }

        components.fragment = nil
        components.query = nil
        components.path = ""
        components.user = nil
        components.password = nil
        if components.port == nil, components.scheme?.lowercased() != "https" { components.port = 7331 }

        guard let baseURL = components.url else { return nil }
        return ServerAddress(baseURL: baseURL, token: token?.isEmpty == true ? nil : token)
    }

    /// The fragment is `key=value&key=value`, like a query string.
    public static func bearerToken(inFragment fragment: String) -> String? {
        for pair in fragment.split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2, parts[0] == "token" else { continue }
            let raw = String(parts[1])
            return raw.removingPercentEncoding ?? raw
        }
        return nil
    }
}

/// The "Recent servers" list on the connect screen: newest first, at most
/// `limit`, stored newline-separated under `key` (the same key ConnectView's
/// `@AppStorage` reads). A server joins it only once it has accepted a token.
public struct RecentServers: @unchecked Sendable {
    public static let key = "cadpole.recentServers"
    public static let limit = 8

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public var entries: [String] {
        Self.entries(raw: defaults.string(forKey: Self.key) ?? "")
    }

    public static func entries(raw: String) -> [String] {
        raw.split(separator: "\n").map(String.init)
    }

    /// What an entry is filed under for `address`: the whole base URL, so the
    /// Recent button re-parses it into the same server (scheme, port and an
    /// IPv6 literal's brackets included). Entries saved as a bare host by
    /// older builds still parse, as the http://host:7331 they meant.
    public static func entry(for address: ServerAddress) -> String {
        address.baseURL.absoluteString
    }

    public func remember(_ address: ServerAddress) {
        let entry = Self.entry(for: address)
        var list = entries.filter { $0 != entry }
        list.insert(entry, at: 0)
        defaults.set(list.prefix(Self.limit).joined(separator: "\n"), forKey: Self.key)
    }
}
