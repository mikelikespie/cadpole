// The page's parameter overrides against the session's.
//
// The overrides are session state on the server, and the page is one of
// several writers: an assistant's `SetParams`, `cadpole op` and another tab
// write them too. While the user's own edit is on its way, a session read
// must not put the server's older values back under the slider -- that is
// what `dirty` is for. Once the edit has been answered -- or refused -- the
// server's word is the page's again.

import type { WriteOptions } from "./api.js";
import type { ParamValue, Session } from "./types.js";
import { overridesEqual } from "./util.js";

type Overrides = Record<string, ParamValue>;

export interface ParamSyncDeps {
  /** The overrides the page is showing and evaluating. */
  current(): Overrides;
  /** Write them to the session (replace semantics). */
  put(overrides: Overrides, opts: WriteOptions): Promise<Session>;
  /** Read the session again. */
  read(): Promise<Session>;
  /** Merge a session into the page. */
  apply(session: Session): void;
  /** A write that failed: the session no longer matches the screen. */
  report(err: unknown): void;
}

export class ParamSync {
  private isDirty = false;

  constructor(private readonly deps: ParamSyncDeps) {}

  /**
   * Whether the page holds an edit the session has not yet acknowledged, in
   * which case a session read must not overwrite the page's overrides.
   */
  get dirty(): boolean {
    return this.isDirty;
  }

  /** The user changed an override. */
  edited(): void {
    this.isDirty = true;
  }

  /** Send the page's overrides; resolves once the answer has been applied. */
  send(opts: WriteOptions = {}): Promise<void> {
    const sent = this.deps.current();
    return this.deps
      .put(sent, opts)
      .then((sess) => {
        if (overridesEqual(this.deps.current(), sent)) this.isDirty = false;
        this.deps.apply(sess);
      })
      .catch((err: unknown) => {
        this.deps.report(err);
        // The write was refused, so it is never going to be acknowledged.
        // Holding the mark from here on would make the page ignore every
        // later change another writer makes, and the user's next edit --
        // which replaces -- would then delete them. The failure has been
        // reported; the session is the truth again, unless the user has
        // edited since this write was sent, in which case that newer edit is
        // still on its way and keeps its protection.
        if (!overridesEqual(this.deps.current(), sent)) return;
        this.isDirty = false;
        return this.deps.read().then(
          (sess) => this.deps.apply(sess),
          // Unreachable too: the next session event, or the reconnection
          // after a restart, brings the session's overrides back.
          () => undefined,
        );
      });
  }
}
