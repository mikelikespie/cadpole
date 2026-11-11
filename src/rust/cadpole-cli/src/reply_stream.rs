//! Messages posted a piece at a time: the rules behind `StreamReply`.
//!
//! A streamed message is an ordinary entry of the session's message list
//! whose `state` is "streaming" until it ends as "done" or "failed". This
//! module is every rule about how such a message may change, kept apart from
//! the HTTP handler so that the same code is driven by the server, by the
//! unit tests and by the traces of `spec/reply_stream.qnt`
//! (`the_model_traces_replay_against_the_handler` in `server.rs`):
//!
//! - the server attributes it to the assistant, whoever calls;
//! - text is only ever appended, and only by the client that started the
//!   message (the token's role and the `X-Cadpole-Client` id);
//! - once a message is done or failed nothing changes it;
//! - what the caller appends is capped in total, the activity line is capped
//!   and one line, and calls past a burst are answered `throttled` and change
//!   nothing;
//! - a client has one message in progress, and a session a handful;
//! - a message that is not fed for [`Limits::idle`], or is still open after
//!   [`Limits::life`], is ended as failed by [`ReplyStreams::expire`], so
//!   none stays in progress for ever.
//!
//! Nothing here touches the event log. The caller turns each message this
//! module reports as ended into one `message` event; the appends in between
//! are not events at all (see `App::live` in `server.rs`).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cadpole_ops::capability::Role;

use crate::server::ChatMessage;

/// `SessionMessage.state` of a message still being written.
pub const STREAMING: &str = "streaming";
/// `SessionMessage.state` of a streamed message that ended normally.
pub const DONE: &str = "done";
/// `SessionMessage.state` of a streamed message that ended early.
pub const FAILED: &str = "failed";

/// The line the server adds when it ends a message itself.
pub const ABANDONED_NOTE: &str = "The assistant stopped before finishing this reply.";

/// The numbers behind the rules. [`Limits::default`] is what `cadpole serve`
/// runs with; the model traces are replayed under small ones.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Most bytes of text the caller may append to one message in total.
    pub max_text: usize,
    /// Most bytes of the activity line.
    pub max_activity: usize,
    /// Calls accepted back to back for one message before the rate limit
    /// bites.
    pub burst: u32,
    /// How often one more call is earned back.
    pub refill: Duration,
    /// A message with no accepted call for this long is ended as failed.
    pub idle: Duration,
    /// A message still in progress this long after it started is ended as
    /// failed.
    pub life: Duration,
    /// Most messages in progress in one session.
    pub max_open: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_text: crate::security::MAX_MESSAGE_BYTES,
            max_activity: 120,
            // Five a second in the steady state: the page redraws a growing
            // message a few times a second, and no faster.
            burst: 8,
            refill: Duration::from_millis(200),
            idle: Duration::from_secs(30),
            life: Duration::from_secs(30 * 60),
            max_open: 4,
        }
    }
}

/// Who is calling: the token's role and the client id of the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub role: Role,
    pub client: String,
}

/// How a call asks to leave the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    Done,
    Failed,
}

/// One `StreamReply` call.
#[derive(Clone, Debug, Default)]
pub struct Update<'a> {
    /// 0 starts a message.
    pub seq: u64,
    pub append: &'a str,
    pub activity: &'a str,
    pub finish: Option<Finish>,
}

