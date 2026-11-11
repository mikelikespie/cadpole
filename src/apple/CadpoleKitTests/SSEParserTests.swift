// SSE framing, the part that breaks when a TCP segment lands mid-line.

import Foundation
import CadpoleKit
import XCTest

final class SSEParserTests: XCTestCase {
    private func frames(of text: String, chunkedBy size: Int) -> [SSEFrame] {
        var parser = SSEParser()
        var out: [SSEFrame] = []
        let bytes = Array(text.utf8)
        var i = 0
        while i < bytes.count {
            let end = min(i + size, bytes.count)
            out += parser.push(bytes[i ..< end])
            i = end
        }
        return out
    }

    func testFramesSurviveEveryChunkBoundary() {
        let text = """
        id: 1
        event: message
        data: {"seq":1}

        id: 2
        event: message
        data: {"seq":2}


        """
        let whole = frames(of: text, chunkedBy: text.utf8.count)
        XCTAssertEqual(whole.count, 2)
        XCTAssertEqual(whole[0].id, "1")
        XCTAssertEqual(whole[0].event, "message")
        XCTAssertEqual(whole[0].data, #"{"seq":1}"#)

        // One byte at a time is the pathological case: every field name, every
        // colon and every newline arrives in its own chunk.
        for size in [1, 2, 3, 7, 13] {
            XCTAssertEqual(frames(of: text, chunkedBy: size), whole, "chunk size \(size)")
        }
    }

    func testCRLFAndKeepAliveComments() {
        let text = ":\r\n:ping\r\nid: 5\r\ndata: hello\r\n\r\n"
        let out = frames(of: text, chunkedBy: 4)
        XCTAssertEqual(out.count, 1)
        XCTAssertEqual(out[0].id, "5")
        XCTAssertEqual(out[0].data, "hello")
    }

    func testMultiLineDataIsJoinedWithNewlines() {
        let out = frames(of: "data: one\ndata: two\n\n", chunkedBy: 5)
        XCTAssertEqual(out.map(\.data), ["one\ntwo"])
    }

    func testFieldWithNoColonAndNoLeadingSpace() {
        let out = frames(of: "data\ndata:x\n\n", chunkedBy: 100)
        // A bare "data" contributes an empty line; "data:x" has no space to eat.
        XCTAssertEqual(out.map(\.data), ["\nx"])
    }

    func testEventDecodeReadsSeqKindAndPayload() throws {
        let event = try EngineEvent.decode(
            sseData: #"{"seq":42,"at":"2026-09-14T00:00:00Z","kind":"params_changed","data":{"overrides":{"w":5}}}"#
        )
        XCTAssertEqual(event.seq, 42)
        XCTAssertEqual(event.kind, "params_changed")
        XCTAssertEqual(event.at, "2026-09-14T00:00:00Z")

        struct Payload: Decodable { var overrides: [String: JSONValue] }
        let payload = try JSONDecoder().decode(Payload.self, from: event.dataJSON)
        XCTAssertEqual(payload.overrides["w"], .number(5))
    }

    func testEventDecodeRejectsNonEvents() {
        XCTAssertThrowsError(try EngineEvent.decode(sseData: "not json"))
        XCTAssertThrowsError(try EngineEvent.decode(sseData: #"{"seq":1}"#), "no kind")
    }
}
