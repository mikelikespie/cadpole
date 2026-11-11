// Messages: chat-like list between user and assistant plus a composer with
// queued snapshot attachments. In static mode there is no assistant; the
// list doubles as local notes.

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import type { Attachment, Message } from "../types.js";
import { fmtTime } from "../util.js";
import { h, replaceChildren } from "./dom.js";

function thumb(a: Attachment, alt: string): HTMLElement {
  const src = `data:image/png;base64,${a.png_base64}`;
  return h(
    "a",
    { class: "msg-img", href: src, target: "_blank", rel: "noopener", title: "Open screenshot" },
    h("img", { src, alt, loading: "lazy" }),
  );
}

export function createMessagesPanel(store: AppStore, actions: Actions): HTMLElement {
  const list = h("div", { class: "msg-list", role: "log", aria: { live: "polite", relevant: "additions" } });
  const attachStrip = h("div", { class: "attach-strip", aria: { label: "Queued snapshots" } });
  attachStrip.hidden = true;
  const input = h("textarea", { class: "msg-input", rows: 1, placeholder: "Message the assistant…", aria: { label: "Message" } });
  const send = h("button", { type: "button", class: "btn btn-primary msg-send", title: "Send (Enter)", on: { click: () => submit() } }, "Send");
  const title = h("h2", { class: "section-title" }, "Messages");
  const clients = h("span", { class: "muted small" });
  // Whether what is typed here reaches an attached assistant. Off until the
  // user turns it on; shown only where there is a session server.
  const listen = h(
    "button",
    {
      type: "button",
      class: "btn listen-toggle",
      title: "Pass your messages to an attached assistant",
      aria: { pressed: "false" },
      on: { click: () => void actions.setListening(listen.getAttribute("aria-pressed") !== "true") },
    },
    "Listen",
  );
  listen.hidden = true;
  const root = h(
    "section",
    { class: "panel-section messages", aria: { label: "Messages" } },
    h("div", { class: "section-head" }, title, h("span", { class: "section-tools" }, clients, listen)),
    list,
    attachStrip,
    h("form", { class: "msg-composer", on: { submit: (ev: Event) => ev.preventDefault() } }, input, send),
  );

  // Messages are sent one after another, in the order they were entered. The
  // composer is cleared as soon as one is taken, so the next can be typed
  // while it is being sent; a message that fails to send is put back if the
  // composer is still empty.
  let queue: Promise<void> = Promise.resolve();
  function submit(): void {
    const text = input.value.trim();
    const hasAttachments = store.get().pendingAttachments.length > 0;
    if (!text && !hasAttachments) return;
    input.value = "";
    autosize();
    queue = queue.then(async () => {
      try {
        await actions.sendMessage(text || "(screenshot)");
      } catch (err) {
        if (!input.value.trim()) {
          input.value = text;
          autosize();
        }
        throw err;
      }
    });
    // The next message is still sent; the failure goes where uncaught
    // errors go, which the page reports (failure.ts).
    queue = queue.catch((err: unknown) => globalThis.reportError?.(err));
    input.focus();
  }
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && !ev.shiftKey) {
      ev.preventDefault();
      submit();
    }
  });
  const autosize = (): void => {
    input.style.height = "auto";
    input.style.height = `${Math.min(140, input.scrollHeight)}px`;
  };
  input.addEventListener("input", autosize);

  // One element per message, kept across renders: a reply that is being
  // written changes many times a second, and rebuilding the list each time
  // would restart its animation, lose a text selection and move the scroll
  // position. Only what changed in a message is touched.
  interface Row {
    el: HTMLElement;
    text: HTMLElement;
    activity: HTMLElement;
    shown: Message;
  }
  const rows = new Map<number, Row>();
  let empty: HTMLElement | null = null;

  function attachmentsOf(m: Message): HTMLElement | null {
    return m.attachments?.length
      ? h("div", { class: "msg-attachments" }, ...m.attachments.map((a, i) => thumb(a, `Screenshot ${i + 1}${a.annotated ? " (annotated)" : ""}`)))
      : null;
  }

  function createRow(m: Message): Row {
    const text = h("div", { class: "msg-text" }, m.text);
    // In the meta line, so that it coming and going never changes the height
    // of the message.
    const activity = h("span", { class: "msg-activity" });
    const el = h(
      "div",
      { class: `msg from-${m.from}`, dataset: { seq: String(m.seq) } },
      h("div", { class: "msg-meta" }, h("span", { class: "msg-from" }, m.from), h("time", { dateTime: m.at }, fmtTime(m.at)), activity),
      text,
      attachmentsOf(m),
    );
    const row = { el, text, activity, shown: { ...m, text: m.text } };
    applyState(row, m);
    return row;
  }

  function applyState(row: Row, m: Message): void {
    row.el.classList.toggle("streaming", m.state === "streaming");
    row.el.classList.toggle("failed", m.state === "failed");
    // A screen reader hears the reply once, when it is complete, not every
    // piece of it.
    if (m.state === "streaming") row.el.setAttribute("aria-busy", "true");
    else row.el.removeAttribute("aria-busy");
    row.activity.textContent = m.state === "streaming" ? (m.activity ?? "") : "";
  }

  function updateRow(row: Row, m: Message): void {
    const had = row.shown;
    if (m.text !== had.text) {
      // Text that only grew is appended, so what is already on screen is
      // not laid out again.
      if (m.text.startsWith(had.text) && row.text.firstChild) row.text.append(m.text.slice(had.text.length));
      else row.text.textContent = m.text;
    }
    if (m.state !== had.state || (m.activity ?? "") !== (had.activity ?? "")) applyState(row, m);
    if ((m.attachments?.length ?? 0) !== (had.attachments?.length ?? 0)) {
      row.el.querySelector(".msg-attachments")?.remove();
      const atts = attachmentsOf(m);
      if (atts) row.el.append(atts);
    }
    row.shown = m;
  }

  /** How close to the end of the list counts as "reading the latest". */
  const PIN_SLACK = 32;

  // Whether the reader is at the end of the list, kept as they scroll rather
  // than measured at each render: a screenshot that finishes loading above
  // the end makes the list taller without anyone having scrolled away.
  // A scroll by the reader is noticed by the position having moved since
  // this code last set it, which does not wait for the scroll event.
  let stick = true;
  let setTop = -1;
  const atEnd = (): boolean => list.scrollHeight - list.scrollTop - list.clientHeight <= PIN_SLACK;
  const toEnd = (): void => {
    list.scrollTop = list.scrollHeight;
    setTop = list.scrollTop;
    stick = true;
  };
  const readerMoved = (): void => {
    if (list.scrollTop !== setTop) stick = atEnd();
  };
  list.addEventListener("scroll", readerMoved, { passive: true });
  list.addEventListener(
    "load",
    () => {
      readerMoved();
      if (stick) toEnd();
    },
    true,
  );

  let lastRendered: Message[] | null = null;
  function render(messages: Message[]): void {
    if (messages === lastRendered) return;
    lastRendered = messages;
    if (messages.length === 0) {
      rows.clear();
      empty = h(
        "div",
        { class: "empty" },
        store.get().apiMode === "session"
          ? "No messages yet. Ask the assistant to change something, or select geometry and describe what you want."
          : "No session server: messages stay in this browser as notes.",
      );
      replaceChildren(list, empty);
      return;
    }
    // Follow the end of the list only for someone who is at the end: a
    // reader who scrolled up to look at something stays where they are.
    readerMoved();
    const pinned = stick;
    empty?.remove();
    empty = null;
    const seen = new Set<number>();
    let own = false;
    let at: ChildNode | null = list.firstChild;
    for (const m of messages) {
      seen.add(m.seq);
      let row = rows.get(m.seq);
      // A message whose author or time changed is another message under the
      // same number (the server restarted); it gets a new element.
      if (row && (row.shown.from !== m.from || row.shown.at !== m.at)) {
        row.el.remove();
        row = undefined;
      }
      if (!row) {
        row = createRow(m);
        rows.set(m.seq, row);
        if (m.from === "user") own = true;
      } else if (row.shown !== m) {
        updateRow(row, m);
      }
      if (at === row.el) at = at.nextSibling;
      else list.insertBefore(row.el, at);
    }
    for (const [seq, row] of rows) {
      if (!seen.has(seq)) {
        row.el.remove();
        rows.delete(seq);
      }
    }
    // What the user just sent is always brought into view.
    if (pinned || own) toEnd();
  }

  function renderPending(atts: Attachment[]): void {
    attachStrip.hidden = atts.length === 0;
    replaceChildren(
      attachStrip,
      ...atts.map((a, i) =>
        h(
          "div",
          { class: "attach-thumb" },
          thumb(a, `Queued screenshot ${i + 1}`),
          h("span", { class: "attach-tag" }, a.annotated ? "annotated" : "plain"),
          h(
            "button",
            {
              type: "button",
              class: "icon-btn attach-x",
              aria: { label: `Remove queued screenshot ${i + 1}` },
              on: { click: () => actions.removeAttachment(i) },
            },
            "×",
          ),
        ),
      ),
    );
  }

  store.subscribe((s, prev) => {
    if (s.session?.messages !== prev.session?.messages) render(s.session?.messages ?? []);
    if (s.pendingAttachments !== prev.pendingAttachments) renderPending(s.pendingAttachments);
    if (s.session?.clients !== prev.session?.clients) {
      const n = s.session?.clients ?? 0;
      clients.textContent = s.apiMode === "session" && n > 0 ? `${n} client${n === 1 ? "" : "s"}` : "";
    }
    if (s.apiMode !== prev.apiMode) applyMode(s.apiMode);
    if (s.session?.assistantListening !== prev.session?.assistantListening || s.apiMode !== prev.apiMode) applyListening();
    if (s.focusMessage !== prev.focusMessage) input.focus();
  });
  function applyMode(mode: "session" | "static" | "published"): void {
    title.textContent = mode === "session" ? "Messages" : "Notes";
    root.setAttribute("aria-label", mode === "session" ? "Messages" : "Notes");
    input.placeholder = mode === "session" ? "Message the assistant…" : "Add a note (kept locally)…";
  }
  function applyListening(): void {
    const s = store.get();
    const on = s.session?.assistantListening;
    listen.hidden = s.apiMode !== "session" || on === undefined;
    listen.setAttribute("aria-pressed", String(on === true));
  }
  applyMode(store.get().apiMode);
  applyListening();
  render(store.get().session?.messages ?? []);
  renderPending(store.get().pendingAttachments);
  return root;
}