/// Why a call was refused. A refused call changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No message with that `seq` is being written: it never was, or it has
    /// ended.
    NotInProgress(u64),
    /// The message is being written by another client.
    NotTheAuthor(u64),
    /// The text would pass the cap.
    TooLong { have: usize, adding: usize, max: usize },
    /// The activity line is over its cap or is not one line of text.
    BadActivity { max: usize },
    /// The session already has as many messages in progress as it allows.
    TooManyInProgress(usize),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotInProgress(seq) => {
                write!(f, "message {seq} is not in progress: it has ended or was never started with StreamReply, and a finished message does not change")
            }
            Refusal::NotTheAuthor(seq) => write!(f, "message {seq} is being written by another client; only the client that started it may continue it"),
            Refusal::TooLong { have, adding, max } => {
                write!(f, "the message has {have} bytes and this call adds {adding}, over the {max} byte limit")
            }
            Refusal::BadActivity { max } => write!(f, "`activity` is one line of at most {max} bytes, without control characters"),
            Refusal::TooManyInProgress(n) => write!(f, "{n} messages are already in progress in this session, which is the limit; finish one first"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What an accepted call did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The message the call was about.
    pub seq: u64,
    /// Its state after the call.
    pub state: &'static str,
    /// The size of its text after the call, in bytes.
    pub length: usize,
    /// The call came too soon and changed nothing.
    pub throttled: bool,
    /// Where the appended text starts, when the message is still in
    /// progress and the call changed it: what a page showing the message
    /// needs to follow along.
    pub offset: Option<usize>,
    /// Messages this call ended, oldest first: the caller's earlier message
    /// when this call started a new one, and the message itself when the call
    /// finished it. Each is owed one `message` event.
    pub ended: Vec<u64>,
}

struct Open {
    author: Author,
    started: Instant,
    touched: Instant,
    tokens: u32,
    refilled: Instant,
    /// Bytes the caller has appended; the server's own closing line is not
    /// counted against the cap.
    appended: usize,
}

/// The messages in progress in one session.
pub struct ReplyStreams {
    limits: Limits,
    open: BTreeMap<u64, Open>,
}

impl Default for ReplyStreams {
    fn default() -> Self {
        ReplyStreams::with_limits(Limits::default())
    }
}

impl ReplyStreams {
    pub fn with_limits(limits: Limits) -> ReplyStreams {
        ReplyStreams { limits, open: BTreeMap::new() }
    }

    /// How many messages are in progress.
    pub fn in_progress(&self) -> usize {
        self.open.len()
    }

    /// One call. `at` is the timestamp a new message is given.
    pub fn apply(&mut self, messages: &mut Vec<ChatMessage>, author: &Author, req: &Update<'_>, now: Instant, at: &str) -> Result<Applied, Refusal> {
        // A message that fell out of the list's window is no longer there to
        // write to.
        self.open.retain(|seq, _| messages.iter().any(|m| m.seq == *seq));
        if req.seq == 0 { self.start(messages, author, req, now, at) } else { self.resume(messages, author, req, now) }
    }

    fn check_activity(&self, activity: &str) -> Result<(), Refusal> {
        if activity.len() > self.limits.max_activity || activity.chars().any(char::is_control) {
            return Err(Refusal::BadActivity { max: self.limits.max_activity });
        }
        Ok(())
    }

    fn start(&mut self, messages: &mut Vec<ChatMessage>, author: &Author, req: &Update<'_>, now: Instant, at: &str) -> Result<Applied, Refusal> {
        self.check_activity(req.activity)?;
        if req.append.len() > self.limits.max_text {
            return Err(Refusal::TooLong { have: 0, adding: req.append.len(), max: self.limits.max_text });
        }
        let earlier: Option<u64> = self.open.iter().find(|(_, o)| &o.author == author).map(|(seq, _)| *seq);
        let others = self.open.len() - usize::from(earlier.is_some());
        if others >= self.limits.max_open {
            return Err(Refusal::TooManyInProgress(others));
        }
        let mut ended = Vec::new();
        // One message in progress per client: starting the next one is how a
        // client that lost track of its last one says so.
        if let Some(seq) = earlier {
            self.end(messages, seq, FAILED, Some(ABANDONED_NOTE));
            ended.push(seq);
        }
        let seq = messages.last().map(|m| m.seq).unwrap_or(0) + 1;
        messages.push(ChatMessage {
            seq,
            // Never the caller's choice.
            from: "assistant".to_owned(),
            text: req.append.to_owned(),
            at: at.to_owned(),
            attachments: Vec::new(),
            state: STREAMING.to_owned(),
            activity: req.activity.to_owned(),
        });
        self.open.insert(
            seq,
            Open { author: author.clone(), started: now, touched: now, tokens: self.limits.burst, refilled: now, appended: req.append.len() },
        );
        let mut applied = Applied { seq, state: STREAMING, length: req.append.len(), throttled: false, offset: Some(0), ended };
        if let Some(finish) = req.finish {
            applied.state = self.end(messages, seq, finish_state(finish), None);
            applied.offset = None;
            applied.ended.push(seq);
        }
        Ok(applied)
    }

