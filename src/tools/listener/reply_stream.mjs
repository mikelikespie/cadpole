// The reply to one message, posted while it is being written.
//
// The listener starts a message in the session with `stream_reply` when the
// model first does something (writes, or starts a tool call), appends the
// model's text to it as the text arrives, shows what a tool call is doing as
// a short activity line, and ends the message as done or failed. A turn that
// fails before the model did anything has no message; the listener says
// what went wrong in a line of its own (listener.mjs). This file is that bookkeeping and nothing else: the driver hands
// it text and activity through `sink`, and it decides when the server is
// called.
//
// How often: at most once per `flushMs` (250 ms), with everything that
// arrived since the last call in one piece, so a fast model costs the server
// four calls a second and the page four redraws. The server allows a little
// more and answers `throttled` beyond it; a throttled piece is kept and sent
// with the next one. While nothing arrives, an empty call every
// `keepAliveMs` tells the server the reply is still being written: a message
// that gets no call for 30 seconds is ended as failed by the server, which is
// what keeps a listener that died from leaving a reply in progress for ever.
//
// A server too old to have `stream_reply` gets the whole reply at the end,
// with `reply`.

/** What the listener says a Cadpole tool call is doing. Never the tool name. */
const ACTIVITY = {
  get_session: "reading the model…",
  get_selection: "looking at the selection…",
  explain_ref: "finding what made the selection…",
  api_doc: "reading the API reference…",
  get_attachment: "looking at the screenshot…",
  list_assets: "reading the model…",
  get_asset: "reading the model…",
  edit_script: "editing the script…",
  set_params: "setting parameters…",
  evaluate: "checking the model…",
  inspect: "checking the model…",
  print_check: "checking the model…",
  plate: "laying out the print plate…",
  run_tests: "running the tests…",
  render: "rendering…",
  sweep: "rendering…",
  animate: "rendering…",
  export: "exporting…",
  drawing: "drawing…",
};

/** Shown from the start of a turn until the model writes or calls a tool. */
export const THINKING = "thinking…";

/**
 * The activity line for a tool call, from the tool's name as the model's
 * session spells it (`mcp__cadpole__render`). A tool this table does not
 * know is "working…": the line is for a person, so it never carries a raw
 * tool name or its arguments.
 *
 * @param {string} tool
 */
export function activityFor(tool) {
  const name = String(tool ?? "").replace(/^mcp__cadpole__/, "");
  return Object.hasOwn(ACTIVITY, name) ? ACTIVITY[/** @type {keyof typeof ACTIVITY} */ (name)] : "working…";
}

/** Appended when a reply reaches the size the listener stops at. */
const CUT_NOTE = "(The reply was cut short: it reached the size limit for one message.)";

const bytes = (/** @type {string} */ s) => Buffer.byteLength(s, "utf8");

/** The longest prefix of `s` that fits in `max` bytes, on a character boundary. */
function fit(/** @type {string} */ s, /** @type {number} */ max) {
  let out = "";
  let used = 0;
  for (const ch of s) {
    const n = bytes(ch);
    if (used + n > max) break;
    out += ch;
    used += n;
  }
  return out;
}

export class ReplyStream {
  /**
   * @param {{
   *   cadpole: {callTool: (name: string, args: Record<string, unknown>) => Promise<any>},
   *   log?: (s: string) => void,
   *   flushMs?: number,
   *   keepAliveMs?: number,
   *   maxBytes?: number,
   * }} opts
   */
  constructor(opts) {
    this.cadpole = opts.cadpole;
    this.log = opts.log ?? (() => {});
    this.flushMs = opts.flushMs ?? 250;
    this.keepAliveMs = opts.keepAliveMs ?? 5000;
    // Under the server's cap on one message (64 KiB), with room for the
    // closing lines.
    this.maxBytes = opts.maxBytes ?? 60_000;
    /** The message being written; 0 until the server has given it a number. */
    this.seq = 0;
    /** False when the server has no `stream_reply`: the reply is posted whole. */
    this.streaming = true;
    /** Text not yet sent. */
    this.buffer = "";
    /** Everything the model wrote, for the log and for the whole-reply fallback. */
    this.text = "";
    this.sentBytes = 0;
    this.full = false;
    this.activity = "";
    this.sentActivity = "";
    this.lastCall = 0;
    this.ended = false;
    /** Whether the message has been asked for. */
    this.started = false;
    /** Calls are made one at a time, in order. */
    this.chain = Promise.resolve();
    /** @type {ReturnType<typeof setInterval> | null} */
    this.timer = null;
    /** What the driver writes to. */
    this.sink = {
      text: (/** @type {string} */ piece) => {
        this.write(piece);
        this.start();
      },
      activity: (/** @type {string} */ what) => {
        this.activity = String(what ?? "");
        this.start();
      },
    };
  }

  /** @param {string} piece */
  write(piece) {
    if (!piece || this.ended) return;
    this.text += piece;
    if (this.full) return;
    this.buffer += piece;
  }

