// One login to a `cadpole serve`, from the pasted URL to the server's verdict
// on its token.
//
// The rules, in one place so they are tested on Linux:
//
//   * `verify` reads the session once before the app leaves the connect
//     screen. Only if the server ACCEPTS the token is it saved to the
//     Keychain and the server added to "Recent servers"; a mistyped token is
//     never persisted.
//   * A 401 -- at that first read, or later as the event stream's
//     `.unauthorized` state, which `watch` follows -- means the server
//     restarted and minted a new token. The stored token is deleted and the
//     app goes back to the connect screen with a message, rather than
//     retrying or leaving a badge that asks for something the screen does not
//     offer.

import Foundation

public struct ServerLogin: Sendable {
    public let address: ServerAddress
    public let bearer: String
    /// Where `bearer` came from.
    public let source: Source

    public enum Source: Sendable, Equatable {
        case pastedURL
        case typed
        case stored
    }

    public enum Problem: Error, Equatable, Sendable {
        case notAnAddress
        case noToken
        /// The server answered 401: the token is dead and has been forgotten.
        case rejected(host: String)
        /// Anything else that stopped the first session read.
        case unreachable(String)

        public var message: String {
            switch self {
            case .rejected(let host):
                return "\(host) did not accept this token; `cadpole serve` mints a new one each time it starts. Paste the URL it printed."
            case .unreachable(let why):
                return "Could not reach the server: \(why)"
            case .notAnAddress:
                return "That does not look like a server address."
            case .noToken:
                return "No token. Paste the whole URL `cadpole serve` printed, or type the token."
            }
        }
    }

    /// How a login ended.
    public enum Outcome: Sendable, Equatable {
        /// The server refused the token; it has been forgotten. `message` is
        /// what the connect screen shows.
        case rejected(message: String)
        /// The connection stream finished without a verdict.
        case ended
    }

    public init(address: ServerAddress, bearer: String, source: Source) {
        self.address = address
        self.bearer = bearer
        self.source = source
    }

    /// What the connect screen's two fields name. Saves nothing.
    public static func resolve(pasted: String, typed: String, store: any TokenStoring) -> Result<ServerLogin, Problem> {
        guard let address = ServerAddress.parse(pasted) else { return .failure(.notAnAddress) }
        let typed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        if let token = address.token, !token.isEmpty {
            return .success(ServerLogin(address: address, bearer: token, source: .pastedURL))
        }
        if !typed.isEmpty {
            return .success(ServerLogin(address: address, bearer: typed, source: .typed))
        }
        if let stored = store.load(host: address.host), !stored.isEmpty {
            return .success(ServerLogin(address: address, bearer: stored, source: .stored))
        }
        return .failure(.noToken)
    }

    /// Read the session once with this login's engine. Accepted: the token is
    /// saved and the server remembered. A 401: the stored token is deleted if
    /// it is the one that was rejected.
    /// Anything else saves nothing.
    public func verify(
        with engine: any EngineClient,
        store: any TokenStoring,
        recents: RecentServers
    ) async -> Result<Void, Problem> {
        do {
            _ = try await engine.session()
        } catch EngineError.http(status: 401, _) {
            forgetIfStored(store)
            return .failure(.rejected(host: address.host))
        } catch let error as EngineError {
            return .failure(.unreachable(String(describing: error)))
        } catch {
            return .failure(.unreachable(error.localizedDescription))
        }
        try? store.save(bearer, host: address.host)
        recents.remember(address)
        return .success(())
    }

    /// Follow the engine's connection state for as long as this login is in
    /// use. `.unauthorized` (the event stream got a 401) deletes the stored
    /// token and ends the login.
    public func watch<States: AsyncSequence>(
        _ states: States,
        store: any TokenStoring
    ) async -> Outcome where States.Element == ConnectionState {
        do {
            for try await state in states where state == .unauthorized {
                forgetIfStored(store)
                return .rejected(message: Problem.rejected(host: address.host).message)
            }
        } catch {}
        return .ended
    }

    /// A 401 says *this* token is dead. Delete the stored one only when it is
    /// the one that was rejected: a mistyped token, or another window's older
    /// one, must not cost the good token saved for the host.
    private func forgetIfStored(_ store: any TokenStoring) {
        if store.load(host: address.host) == bearer {
            store.delete(host: address.host)
        }
    }
}
