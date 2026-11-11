// A reply that is still being written (`StreamReply`), as the page keeps it.
//
// The server tells a page about such a message in three ways, and they can
// arrive in any order:
//
//   * a `reply_stream` frame on the event stream each time the message
//     grows: where the new text starts (in bytes of UTF-8), the text, and
//     the activity line. Frames are not in the event log and are not
//     replayed, so one can be missed;
//   * a session read, which carries the message as far as it had got;
//   * one `message` event when it ends, with the whole text and the state
//     "done" or "failed".
//
// The functions here fold each of those into the message list. They are pure
// so that the rules can be tested without a page: text only ever grows, a
// message that has ended is not reopened by a late frame or a stale read,
// and a frame that does not continue what the page holds asks for a session
// read instead of guessing.

import type { Message } from "./types.js";

/** One `reply_stream` frame. */
export interface ReplyFrame {
  seq: number;
  at: string;
  /** Where `append` starts in the message's text, in bytes of UTF-8. */
  offset: number;
  append: string;
  activity: string;
}

const encoder = new TextEncoder();
const bytes = (s: string): number => encoder.encode(s).length;

const settled = (m: Message): boolean => m.state === "done" || m.state === "failed";

/**
 * Fold a frame into the list. `resync` is set when the frame cannot be
 * applied to what the page holds (a frame was missed): the caller reads the
 * session and the list is returned unchanged.
 */
export function applyFrame(messages: Message[], f: ReplyFrame): { messages: Message[]; resync: boolean } {
  const i = messages.findIndex((m) => m.seq === f.seq);
  if (i < 0) {
    if (f.offset !== 0) return { messages, resync: true };
    const fresh: Message = { seq: f.seq, from: "assistant", text: f.append, at: f.at, state: "streaming" };
    if (f.activity) fresh.activity = f.activity;
    return { messages: [...messages, fresh], resync: false };
  }
  const have = messages[i] as Message;
  // A frame that was overtaken by the end of its message, or one for a
  // message that was never streamed: nothing to do.
  if (have.state !== "streaming") return { messages, resync: false };
  const known = bytes(have.text);
  if (f.offset > known) return { messages, resync: true };
  // Already held (a session read got there first): only the activity is news.
  const text = f.offset === known ? have.text + f.append : have.text;
  if (text === have.text && (have.activity ?? "") === f.activity) return { messages, resync: false };
  const next: Message = { ...have, text };
  if (f.activity) next.activity = f.activity;
  else delete next.activity;
  const out = messages.slice();
  out[i] = next;
  return { messages: out, resync: false };
}

/**
 * Fold one message from a `message` event, or from the answer to a post,
 * into the list. A streamed message replaces its in-progress copy; a message
 * the page already holds is kept, with whichever copy has the pictures.
 */
export function upsertMessage(messages: Message[], m: Message): Message[] {
  const i = messages.findIndex((x) => x.seq === m.seq && (x.text === m.text || x.state !== undefined || m.state !== undefined));
  if (i < 0) return [...messages, m];
  const have = messages[i] as Message;
  let next: Message | null = null;
  if (m.state !== undefined || have.state !== undefined) {
    // An ended message is final; an in-progress copy never replaces it.
    if (settled(have) && !settled(m)) return messages;
    if (have.text !== m.text || have.state !== m.state || (have.activity ?? "") !== (m.activity ?? "")) next = m;
  } else if (m.attachments?.length && !have.attachments?.length) {
    // The same message arrives twice: once as the answer to the call that
    // posted it, once on the event stream, which never carries image bytes.
    next = m;
  }
  if (!next) return messages;
  const out = messages.slice();
  out[i] = next;
  return out;
}

/**
 * The message list of a session read, merged with what the page holds. The
 * read may be older than the frames and events already applied, so for a
 * streamed message the copy that is further along wins; local screenshot
 * payloads, which the server strips, are kept.
 */
export function mergeSnapshot(prev: Message[], incoming: Message[]): Message[] {
  return incoming.map((m) => {
    const old = prev.find((x) => x.seq === m.seq);
    if (!old) return m;
    if (old.state !== undefined && m.state !== undefined) {
      if (settled(old) && !settled(m)) return old;
      if (!settled(m) && old.text.length > m.text.length && old.text.startsWith(m.text)) return old;
    }
    return old.attachments && !m.attachments ? { ...m, attachments: old.attachments } : m;
  });
}
