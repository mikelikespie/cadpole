// Connecting to a `cadpole serve` (docs/apple-apps.md).
//
// Paste the URL the server printed; the token rides in the fragment and is
// never put back in a URL. It reaches the Keychain, and the server the
// "Recent servers" list, only once the server accepts it, and a 401 forgets
// it again: the app holding the engine runs `ServerLogin.watch`
// (CadpoleKit/ServerLogin.swift) and comes back here with its message.
// Bonjour discovery is deliberately absent.

import CadpoleKit
import SwiftUI

public struct ConnectView: View {
    /// Handed a ready-to-use engine once a server has been named.
    /// The login rides along so the caller can watch the engine's connection
    /// with it (`ServerLogin.watch`): that is what keeps or forgets the token.
    private let connect: (RemoteEngine, ServerLogin) -> Void

    @State private var pasted = ""
    @State private var token = ""
    @State private var problem: String?
    @State private var connecting = false
    @AppStorage(RecentServers.key) private var recentsRaw = ""

    /// - Parameter problem: why the app came back here, e.g. the server
    ///   refused the token it had.
    public init(problem: String? = nil, connect: @escaping (RemoteEngine, ServerLogin) -> Void) {
        self.connect = connect
        _problem = State(initialValue: problem)
    }

    public var body: some View {
        Form {
            Section {
                TextField("http://cadpole-host:7331/#token=\u{2026}", text: $pasted)
                    .textFieldStyle(.roundedBorder)
                    .font(.body.monospaced())
                    #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    #endif
                TextField("Token (only if the URL had none)", text: $token)
                    .textFieldStyle(.roundedBorder)
                    #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    #endif
            } header: {
                Text("Server")
            } footer: {
                Text("`cadpole serve` prints one URL with the token in its fragment. Fragments are never sent to a server.")
                    .font(.footnote)
            }

            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
            }

            Button("Connect") { attempt() }
                .buttonStyle(.borderedProminent)
                .disabled(connecting || pasted.trimmingCharacters(in: .whitespaces).isEmpty)
            if connecting {
                ProgressView("Connecting\u{2026}")
            }

            if !recents.isEmpty {
                Section("Recent servers") {
                    ForEach(recents, id: \.self) { host in
                        Button(host) {
                            pasted = host
                            attempt()
                        }
                        .disabled(connecting)
                    }
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("Connect to Cadpole")
    }

    private var recents: [String] {
        RecentServers.entries(raw: recentsRaw)
    }

    private func attempt() {
        problem = nil
        let login: ServerLogin
        switch ServerLogin.resolve(pasted: pasted, typed: token, store: KeychainTokens()) {
        case .success(let l): login = l
        case .failure(let p):
            problem = p.message
            return
        }

        let engine = RemoteEngine(
            baseURL: login.address.baseURL,
            token: login.bearer,
            transport: URLSessionTransport(session: URLSessionTransport.callSession()),
            eventTransport: URLSessionTransport(session: URLSessionTransport.eventStreamSession())
        )
        // One session read before leaving this screen: it is what decides
        // whether the token is kept (and the server remembered) or forgotten.
        connecting = true
        Task { @MainActor in
            let verdict = await login.verify(with: engine, store: KeychainTokens(), recents: RecentServers())
            connecting = false
            switch verdict {
            case .success:
                connect(engine, login)
            case .failure(let p):
                problem = p.message
            }
        }
    }
}

/// What the apps put around a connected `DocumentScreen`: follows the
/// engine's connection with `ServerLogin.watch` and, when the server rejects
/// the token (it restarted and minted a new one), calls `rejected` with the
/// message the connect screen should show. The token is already forgotten by
/// then.
public struct ServerLoginWatch: ViewModifier {
    let engine: RemoteEngine
    let login: ServerLogin
    let rejected: (String) -> Void

    public init(engine: RemoteEngine, login: ServerLogin, rejected: @escaping (String) -> Void) {
        self.engine = engine
        self.login = login
        self.rejected = rejected
    }

    public func body(content: Content) -> some View {
        content.task(id: ObjectIdentifier(engine)) {
            if case .rejected(let message) = await login.watch(engine.connection, store: KeychainTokens()) {
                rejected(message)
            }
        }
    }
}
