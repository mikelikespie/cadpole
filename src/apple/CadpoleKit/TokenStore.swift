// Where the bearer token lives (docs/apple-apps.md).
//
// A generic-password Keychain item, service `KeychainTokenStore.service`,
// account = host. Accessible after first unlock, THIS DEVICE ONLY and not iCloud-synced:
// a `cadpole serve` token is a per-machine secret and syncing it would spread a
// credential the other device cannot use anyway.
//
// A 401 means the server rotated its token. Delete the item and ask for a new
// URL rather than retrying -- retrying a dead token just makes noise in the
// server's log. `ServerLogin` (ServerLogin.swift) is what does both, and what
// saves a token only once the server has accepted it.

import Foundation

#if canImport(Security)
    import Security
#endif

public enum KeychainTokenStore {
    public static let service = "io.github.mikelikespie.cadpole.token"

    public enum StoreError: Error, Equatable {
        case unavailable
        case status(Int32)
    }

    #if canImport(Security)

        public static func save(_ token: String, host: String) throws {
            let data = Data(token.utf8)
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: host,
            ]
            let attributes: [String: Any] = [
                kSecValueData as String: data,
                kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            ]
            let updated = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
            if updated == errSecSuccess { return }
            guard updated == errSecItemNotFound else { throw StoreError.status(updated) }

            var insert = query
            insert.merge(attributes) { _, new in new }
            let added = SecItemAdd(insert as CFDictionary, nil)
            guard added == errSecSuccess else { throw StoreError.status(added) }
        }

        public static func load(host: String) -> String? {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: host,
                kSecReturnData as String: true,
                kSecMatchLimit as String: kSecMatchLimitOne,
            ]
            var item: CFTypeRef?
            guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
                  let data = item as? Data
            else { return nil }
            return String(data: data, encoding: .utf8)
        }

        /// Called on a 401: the token this host had is no longer valid.
        public static func delete(host: String) {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: host,
            ]
            _ = SecItemDelete(query as CFDictionary)
        }

    #else

        // Linux: there is no Keychain and no app to keep a secret for. The
        // module still compiles so //src/apple/CadpoleKitTests can exercise
        // everything around it.
        public static func save(_ token: String, host: String) throws {
            throw StoreError.unavailable
        }

        public static func load(host: String) -> String? { nil }

        public static func delete(host: String) {}

    #endif
}

/// Where a token is kept, as something `ServerLogin` can be handed: the
/// Keychain in the apps, a dictionary in the Linux tests.
public protocol TokenStoring: Sendable {
    func save(_ token: String, host: String) throws
    func load(host: String) -> String?
    func delete(host: String)
}

/// `KeychainTokenStore` as a `TokenStoring`.
public struct KeychainTokens: TokenStoring {
    public init() {}
    public func save(_ token: String, host: String) throws { try KeychainTokenStore.save(token, host: host) }
    public func load(host: String) -> String? { KeychainTokenStore.load(host: host) }
    public func delete(host: String) { KeychainTokenStore.delete(host: host) }
}