  /**
   * Ask for the message, once; what has arrived goes with it. Never throws:
   * without `stream_reply` the reply is posted whole at the end.
   */
  start() {
    if (this.started || this.ended) return;
    this.started = true;
    void this.enqueue(async () => {
      try {
        await this.call(null);
      } catch (err) {
        this.streaming = false;
        this.log(`  replies will be posted whole: ${err instanceof Error ? err.message.split("\n")[0] : String(err)}`);
        return;
      }
      if (!this.ended) this.timer = setInterval(() => void this.flush(), this.flushMs);
    });
  }

  /**
   * Start the message now, with an activity line, and wait until the server
   * has it.
   *
   * @param {string} activity
   */
  async begin(activity = THINKING) {
    this.activity = activity;
    this.start();
    await this.chain;
  }

  /** Send what has arrived, if there is anything to say or it is time for a keep-alive. */
  flush() {
    return this.enqueue(async () => {
      if (this.ended || !this.streaming || !this.started) return;
      const idle = Date.now() - this.lastCall;
      if (!this.buffer && this.activity === this.sentActivity && idle < this.keepAliveMs) return;
      try {
        await this.call(null);
      } catch (err) {
        // The server ended the message (it was not fed in time, say). What
        // is still to come goes into a new one.
        this.log(`  the reply in progress was closed by the server: ${err instanceof Error ? err.message.split("\n")[0] : String(err)}`);
        this.seq = 0;
        this.sentBytes = 0;
        this.sentActivity = "";
      }
    });
  }

  /**
   * One `stream_reply` call carrying the buffered text.
   *
   * @param {"REPLY_FINISH_DONE" | "REPLY_FINISH_FAILED" | null} finish
   */
  async call(finish) {
    let append = this.buffer;
    if (this.sentBytes + bytes(append) > this.maxBytes) {
      append = fit(append, this.maxBytes - this.sentBytes);
      this.full = true;
    }
    this.buffer = "";
    const activity = finish ? "" : this.activity;
    /** @type {Record<string, unknown>} */
    const args = { seq: this.seq, append, activity };
    if (finish) args.finish = finish;
    let r;
    try {
      r = await this.cadpole.callTool("stream_reply", args);
    } catch (err) {
      this.buffer = append + this.buffer;
      throw err;
    }
    this.lastCall = Date.now();
    if (r?.throttled) {
      // Nothing was stored: the same text goes with the next call.
      this.buffer = append + this.buffer;
      return r;
    }
    this.seq = Number(r?.seq ?? this.seq);
    this.sentBytes += bytes(append);
    this.sentActivity = activity;
    return r;
  }

  /** @template T @param {() => Promise<T>} work @returns {Promise<T>} */
  enqueue(work) {
    const next = this.chain.then(work);
    this.chain = next.then(
      () => {},
      () => {},
    );
    return next;
  }

  /**
   * End the message. `failure`, when given, is the one line that says what
   * went wrong, on a line of its own after whatever was written. A message
   * that was never started (the model did nothing) is not posted at all,
   * and the result is false.
   *
   * @param {string} [failure]
   * @returns {Promise<boolean>} whether anything was posted
   */
  async end(failure) {
    if (!this.started) {
      this.ended = true;
      return false;
    }
    await this.chain;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    await this.enqueue(async () => {
      if (this.ended) return;
      this.ended = true;
      if (!this.full && this.sentBytes + bytes(this.buffer) > this.maxBytes) {
        this.buffer = fit(this.buffer, this.maxBytes - this.sentBytes);
        this.full = true;
      }
      /** @type {string[]} */
      const closing = [];
      if (this.full) closing.push(CUT_NOTE);
      if (failure) closing.push(failure);
      if (!this.streaming) {
        const whole = [fit(this.text, this.maxBytes).trim(), ...closing].filter(Boolean).join("\n\n");
        await this.cadpole.callTool("reply", { text: whole }).catch(() => {});
        return;
      }
      const finish = failure ? "REPLY_FINISH_FAILED" : "REPLY_FINISH_DONE";
      // The closing lines are not subject to the listener's own cut-off; the
      // server's cap has room for them.
      const tail = closing.length ? (this.sentBytes + bytes(this.buffer) > 0 ? "\n\n" : "") + closing.join("\n\n") : "";
      const reserve = this.maxBytes;
      this.maxBytes += bytes(tail) + 4;
      this.buffer += tail;
      try {
        await this.call(finish);
      } catch (err) {
        // The message is gone or the server is: say what is left in a new
        // message rather than nowhere.
        this.log(`  could not end the reply in progress: ${err instanceof Error ? err.message.split("\n")[0] : String(err)}`);
        const rest = this.buffer.trim();
        this.buffer = "";
        if (rest) await this.cadpole.callTool("reply", { text: rest }).catch(() => {});
      }
      this.maxBytes = reserve;
    });
    return true;
  }
}
