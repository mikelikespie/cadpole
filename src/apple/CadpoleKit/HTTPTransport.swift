// The one seam that keeps this module testable on Linux.
//
// `RemoteEngine` talks to an `HTTPTransport`, never to URLSession directly, so
// the whole client -- URL building, the bearer header, JSON and protobuf
// decoding, SSE framing, reconnection -- is exercised by a `swift_test` on
// Linux with no server and no Apple SDK. `URLSessionTransport` below is the
// only Apple-only code in CadpoleKit and is the only thing the Simulator test adds
// coverage for.

import Foundation

#if canImport(FoundationNetworking)
    import FoundationNetworking
#endif

public struct HTTPRequest: Sendable, Equatable {
    public var method: String
    public var url: URL
    public var headers: [String: String]
    public var body: Data?

    public init(method: String, url: URL, headers: [String: String] = [:], body: Data? = nil) {
        self.method = method
        self.url = url
        self.headers = headers
        self.body = body
    }
}

public struct HTTPResponse: Sendable, Equatable {
    public var status: Int
    public var headers: [String: String]
    public var body: Data

    public init(status: Int, headers: [String: String] = [:], body: Data = Data()) {
        self.status = status
        self.headers = headers
        self.body = body
    }
}

public protocol HTTPTransport: Sendable {
    /// One request, one response, fully buffered.
    func send(_ request: HTTPRequest) async throws -> HTTPResponse

    /// A live byte stream, for `text/event-stream`. Throws
    /// `EngineError.http` on a non-2xx status before yielding anything.
    ///
    /// Cadpole's Apple clients put the token in `Authorization: Bearer` here too.
    /// `docs/security.md`'s `?token=` exception on `/api/events` exists only
    /// because the browser's `EventSource` cannot set headers.
    func stream(_ request: HTTPRequest) async throws -> AsyncThrowingStream<UInt8, any Error>
}

/// How long the real transport waits. Here, outside `#if canImport(Darwin)`,
/// so the Linux tests can hold them against the server's budget.
public enum HTTPTimeouts {
    /// One `/api/call` (evaluate, render). The server bounds the work itself
    /// -- a `--render-budget-ms` budget per job (60 s by default, 504 past
    /// it) behind a queue of 16 (429 beyond) -- but the budget starts only
    /// once a slot is free, so a legal answer can take the whole queue's
    /// budgets plus ours, and the 504 tells the user to RAISE the budget.
    /// URLSession.shared's 60 s gave up first, so the explanatory 504 never
    /// arrived. This is a backstop against a dead connection, not a budget.
    public static let call: TimeInterval = 60 * 60
    /// The event stream's idle timeout: longer than the server's SSE
    /// keep-alive interval.
    public static let eventStreamIdle: TimeInterval = 120
    /// The session JSON (`GET`/`PUT /api/session*`). Those routes never wait
    /// behind the render queue, so a host that accepts the connection and
    /// never answers is given up on in half a minute, not an hour.
    public static let session: TimeInterval = 30

    /// The request timeout for a path: only `/api/call` waits on the render
    /// queue and gets `call`.
    public static func forPath(_ path: String) -> TimeInterval {
        if path.hasSuffix("/api/call") { return call }
        if path.hasSuffix("/api/events") { return eventStreamIdle }
        return session
    }
}

#if canImport(Darwin)

    /// The real transport. Apple-only on purpose: swift-corelibs-foundation has
    /// no `URLSession.bytes(for:)`, and a Linux fallback would be a second
    /// implementation of the thing the tests are trying to pin down.
    public final class URLSessionTransport: HTTPTransport {
        private let session: URLSession

        public init(session: URLSession = URLSessionTransport.callSession()) {
            self.session = session
        }

        /// A session for `/api/call` and the session JSON: request and
        /// resource timeouts of `HTTPTimeouts.call`.
        public static func callSession() -> URLSession {
            let config = URLSessionConfiguration.default
            config.timeoutIntervalForRequest = HTTPTimeouts.call
            config.timeoutIntervalForResource = HTTPTimeouts.call
            return URLSession(configuration: config)
        }

        /// A session configured for a long-lived event stream: no resource
        /// timeout, and a request timeout longer than the server's SSE
        /// keep-alive interval.
        public static func eventStreamSession() -> URLSession {
            let config = URLSessionConfiguration.default
            config.timeoutIntervalForRequest = HTTPTimeouts.eventStreamIdle
            config.timeoutIntervalForResource = .infinity
            config.waitsForConnectivity = true
            return URLSession(configuration: config)
        }

        private func urlRequest(_ request: HTTPRequest) -> URLRequest {
            var r = URLRequest(url: request.url)
            r.timeoutInterval = HTTPTimeouts.forPath(request.url.path)
            r.httpMethod = request.method
            r.httpBody = request.body
            for (k, v) in request.headers {
                r.setValue(v, forHTTPHeaderField: k)
            }
            return r
        }

        public func send(_ request: HTTPRequest) async throws -> HTTPResponse {
            let (data, response) = try await session.data(for: urlRequest(request))
            guard let http = response as? HTTPURLResponse else {
                throw EngineError.decoding("not an HTTP response")
            }
            var headers: [String: String] = [:]
            for (k, v) in http.allHeaderFields {
                if let k = k as? String, let v = v as? String { headers[k] = v }
            }
            return HTTPResponse(status: http.statusCode, headers: headers, body: data)
        }

        public func stream(_ request: HTTPRequest) async throws -> AsyncThrowingStream<UInt8, any Error> {
            let (bytes, response) = try await session.bytes(for: urlRequest(request))
            guard let http = response as? HTTPURLResponse else {
                throw EngineError.decoding("not an HTTP response")
            }
            guard (200 ..< 300).contains(http.statusCode) else {
                throw EngineError.http(status: http.statusCode, body: "")
            }
            return AsyncThrowingStream { continuation in
                let task = Task {
                    do {
                        for try await byte in bytes {
                            continuation.yield(byte)
                        }
                        continuation.finish()
                    } catch {
                        continuation.finish(throwing: error)
                    }
                }
                continuation.onTermination = { _ in task.cancel() }
            }
        }
    }

#endif
