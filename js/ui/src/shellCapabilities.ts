/**
 * What the surrounding shell can actually offer.
 *
 * The workspace usually runs inside the app served by a yas server, where
 * the page has a Relay-backed session (remotes to add and switch) and a
 * same-origin service worker for web-pane previews. Embedded — yas.run
 * opening a share link — none of that exists: the connection list is fixed
 * by the host, and there is no `sw.js` at the page's origin to register.
 * These flags let the one Workspace serve both lives instead of the embed
 * growing a second, lesser client; the affordances they gate are hidden,
 * not broken, because a menu entry that opens an empty panel is a bug
 * report waiting to be filed.
 *
 * Module state rather than a prop: the flags describe the page, not a
 * component instance, and they are set exactly once before mount.
 */

export interface ShellCapabilities {
  /** The page can manage the current session's Relay remotes. */
  remotes: boolean;
  /** The page can register the preview service worker. */
  previews: boolean;
}

let caps: ShellCapabilities = {
  remotes: true,
  previews: true,
};

export function setShellCapabilities(next: Partial<ShellCapabilities>): void {
  caps = { ...caps, ...next };
}

export function shellCapabilities(): ShellCapabilities {
  return caps;
}
