// The /api/call request timeout covers the server's legal response time:
// queue wait plus the render budget (docs/security.md: a bounded queue of 16
// in front of `min(2, max(1, cores/4))` slots, 60 s per job by default).

import CadpoleKit
import XCTest

final class CallTimeoutTests: XCTestCase {
    func testACallOutlastsAFullRenderQueueAtTheDefaultBudget() {
        let budget: Double = 60
        let queued: Double = 16
        // One slot: every queued job runs its full budget first, then ours.
        let worst = (queued + 1) * budget
        XCTAssertGreaterThan(HTTPTimeouts.call, worst)
    }

    func testTheEventStreamKeepsItsOwnShorterIdleTimeout() {
        XCTAssertLessThan(HTTPTimeouts.eventStreamIdle, HTTPTimeouts.call)
    }

    /// Only `/api/call` waits behind the render queue. The session reads and
    /// writes do not, so a connect screen's first read, or a parameter write
    /// on a dropped path, gives up in half a minute rather than an hour.
    func testOnlyTheCallRouteGetsTheLongTimeout() {
        XCTAssertEqual(HTTPTimeouts.forPath("/api/call"), HTTPTimeouts.call)
        XCTAssertEqual(HTTPTimeouts.forPath("/api/events"), HTTPTimeouts.eventStreamIdle)
        for path in ["/api/session", "/api/session/params", "/api/session/selection", "/api/session/document"] {
            XCTAssertEqual(HTTPTimeouts.forPath(path), HTTPTimeouts.session, path)
        }
        XCTAssertLessThanOrEqual(HTTPTimeouts.session, 30)
    }
}
