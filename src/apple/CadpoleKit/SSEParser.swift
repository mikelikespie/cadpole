// Server-sent events, framed by hand.
//
// `cadpole serve`'s `/api/events` emits `id:`, `event:` and `data:` lines; the
// payload of each `data:` is the whole `Event` object, so `kind` arrives twice
// and `seq` is authoritative. Parsing is a pure value type so the Linux test
// can feed it split byte runs -- the case that breaks naive readers is a frame
// boundary landing inside "data:" or between \r and \n.

import Foundation

public struct SSEFrame: Sendable, Equatable {
    public var id: String?
    public var event: String?
    public var data: String

    public init(id: String? = nil, event: String? = nil, data: String) {
        self.id = id
        self.event = event
        self.data = data
    }
}

/// Incremental SSE framer. Feed it bytes in whatever chunks arrive.
public struct SSEParser: Sendable {
    private var pending: [UInt8] = []
    private var id: String?
    private var event: String?
    private var data: [String] = []

    public init() {}

    /// Consume bytes and return every frame they completed.
    public mutating func push(_ bytes: some Sequence<UInt8>) -> [SSEFrame] {
        var frames: [SSEFrame] = []
        for byte in bytes {
            if byte == UInt8(ascii: "\n") {
                // A CRLF stream leaves the \r at the end of the line.
                if pending.last == UInt8(ascii: "\r") { pending.removeLast() }
                let line = String(decoding: pending, as: UTF8.self)
                pending.removeAll(keepingCapacity: true)
                if let frame = consume(line: line) { frames.append(frame) }
            } else {
                pending.append(byte)
            }
        }
        return frames
    }

    /// One line of the stream. Returns a frame when the line closed one.
    public mutating func consume(line: String) -> SSEFrame? {
        if line.isEmpty {
            // Blank line dispatches. A frame with no data at all (a keep-alive
            // comment run) dispatches nothing.
            defer {
                id = nil
                event = nil
                data.removeAll(keepingCapacity: true)
            }
            guard !data.isEmpty else { return nil }
            return SSEFrame(id: id, event: event, data: data.joined(separator: "\n"))
        }
        // ":" starts a comment; the keep-alive is exactly that.
        if line.hasPrefix(":") { return nil }

        let field: String
        var value: String
        if let colon = line.firstIndex(of: ":") {
            field = String(line[line.startIndex ..< colon])
            value = String(line[line.index(after: colon)...])
            // One optional leading space after the colon is part of the framing.
            if value.hasPrefix(" ") { value.removeFirst() }
        } else {
            field = line
            value = ""
        }

        switch field {
        case "id": id = value
        case "event": event = value
        case "data": data.append(value)
        default: break  // "retry" and anything unknown.
        }
        return nil
    }
}

extension EngineEvent {
    /// Decode one `data:` payload. The server sends the whole `Event` object in
    /// it, so `kind` and `seq` come from the JSON rather than from the SSE
    /// framing -- and `seq` is what the reconnect cursor and the restart
    /// check are built on.
    public static func decode(sseData: String) throws -> EngineEvent {
        guard let bytes = sseData.data(using: .utf8) else {
            throw EngineError.decoding("event data was not UTF-8")
        }
        return try decode(json: bytes)
    }

    public static func decode(json: Data) throws -> EngineEvent {
        struct Wire: Decodable {
            var seq: UInt64?
            var at: String?
            var kind: String
            var data: JSONValue?
            var by: String?
        }
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: json)
        } catch {
            throw EngineError.decoding("event: \(error)")
        }
        var payload = Data()
        if let inner = wire.data, inner != .null {
            payload = (try? JSONEncoder().encode(inner)) ?? Data()
        }
        return EngineEvent(
            seq: wire.seq ?? 0,
            at: wire.at ?? "",
            kind: wire.kind,
            dataJSON: payload,
            by: wire.by ?? ""
        )
    }
}
