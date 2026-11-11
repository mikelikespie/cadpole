// The listener loop: long-poll a Cadpole session, decide which events deserve a
// turn, hand those to an agent, and record how far it got.
//
// Everything here is injectable (the MCP client and the driver are parameters,
// not imports), because this is the part that has to be right and the part a
// test can actually run. `main.mjs` is the wiring.

import { ReplyStream } from "./reply_stream.mjs";

/**
 * Events that are worth waking a model for.
 *
 * `message` from the user is a request. `selection_changed` is *only* a
 * request when it carries a note -- otherwise every click would start a turn,
 * which is both expensive and infuriating. Everything else (a slider moving, an
 * evaluation finishing, the script changing) is context the agent will read
 * when it next runs, not a reason to run.
 *
 * @param {any} ev
 */
export function isTurnWorthy(ev) {
  if (ev?.kind === "message") return ev?.data?.from === "user";
  if (ev?.kind === "selection_changed") return Boolean(ev?.data?.note);
  return false;
}

/**
 * The envelope an event is handed to the model in.
 *
 * It is XML-ish and explicitly labelled because the contents are not all the
 * same kind of thing: the `text` of a message is what the person asked for,
 * and everything else is a description of what they are looking at. The system
 * prompt says which is which; this tag is what makes the distinction available
 * to say.
 *
 * @param {any} ev
 */
export function promptFor(ev) {
  const kind = String(ev?.kind ?? "");
  const seq = Number(ev?.seq ?? 0);
  const role = kind === "message" ? "request-from-the-user" : "data-about-what-the-user-is-looking-at";
  return (
    `<cadpole_event kind="${kind}" seq="${seq}" content-is="${role}">\n` +
    `${JSON.stringify(ev?.data ?? {}, null, 2)}\n` +
    `</cadpole_event>`
  );
}

/**
 * The message that starts a new conversation instead of a turn: the model
 * forgets what was said so far. Handled here, so it costs no model turn.
 */
export const NEW_CONVERSATION = "/new";

export class Listener {
  /**
   * @param {{
   *   cadpole: {callTool: (name: string, args: Record<string, unknown>) => Promise<any>},
   *   driver: {ask: (prompt: string, sink: {text: (piece: string) => void, activity: (what: string) => void}) => Promise<string[]>,
   *            stop: () => Promise<void>, reset?: () => Promise<void>},
   *   log?: (s: string) => void,
   *   flushMs?: number,
   *   alwaysListen?: boolean,
   *   pollMs?: number,
   *   client?: string,
   * }} opts
   */
  constructor(opts) {
    this.cadpole = opts.cadpole;
    this.driver = opts.driver;
    this.log = opts.log ?? (() => {});
    this.alwaysListen = Boolean(opts.alwaysListen);
    this.pollMs = opts.pollMs ?? 30_000;
    this.client = opts.client ?? "";
    this.cursor = 0;
    this.listening = false;
    this.turns = 0;
    this.stopped = false;
    /** The last failure told to the user, so a failure that repeats is said once. */
    this.lastFailure = "";
    /** How often a reply in progress is sent on (reply_stream.mjs). */
    this.flushMs = opts.flushMs;
    /** Settles when the turn in flight has ended its reply; `stop` waits for it. */
    this.turn = Promise.resolve();
  }

  /**
   * Pick up where this client left off.
   *
   * `ack` with cursor 0 returns the cursor the *server* holds for this client
   * id, so a listener that was restarted resumes instead of either replaying
   * the whole log or silently dropping what happened while it was down.
   * A genuinely new client starts at the tail: the backlog
   * of a session that has been open all day is not a to-do list.
   */
  async resume() {
    const acked = await this.cadpole.callTool("ack", { client: this.client, cursor: 0 }).catch(() => null);
    const session = await this.cadpole.callTool("get_session", {});
    this.listening = Boolean(session?.assistant_listening);
    const resumed = Number(acked?.cursor ?? 0);
    this.cursor = resumed > 0 ? resumed : Number(session?.last_event_seq ?? 0);
    // A new client adopts the tail; record it at once, or a run that sees no
    // events leaves the server holding 0 and the next start jumps to the
    // tail again, past whatever was posted in between.
    if (resumed === 0 && this.cursor > 0) {
      await this.cadpole.callTool("ack", { client: this.client, cursor: this.cursor }).catch(() => {});
    }
    this.log(
      `resuming at seq ${this.cursor}; the UI's Listen toggle is ${this.listening ? "on" : "off"}` +
        (this.alwaysListen ? " (--always-listen overrides it)" : ""),
    );
    return this.cursor;
  }

