// "A new build is available", said rather than done.
//
// Most rebuilds never reach this: a stylesheet is swapped and an engine is
// replaced without the user being told anything they have to act on, because
// nothing was lost. What is left is the case where the UI bundle itself moved,
// and a page genuinely cannot replace the code it is running. It does not
// reload on the user's behalf: a reload lands in the middle of a thought, and
// the thought is worth more than the freshness.
//
// So the bar is the *control*, and it has exactly three states:
//
//   nothing            the page is current. No chrome, no badge, no noise.
//   a note             something was replaced and the user lost nothing. One
//                      line, five seconds, no buttons -- this is a receipt,
//                      not a question.
//   an offer           a reload is the only way to take this build. Reload
//                      now, or Not now.
//
// "Not now" collapses the bar to a chip that does not go away. Dismissing
// hides the interruption, not the fact: a page that quietly forgot it was
// behind would leave a browser running an old build with nobody knowing.
//
// If the offer is ignored, nothing breaks. The page keeps working against the
// build it has -- a stale page that refuses to be useful is strictly worse
// than a stale page, and there is nothing a running Cadpole session does that
// becomes *unsafe* because `app.js` moved on disk. What it does not do is
// pretend: the chip stays up, `data-stale-build` is on the document element,
// and any crash report from here on says which build the page is running and
// that a newer one exists.

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import { h } from "./dom.js";

export function createBuildBar(store: AppStore, actions: Actions): HTMLElement {
  const text = h("span", { class: "build-bar-text" });
  const reload = h(
    "button",
    {
      type: "button",
      class: "btn build-bar-reload",
      on: { click: () => actions.reloadForBuild() },
    },
    "Reload now",
  );
  const later = h(
    "button",
    {
      type: "button",
      class: "btn build-bar-later",
      on: { click: () => actions.dismissBuildUpdate() },
    },
    "Not now",
  );

  const bar = h(
    "div",
    {
      class: "build-bar",
      // `status`, not `alert`: this is news, not an error, and an assertive
      // live region that interrupts a screen reader mid-sentence to announce a
      // rebuild is the audible version of reloading the page unasked. The
      // announcement itself is made once, by main.ts, through the announcer.
      role: "status",
      aria: { live: "polite" },
    },
    text,
    reload,
    later,
  );
  bar.hidden = true;

  function paint(): void {
    const s = store.get();
    const update = s.buildUpdate;
    const note = s.hotNote;

    if (update) {
      bar.hidden = false;
      bar.classList.toggle("dismissed", update.dismissed);
      bar.classList.remove("note");
      text.textContent = update.dismissed ? "New build available" : `A new build is available — ${update.reason}.`;
      reload.hidden = false;
      later.hidden = update.dismissed;
      document.documentElement.dataset["staleBuild"] = "1";
      return;
    }

    delete document.documentElement.dataset["staleBuild"];
    if (note) {
      bar.hidden = false;
      bar.classList.add("note");
      bar.classList.remove("dismissed");
      text.textContent = note;
      reload.hidden = true;
      later.hidden = true;
      return;
    }
    bar.hidden = true;
  }

  store.subscribe((s, prev) => {
    if (s.buildUpdate !== prev.buildUpdate || s.hotNote !== prev.hotNote) paint();
  });
  paint();
  return bar;
}