    fn resume(&mut self, messages: &mut [ChatMessage], author: &Author, req: &Update<'_>, now: Instant) -> Result<Applied, Refusal> {
        let limits = self.limits.clone();
        let Some(open) = self.open.get_mut(&req.seq) else {
            return Err(Refusal::NotInProgress(req.seq));
        };
        if &open.author != author {
            return Err(Refusal::NotTheAuthor(req.seq));
        }
        if req.activity.len() > limits.max_activity || req.activity.chars().any(char::is_control) {
            return Err(Refusal::BadActivity { max: limits.max_activity });
        }
        if open.appended + req.append.len() > limits.max_text {
            return Err(Refusal::TooLong { have: open.appended, adding: req.append.len(), max: limits.max_text });
        }
        let Some(msg) = messages.iter_mut().find(|m| m.seq == req.seq) else {
            return Err(Refusal::NotInProgress(req.seq));
        };
        // The call that ends a message is never throttled: a message can
        // always be closed, and it can be closed only once.
        if req.finish.is_none() {
            let earned = (now.saturating_duration_since(open.refilled).as_millis() / limits.refill.as_millis().max(1)) as u32;
            if earned > 0 {
                open.tokens = open.tokens.saturating_add(earned).min(limits.burst);
                open.refilled += limits.refill * earned;
            }
            if open.tokens == 0 {
                return Ok(Applied { seq: req.seq, state: STREAMING, length: msg.text.len(), throttled: true, offset: None, ended: Vec::new() });
            }
            open.tokens -= 1;
        }
        let offset = msg.text.len();
        msg.text.push_str(req.append);
        msg.activity = req.activity.to_owned();
        open.appended += req.append.len();
        open.touched = now;
        let length = msg.text.len();
        match req.finish {
            None => Ok(Applied { seq: req.seq, state: STREAMING, length, throttled: false, offset: Some(offset), ended: Vec::new() }),
            Some(finish) => {
                let state = self.end(messages, req.seq, finish_state(finish), None);
                Ok(Applied { seq: req.seq, state, length, throttled: false, offset: None, ended: vec![req.seq] })
            }
        }
    }

    /// End every message whose author stopped feeding it, or that has been
    /// open too long. Returns the messages ended, each owed one `message`
    /// event.
    pub fn expire(&mut self, messages: &mut [ChatMessage], now: Instant) -> Vec<u64> {
        let (idle, life) = (self.limits.idle, self.limits.life);
        let due: Vec<u64> = self
            .open
            .iter()
            .filter(|(_, o)| now.saturating_duration_since(o.touched) >= idle || now.saturating_duration_since(o.started) >= life)
            .map(|(seq, _)| *seq)
            .collect();
        for seq in &due {
            self.end(messages, *seq, FAILED, Some(ABANDONED_NOTE));
        }
        due
    }

    /// The one way a message leaves "streaming". `note` is the server's own
    /// closing line, on a line of its own.
    fn end(&mut self, messages: &mut [ChatMessage], seq: u64, state: &'static str, note: Option<&str>) -> &'static str {
        self.open.remove(&seq);
        if let Some(msg) = messages.iter_mut().find(|m| m.seq == seq) {
            msg.state = state.to_owned();
            msg.activity.clear();
            if let Some(note) = note {
                if !msg.text.is_empty() {
                    msg.text.push_str("\n\n");
                }
                msg.text.push_str(note);
            }
        }
        state
    }
}

fn finish_state(finish: Finish) -> &'static str {
    match finish {
        Finish::Done => DONE,
        Finish::Failed => FAILED,
    }
}
