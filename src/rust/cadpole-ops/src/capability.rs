//! Which operations a *restricted* token may call.
//!
//! The limited-privilege listener (docs/assistant-integration.md) is an
//! agent with a small tool surface. "Small" has to mean something an agent
//! cannot argue its way out of, so the boundary is not the prompt, not the
//! tool list the client happens to show, and not a flag the listener passes
//! itself: it is the *server*, which knows from the bearer token which role
//! is calling and refuses a denied operation with `403` before any handler
//! runs.
//!
//! This module is the one statement of that policy, so the client-side tool
//! filter in `cadpole mcp --listener` and the server-side refusal cannot drift.
//! Every operation in the registry is either granted or denied *by name*, and
//! `every_operation_is_classified` below fails the build when a new rpc is
//! added to `ops.proto` without a decision being made about it. That is the
//! point: a new operation is denied to the listener until someone writes down
//! why it is safe.

/// Who is calling, as decided by which token was presented.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// The user, the web UI, `cadpole op`, an interactive `cadpole mcp`. Everything.
    Owner,
    /// The limited-privilege listener: the operations in [`GRANTED`] and
    /// nothing else, over the op registry only.
    Listener,
}

impl Role {
    /// The word used on the command line, in the token file name and in the
    /// server's banner.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Listener => "listener",
        }
    }

    /// May a caller in this role invoke `op`? `op` is the rpc name
    /// (`EditScript`), not a CLI or MCP spelling.
    pub fn may(self, op: &str) -> bool {
        match self {
            Role::Owner => true,
            Role::Listener => GRANTED.iter().any(|(n, _)| *n == op),
        }
    }
}

/// The operations the listener may call, each with the reason it is safe to
/// hand to an agent that may have been talked into anything.
///
/// The common thread: every one of these is either read-only, or a write whose
/// blast radius is one revision of one script that the user can see and undo.
pub const GRANTED: &[(&str, &str)] = &[
    (
        "GetSession",
        "read-only; the document, script text and revision are what the listener is for, and the revision is what EditScript needs as its base",
    ),
    ("GetSelection", "read-only; resolving what the user is pointing at is the whole job"),
    ("ExplainRef", "read-only; derived from the document it may already read"),
    ("WaitEvents", "read-only long poll; the loop's only clock. It returns events and changes nothing"),
    (
        "Ack",
        "writes one cursor keyed by this client's own id; the worst it can do is make the listener replay or skip its own events",
    ),
    (
        "GetAttachment",
        "read-only; returns a screenshot the user deliberately attached to a message. This is 'see what the user sees'",
    ),
    ("ApiDoc", "read-only; static documentation of the Starlark API"),
    (
        "Evaluate",
        "a pure function of a document, inside the Starlark sandbox and under the server's evaluation deadline; it writes nothing",
    ),
    (
        "Render",
        "pure; bounded by the render semaphore, the per-request budget and the resolution/sample clamps, which apply to every token holder alike",
    ),
    ("Sweep", "pure; the same bounds as Render plus the sweep-cell clamp"),
    (
        "Animate",
        "pure: one evaluation per frame of a document the listener may already Render, each inside the same sandbox and deadline, one render per frame under Render's clamps and the frame cap, the bytes returned to the caller. It takes no path and writes no file; the front-end writes what comes back",
    ),
    ("PrintCheck", "read-only analysis over the tessellated bodies"),
    (
        "RunTests",
        "pure: evaluates a document the listener may already Evaluate, once per sweep point, each inside the same sandbox and deadline and capped at 256 points per test; it writes nothing and returns names, verdicts and messages",
    ),
    (
        "Inspect",
        "read-only, the same shape as PrintCheck: it evaluates a document the listener may already read, measures the tessellated bodies (counts, the solid validator, folds, manifold's verdict) and returns the evaluation's diagnostics. It writes nothing and takes no path",
    ),
    (
        "Export",
        "read-only; mesh bytes derived from a document it may already read, returned to the caller. It takes no path and writes no file",
    ),
    (
        "Drawing",
        "read-only, and the same shape as Export: a dimensioned drawing derived from a document the listener may already read, returned as bytes to the caller. It names no path and writes no file",
    ),
    ("ListAssets", "read-only metadata: asset names, sizes and kinds, never bytes"),
    ("GetAsset", "read-only; the bytes of an asset already in the document"),
    (
        "SetParams",
        "a write, but only an override map: no code, last-writer-wins, one revision, visible in the UI and trivially undone",
    ),
    (
        "EditScript",
        "the write the listener exists for, and the safest shape of it: each edit names what it replaces -- exact text, a line range or a feature's arguments -- in a file that already exists, the edit is refused unless `base_revision` still matches, the result is parsed before it is stored, the script size cap applies, and every edit is a revision the user can see and revert",
    ),
    (
        "Reply",
        "appends one message attributed to the assistant, capped at the message size limit. The server sets `from`, so a listener cannot post as the user",
    ),
    (
        "StreamReply",
        "the same effect as Reply, a piece at a time: text attributed to the assistant by the server, never to the user, appended to one message that only the client that started it may continue and that nothing changes once it has ended. The total is capped at the message size limit, calls are rate limited, a client has one message in progress at a time, and the server ends a message its author stopped feeding. The pieces are not session events, so they wake no other assistant; the one event is the finished message, as with Reply",
    ),
    (
        "ListJobs",
        "read-only: the ids, states, timings, limits and origins (a feature id and a line) of this machine's solve jobs, and the solution cache's size. It starts, stops and changes nothing",
    ),
    (
        "WaitJobs",
        "read-only long poll on solve jobs' states, capped at 120 s like WaitEvents; it starts, stops and changes nothing",
    ),
];

