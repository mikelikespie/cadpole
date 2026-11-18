import Foundation

/// Switching between the plates of a project whose models load in the
/// background: what to show at once, which loads still matter, and what to
/// do with each one that finishes.
///
/// - On a switch the old plate's model goes away at once. A plate already
///   loaded is shown straight away; any other shows its own picture (or the
///   empty bed) with a loading mark until its model is in, then fades in.
/// - Only the selected plate's load matters while it is loading: a switch
///   cancels every other load, so a slow plate left behind stops costing
///   time, and its model never replaces the one on screen even if it
///   finishes anyway.
/// - Once the selected plate is shown, its neighbours load one at a time
///   (`prefetch`), so stepping to them is instant.
///
/// A value: the view owns one on its main thread (or shares a
/// `PlateSwitch` with its loading queue).
public struct PlateLoads<ID: Hashable & Sendable>: Sendable {
    /// One load of one plate. A switch away cancels it; a new load of the
    /// same plate gets a new ticket.
    public struct Ticket: Hashable, Sendable {
        public let plate: ID
        public let serial: Int
    }

    /// What to show the moment a plate is selected.
    public enum Show: Equatable, Sendable {
        /// Its model is loaded: show it now, in place of whatever was shown.
        case model(ID)
        /// Not yet: take the old model away now, and show the plate's own
        /// picture (or the empty bed) with a loading mark.
        case loading(ID)
    }

    /// What to do with a load that finished.
    public enum Outcome: Equatable, Sendable {
        /// The selected plate's: fade it in now.
        case present
        /// A neighbour loaded ahead: keep it, show nothing.
        case keep
        /// Cancelled (its plate was left): throw it away; it never replaces
        /// what is on screen.
        case drop
    }

    public private(set) var selected: ID?
    /// The plates whose models are loaded and kept.
    public private(set) var ready: Set<ID> = []
    /// The loads that still matter, one per plate.
    public private(set) var live: [ID: Ticket] = [:]
    private var serial = 0

    public init() {}

    /// The person picked plate `id`: what to show now, and the load to start
    /// (nil when it is loaded or already loading).
    public mutating func select(_ id: ID) -> (show: Show, start: Ticket?) {
        selected = id
        live = live.filter { $0.key == id }
        if ready.contains(id) { return (.model(id), nil) }
        if live[id] != nil { return (.loading(id), nil) }
        let t = ticket(id)
        return (.loading(id), t)
    }

    /// Whether a load still matters. A loader asks before it starts and
    /// between its steps, and stops when it does not.
    public func wants(_ t: Ticket) -> Bool { live[t.plate] == t }

    /// A load finished with its plate's model.
    public mutating func finished(_ t: Ticket) -> Outcome {
        guard live[t.plate] == t else { return .drop }
        live[t.plate] = nil
        ready.insert(t.plate)
        return t.plate == selected ? .present : .keep
    }

    /// A load failed or stopped: forget it (selecting the plate again loads
    /// it again).
    public mutating func failed(_ t: Ticket) {
        if live[t.plate] == t { live[t.plate] = nil }
    }

    /// The next neighbour to load ahead: the first of `candidates` neither
    /// loaded nor loading. Nil until the selected plate is shown, and while
    /// any load is under way (one at a time).
    public mutating func prefetch(_ candidates: [ID]) -> Ticket? {
        guard let s = selected, ready.contains(s), live.isEmpty,
              let id = candidates.first(where: { !ready.contains($0) })
        else { return nil }
        return ticket(id)
    }

    /// Forget a loaded plate (its model was let go to save memory).
    public mutating func evict(_ id: ID) { ready.remove(id) }

    private mutating func ticket(_ id: ID) -> Ticket {
        serial += 1
        let t = Ticket(plate: id, serial: serial)
        live[id] = t
        return t
    }
}

/// `PlateLoads` behind a lock, for a view that starts loads on a background
/// queue: the queue asks `wants` while the main thread switches plates.
public final class PlateSwitch<ID: Hashable & Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var state = PlateLoads<ID>()

    public init() {}

    public var selected: ID? { lock.withLock { state.selected } }
    public func isReady(_ id: ID) -> Bool { lock.withLock { state.ready.contains(id) } }
    public func select(_ id: ID) -> (show: PlateLoads<ID>.Show, start: PlateLoads<ID>.Ticket?) {
        lock.withLock { state.select(id) }
    }
    public func wants(_ t: PlateLoads<ID>.Ticket) -> Bool { lock.withLock { state.wants(t) } }
    public func finished(_ t: PlateLoads<ID>.Ticket) -> PlateLoads<ID>.Outcome { lock.withLock { state.finished(t) } }
    public func failed(_ t: PlateLoads<ID>.Ticket) { lock.withLock { state.failed(t) } }
    public func prefetch(_ candidates: [ID]) -> PlateLoads<ID>.Ticket? { lock.withLock { state.prefetch(candidates) } }
    public func evict(_ id: ID) { lock.withLock { state.evict(id) } }
}
