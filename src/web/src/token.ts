// The session token, in the browser: where it arrives, where it is kept, and
// where it is deliberately not kept.
//
// `cadpole serve` prints one URL per run:
//
//     http://<host>:7331/#token=<32 bytes of base64url>
//
// **It arrives in the fragment.** A fragment is never sent to a server, never
// appears in an access log, never appears in a `Referer`, and is not visible to
// the `cadpole serve` process itself. A query string is all four of those things,
// which is why the SSE endpoint does not accept one.
//
// **It is kept in `sessionStorage`, not `localStorage`.** Three reasons, in
// order of how much they matter:
//
//   1. `localStorage` outlives the server run that issued the token, so after
//      a restart every tab would hold a token that is now useless, and the
//      only cure would be a manual clear. Tokens are per-run now.
//   2. `sessionStorage` dies with the tab. On a shared or unattended machine a
//      closed tab is the end of the grant, not the beginning of an indefinite
//      one.
//   3. It is per-tab, so a token pasted into one tab is not handed to every
//      other page of the origin that happens to be open.
//
// It is not in a cookie, which is the other obvious place, because a cookie is
// ambient authority: the browser would attach it to requests the page did not
// make deliberately, which is exactly the confused-deputy hole that the Origin
// checks exist to close. Nothing here is defence against script injection on
// this origin — a script that runs here can read `sessionStorage` and can also
// just call the API — which is what the strict CSP is for.
//
// The fragment is stripped from the address bar as soon as it is read, so the
// token is not in the visible URL, not in a screenshot of the window, and not
// in whatever the user copies out of the address bar to send to someone.

const STORAGE_KEY = "cadpole.session.token";

/** Held here so the common path never touches storage at all. */
let cached: string | null = null;
let loaded = false;

function read(): string | null {
  try {
    return sessionStorage.getItem(STORAGE_KEY);
  } catch {
    // Private mode, or storage disabled: the in-memory copy still works for
    // the life of this page.
    return null;
  }
}

function write(token: string): void {
  try {
    sessionStorage.setItem(STORAGE_KEY, token);
  } catch {
    /* in-memory only */
  }
}

/**
 * Take `#token=` out of the URL if it is there, remember it, and rewrite the
 * address bar without it. Call once, before the first request.
 *
 * Any other fragment parameters are preserved, so a playground `#ex=`/`#p=`
 * link that also carries a token keeps working.
 */
export function adoptTokenFromUrl(): void {
  const raw = location.hash.replace(/^#/, "");
  if (!raw) return;
  const params = new URLSearchParams(raw);
  const token = params.get("token");
  if (!token) return;
  cached = token;
  loaded = true;
  write(token);
  params.delete("token");
  const rest = params.toString();
  // replaceState, not assign: no new history entry, so Back does not walk
  // into a URL that still has the token in it.
  history.replaceState(null, "", rest ? `#${rest}` : location.pathname + location.search);
}

/** The token for this tab, or null if there is none. */
export function sessionToken(): string | null {
  if (!loaded) {
    cached = read();
    loaded = true;
  }
  return cached;
}

/** `Authorization` for a fetch, or nothing when there is no token. */
export function authHeaders(): Record<string, string> {
  const t = sessionToken();
  return t ? { authorization: `Bearer ${t}` } : {};
}

/**
 * Forget the token. Called on a 401: the server has been restarted and minted
 * a new one, so keeping the old one only produces more 401s.
 */
export function clearToken(): void {
  cached = null;
  loaded = true;
  try {
    sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}