  /**
   * One long poll and whatever it leads to. Returns the events it acted on,
   * which is what the tests assert about.
   */
  async step() {
    const res = await this.cadpole.callTool("wait_events", { after: this.cursor, timeout_ms: this.pollMs, client: this.client });
    const events = res?.events ?? [];
    const cursor = Number(res?.cursor ?? this.cursor);
    /** @type {any[]} */
    const acted = [];
    // `resume` read the toggle as it is now; the events in front of the first
    // change in this batch were sent under the toggle as it was, and the
    // change says which. Each event is judged by the toggle it was sent
    // under, whenever this process happened to attach.
    const change = events.find((/** @type {any} */ e) => e?.kind === "assistant_listening");
    if (typeof change?.data?.was === "boolean") this.listening = change.data.was;
    /** False when the listener was stopped before the batch was done. */
    let finished = true;
    for (const ev of events) {
      // Stopping: nothing new is started. What is left of the batch stays
      // ahead of the server's cursor, for the next run.
      if (this.stopped) {
        finished = false;
        break;
      }
      // The Listen toggle is the human's switch, and it is read from the event
      // stream rather than polled, so turning it off takes effect on the next
      // event rather than on the next minute.
      if (ev?.kind === "assistant_listening") {
        this.listening = Boolean(ev?.data?.listening);
        this.log(`the UI turned Listen ${this.listening ? "on" : "off"}`);
        continue;
      }
      if (!isTurnWorthy(ev)) continue;
      if (!this.listening && !this.alwaysListen) {
        this.log(`ignoring ${ev.kind} at seq ${ev.seq}: Listen is off`);
        continue;
      }
      if (ev.kind === "message" && String(ev?.data?.text ?? "").trim() === NEW_CONVERSATION) {
        this.log(`new conversation asked for at seq ${ev.seq}`);
        await this.driver.reset?.();
        await this.say("Started a new conversation: what was said before is forgotten; the document is unchanged.");
        acted.push(ev);
        await this.reached(ev.seq);
        continue;
      }
      this.turns += 1;
      this.log(`turn ${this.turns}: ${ev.kind} at seq ${ev.seq}`);
      this.turn = this.answer(ev);
      // Stopped in the middle of the turn: the event is left ahead of the
      // cursor and the next run asks again. Recording it as read would drop
      // it without a word.
      if (!(await this.turn)) {
        finished = false;
        break;
      }
      acted.push(ev);
      await this.reached(ev.seq);
    }
    if (finished) await this.reached(cursor);
    return acted;
  }

  /**
   * One turn. The reply is started in the session when the model first does
   * something, grows as the model writes, and ends as done, or as failed
   * with the one line that says why. A turn that fails before the model did
   * anything has no reply: the failure is said in a line of its own, once
   * per distinct failure in a row, so a listener that cannot run at all
   * does not answer every message with the same line. One bad turn must not
   * take the listener down: the user is in front of the UI and their next
   * message should still be answered.
   *
   * Returns false when the listener was stopped before the turn was done.
   *
   * @param {any} ev
   * @returns {Promise<boolean>}
   */
  async answer(ev) {
    const reply = new ReplyStream({ cadpole: this.cadpole, log: this.log, flushMs: this.flushMs });
    try {
      await this.driver.ask(promptFor(ev), reply.sink);
      if (!(await reply.end())) this.log("  the turn ended without the model writing or doing anything; nothing was posted");
      if (reply.text.trim()) this.log(`  said: ${reply.text.trim().replace(/\s+/g, " ").slice(0, 200)}`);
      this.lastFailure = "";
      return true;
    } catch (err) {
      const why = err instanceof Error ? err.message : String(err);
      this.log(`  turn failed: ${why}`);
      const line = `The listener could not finish this: ${why.split("\n")[0].slice(0, 400)}`;
      // A reply in progress is always ended, with what happened.
      if (await reply.end(this.stopped ? "The listener was stopped before it finished this." : line)) {
        this.lastFailure = this.stopped ? this.lastFailure : why;
      } else if (!this.stopped && why !== this.lastFailure) {
        // Stopped before the model did anything: the user has been told
        // nothing, and the next run asks again.
        this.lastFailure = why;
        await this.say(line);
      }
      return !this.stopped;
    }
  }

  /**
   * Record that everything up to `seq` has been dealt with. Server-side, so
   * the position survives this process; after every event acted on and not
   * once per batch, so a listener that dies during a turn repeats that turn
   * and not the ones before it.
   *
   * @param {number} seq
   */
  async reached(seq) {
    if (!(seq > this.cursor)) return;
    this.cursor = seq;
    await this.cadpole.callTool("ack", { client: this.client, cursor: this.cursor }).catch(() => {});
  }

  /**
   * A line from the listener itself in the message list. Best effort.
   *
   * @param {string} text
   */
  async say(text) {
    await this.cadpole.callTool("reply", { text }).catch(() => {});
  }

  /** Poll until stopped. */
  async run() {
    await this.resume();
    while (!this.stopped) {
      try {
        await this.step();
      } catch (err) {
        this.log(`poll failed, retrying: ${err instanceof Error ? err.message : String(err)}`);
        await new Promise((r) => setTimeout(r, 2000));
      }
    }
  }

  async stop() {
    this.stopped = true;
    await this.driver.stop();
    // The turn the driver just abandoned still has a reply to end.
    await this.turn;
  }
}
