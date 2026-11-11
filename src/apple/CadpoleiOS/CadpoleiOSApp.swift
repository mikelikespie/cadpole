// Cadpole on iOS: a thin client.
//
// No kernel, no Starlark, no local evaluation -- the document lives on a
// server, addressed by UUID, and every screen is the same `DocumentScreen` the
// Mac shows. The viewport shows the server's rendered image.

import CadpoleKit
import CadpoleUI
import SwiftUI

@main
struct CadpoleiOSApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }
}

struct RootView: View {
    @State private var connected: (engine: RemoteEngine, login: ServerLogin)?
    /// Why the app came back to the connect screen (a rejected token).
    @State private var problem: String?

    var body: some View {
        if let connected {
            DocumentScreen(engine: connected.engine)
                .modifier(ServerLoginWatch(engine: connected.engine, login: connected.login) { message in
                    problem = message
                    self.connected = nil
                })
        } else {
            NavigationStack {
                ConnectView(problem: problem) { engine, login in
                    problem = nil
                    connected = (engine, login)
                }
            }
        }
    }
}