/// The operations the listener may not call, each with the reason.
pub const DENIED: &[(&str, &str)] = &[
    (
        "SetScript",
        "replaces a whole file, and can create new ones. EditScript does everything the listener needs while making it say what each edit replaces in a file that exists; a whole-file write lets one confused call silently discard work that still matches the revision it read. A listener asked to can still rewrite an existing file through EditScript, a line range at a time, as a revision the user sees and can revert; what it cannot do is create a file",
    ),
    (
        "SetSelection",
        "moving the user's selection is spoofing the UI: the human's next 'make this thicker' would then refer to something the agent chose. The listener names what it means in a Reply instead",
    ),
    (
        "AddAsset",
        "writes caller-chosen bytes into the document, up to the asset caps, under a name scripts later resolve with asset(\"name\"). Nothing the listener does needs it, and it is the one place it could park chosen content for a later script to read",
    ),
    (
        "ReportClientFailure",
        "reports a crash, so what it writes to the event log reads as the *system* speaking: a message, a stack and the operation that failed, carrying none of the attribution that makes `Reply` safe. `Reply` is a listener write that reaches the very same log, and it is granted precisely because the server stamps `from` on it; a `client_failure` has no author, so a listener that could forge one could put a fabricated engine panic in front of the user and every attached assistant. It also buys the listener nothing: it is not a front end, it has no wasm engine and no page, and the failures it could have are its own process's, not this session's. The clients that do need it -- the web UI, an Apple app -- hold the owner token",
    ),
    (
        "PostMessage",
        "posts as the *user*. An agent that can put words in the user's mouth can agree with itself, invent a request it was never given, and leave a transcript that says the human asked for it. The listener speaks with Reply, where the server sets `from`",
    ),
    (
        "Save",
        "writes the session's document to the file the server was started with. The listener's writes are supposed to stay in the session, where the user sees them in the UI and reverts them with one revision; committing them to disk is the user's decision and the user's button",
    ),
    (
        "Solve",
        "starts solve jobs: `cadpole` processes that run past every request's deadline -- up to an hour each by default, several at once -- on a document the caller may supply. Evaluate is granted because it is a pure function under the deadline, and the listener's evaluations read cached answers and attach to running jobs but never start one; this is the one door to that effect, and spending the machine's time on long solves is the owner's call",
    ),
    (
        "CancelJobs",
        "stops, and with `forget` erases, solve jobs the owner started: an hour of solving undone by one call. Nothing the listener does needs it, since it starts no jobs of its own",
    ),
];

/// True when the limited listener may call `op` (an rpc name).
pub fn listener_may(op: &str) -> bool {
    Role::Listener.may(op)
}

/// Why `op` is refused to the listener, or `None` if it is granted or unknown.
pub fn denied_reason(op: &str) -> Option<&'static str> {
    DENIED.iter().find(|(n, _)| *n == op).map(|(_, why)| *why)
}

/// The granted rpc names, in policy order. What `cadpole mcp --listener` shows.
pub fn granted_names() -> Vec<&'static str> {
    GRANTED.iter().map(|(n, _)| *n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guarantee this module exists for: adding an rpc to `ops.proto`
    /// without classifying it fails here, and what a forgotten operation
    /// becomes is *denied*, never granted.
    #[test]
    fn every_operation_is_classified() {
        let registry: Vec<&str> = crate::registry().iter().map(|m| m.name.as_str()).collect();
        for op in &registry {
            let granted = GRANTED.iter().any(|(n, _)| n == op);
            let denied = DENIED.iter().any(|(n, _)| n == op);
            assert!(
                granted != denied,
                "{op} is {} in capability.rs: every operation must be in exactly one of GRANTED and DENIED",
                if granted { "in both lists" } else { "in neither list" }
            );
        }
        for (op, _) in GRANTED.iter().chain(DENIED.iter()) {
            assert!(registry.contains(op), "{op} is classified but is not an rpc of CadpoleOps any more");
        }
        assert_eq!(GRANTED.len() + DENIED.len(), registry.len());
    }

    #[test]
    fn every_reason_is_written_down() {
        for (op, why) in GRANTED.iter().chain(DENIED.iter()) {
            assert!(why.len() > 40, "{op}: the reason is the point, write one ({why:?})");
        }
    }

    #[test]
    fn the_owner_may_do_everything_and_the_listener_may_not() {
        for m in crate::registry() {
            assert!(Role::Owner.may(&m.name), "the owner role is not a restriction");
        }
        assert!(listener_may("EditScript"));
        assert!(listener_may("Reply"));
        assert!(listener_may("StreamReply"));
        assert!(listener_may("Render"));
        assert!(!listener_may("SetScript"));
        assert!(!listener_may("SetSelection"));
        assert!(!listener_may("AddAsset"));
        assert!(!listener_may("ReportClientFailure"));
        // An operation nobody has heard of is not granted by accident.
        assert!(!listener_may("DeleteEverything"));
        assert!(denied_reason("AddAsset").is_some());
        assert!(denied_reason("Reply").is_none());
    }
}
