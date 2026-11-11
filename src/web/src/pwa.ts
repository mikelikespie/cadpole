// PWA glue: service worker registration and the deferred install prompt.

import type { AppStore } from "./state.js";

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

let deferredPrompt: BeforeInstallPromptEvent | null = null;

export function setupPwa(store: AppStore): void {
  window.addEventListener("beforeinstallprompt", (ev) => {
    ev.preventDefault();
    deferredPrompt = ev as BeforeInstallPromptEvent;
    store.set({ installable: true });
  });
  window.addEventListener("appinstalled", () => {
    deferredPrompt = null;
    store.set({ installable: false });
  });

  const params = new URLSearchParams(location.search);
  if (!("serviceWorker" in navigator) || params.get("nosw") === "1") return;
  // Only register on secure origins where a worker can actually control the page.
  if (!window.isSecureContext) return;
  const register = (): void => {
    navigator.serviceWorker
      .register(new URL("./sw.js", import.meta.url).href, { scope: "./" })
      .then((reg) => {
        void navigator.serviceWorker.ready.then(() => store.set({ offlineReady: true }));
        reg.addEventListener("updatefound", () => {
          const w = reg.installing;
          w?.addEventListener("statechange", () => {
            if (w.state === "installed" && navigator.serviceWorker.controller) {
              console.info("cadpole: a new version is available; it will be used on the next load.");
            }
          });
        });
      })
      .catch((err) => console.warn("cadpole: service worker registration failed", err));
  };
  // setupPwa runs after the engine/session probes, i.e. usually after `load`.
  if (document.readyState === "complete") register();
  else window.addEventListener("load", register, { once: true });
}

/** Show the browser's install prompt if one was deferred. */
export async function promptInstall(store: AppStore): Promise<void> {
  const p = deferredPrompt;
  if (!p) return;
  deferredPrompt = null;
  store.set({ installable: false });
  try {
    await p.prompt();
    const choice = await p.userChoice;
    if (choice.outcome !== "accepted") store.set({ installable: true });
  } catch {
    store.set({ installable: true });
  }
}

/**
 * Ask the service worker to throw away every cache, and resolve when it has.
 *
 * Called before a reload triggered by `build_changed`. The worker serves code
 * network-first, so this is belt and braces rather than the only defence, but
 * the failure it guards against is the one that actually happened: a phone
 * loading a broken engine out of cache on every attempt, which is
 * indistinguishable from the page hanging.
 *
 * Resolves either way. A reload that drops no caches is still better than no
 * reload, so a worker that is absent, still installing, or simply does not
 * answer must not block it. One second is the whole budget.
 */
export function dropServiceWorkerCaches(): Promise<void> {
  const sw = navigator.serviceWorker?.controller;
  if (!sw) return Promise.resolve();
  return new Promise<void>((resolve) => {
    const done = (): void => {
      navigator.serviceWorker.removeEventListener("message", onMessage);
      window.clearTimeout(timer);
      resolve();
    };
    const onMessage = (e: MessageEvent): void => {
      if ((e.data as { type?: unknown } | null)?.type === "cadpole:caches-dropped") done();
    };
    const timer = window.setTimeout(done, 1000);
    navigator.serviceWorker.addEventListener("message", onMessage);
    try {
      sw.postMessage({ type: "cadpole:drop-caches" });
    } catch {
      done();
    }
  });
}

/**
 * Track the on-screen keyboard and publish its height as `--kbd`.
 *
 * Chrome on Android shrinks the visual viewport and leaves the layout viewport
 * alone, and iOS Safari does much the same, so a `position: fixed` sheet keeps
 * its old height and the composer ends up underneath the keyboard. The
 * `interactive-widget=resizes-content` viewport hint fixes the Chrome case on
 * its own; this covers the rest, and costs nothing where there is no keyboard
 * because the variable stays at zero.
 *
 * `--kbd` is the difference between the layout viewport and the visual one,
 * ignoring anything under 120px so that a browser's own collapsing toolbar
 * does not read as a keyboard.
 */
/** Dispatched on `window` after `--kbd` is written (`trackKeyboardInset`). */
export const KEYBOARD_EVENT = "cadpole:keyboard";

export function trackKeyboardInset(): void {
  const vv = window.visualViewport;
  if (!vv) return;
  const root = document.documentElement;
  let raf = 0;
  const update = (): void => {
    raf = 0;
    const inset = Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
    const kbd = inset > 120 ? Math.round(inset) : 0;
    root.style.setProperty("--kbd", `${kbd}px`);
    root.classList.toggle("keyboard-up", kbd > 0);
    // The sheet holds the chat, so it is the thing that has to make room.
    document.querySelector(".sheet")?.classList.toggle("keyboard", kbd > 0);
    // The sheet has moved without changing size, which no ResizeObserver
    // reports: whoever places things by where it stands is told here.
    window.dispatchEvent(new Event(KEYBOARD_EVENT));
  };
  const schedule = (): void => {
    if (!raf) raf = window.requestAnimationFrame(update);
  };
  vv.addEventListener("resize", schedule);
  vv.addEventListener("scroll", schedule);
  update();
}

/**
 * Bring the focused text field into view once the keyboard has settled.
 *
 * The viewport resize that the keyboard causes arrives after `focus`, so a
 * scroll issued on focus scrolls to where the field *was*. One frame after the
 * resize is the earliest moment the answer is right.
 */
export function keepFocusedInputVisible(): void {
  document.addEventListener(
    "focusin",
    (e) => {
      const el = e.target;
      if (!(el instanceof HTMLTextAreaElement) && !(el instanceof HTMLInputElement)) return;
      const settle = (): void => {
        window.requestAnimationFrame(() => el.scrollIntoView({ block: "nearest", behavior: "smooth" }));
      };
      if (window.visualViewport) {
        window.visualViewport.addEventListener("resize", settle, { once: true });
        // ...and if no resize comes (a hardware keyboard, a desktop), still do it.
        window.setTimeout(settle, 300);
      } else {
        settle();
      }
    },
    true,
  );
}

/**
 * Stop the *page* zooming, on the one browser that ignores being asked.
 *
 * Safari on iOS has disregarded `user-scalable=no` since iOS 10, so the
 * viewport meta handles every other browser and this handles that one. Pinch
 * belongs to the model: a two-finger gesture that scales the document instead
 * of the camera is the most disorienting thing a 3D app on a phone can do,
 * and it leaves the user zoomed in with no obvious way back.
 *
 * `gesturestart` and friends are Safari-only and are what its page zoom rides
 * on. The viewer's own camera control uses pointer events, so blocking these
 * takes nothing away from it. Double-tap zoom is already handled by
 * `touch-action: manipulation` on the body.
 *
 * This does remove a genuine accessibility affordance, and it is a deliberate
 * trade rather than an oversight: the app has its own zoom, text scales with
 * the system font setting through `-webkit-text-size-adjust`, and every
 * control meets the 44px touch target.
 */
export function preventPageZoom(): void {
  for (const type of ["gesturestart", "gesturechange", "gestureend"]) {
    document.addEventListener(type, (e) => e.preventDefault(), { passive: false });
  }
}
