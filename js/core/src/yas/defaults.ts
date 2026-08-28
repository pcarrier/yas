/** Canonical browser family offer used by edge and nested Relay sessions. */

import {
  YAS_CHANNEL_VERSION,
  YAS_CLIENT_VERSION,
  YAS_DESKTOP_VERSION,
  YAS_ENV_VERSION,
  YAS_EVENTS_VERSION,
  YAS_EXTENSION_VERSION,
  YAS_FONT_VERSION,
  YAS_FS_VERSION,
  YAS_GIT_VERSION,
  YAS_KV_VERSION,
  YAS_LSP_VERSION,
  YAS_MEDIA_VERSION,
  YAS_NET_VERSION,
  YAS_PROCESS_VERSION,
  YAS_RELAY_VERSION,
  YAS_SELECTION_VERSION,
  YAS_SURFACE_VERSION,
  YAS_TERMINAL_VERSION,
  YAS_TRANSFER_VERSION,
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_CLIENT,
  YAS_FAMILY_DESKTOP,
  YAS_FAMILY_ENV,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_FONT,
  YAS_FAMILY_FS,
  YAS_FAMILY_GIT,
  YAS_FAMILY_KV,
  YAS_FAMILY_LSP,
  YAS_FAMILY_MEDIA,
  YAS_FAMILY_NET,
  YAS_FAMILY_PROCESS,
  YAS_FAMILY_RELAY,
  YAS_FAMILY_SELECTION,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TERMINAL,
  YAS_FAMILY_TRANSFER,
} from "./generated";
import type { YasConnectionOptions } from "./session";

/**
 * Browser receive inventory. Surface views keep a 16-frame window so a 120 Hz
 * display does not turn browser scheduling into stop-and-wait. At the 4 MiB
 * protocol frame ceiling that reserves 64 MiB per mounted surface, even though
 * ordinary AV1 frames are orders of magnitude smaller. A 128 MiB aggregate
 * therefore refused a second live surface after catalogue watches took their
 * share, leaving a valid Wayland app behind a permanently black canvas.
 *
 * Use the protocol's hard browser cap. This is receive authority, not an eager
 * allocation; actual queues remain bounded by their family windows. Per-view
 * limits still cap every retained frame and the aggregate remains finite.
 */
export const YAS_BROWSER_RECEIVE_MAX_BUFFERED = 1024n * 1024n * 1024n;

/**
 * Offer every canonical v1 family understood by this browser build. Families
 * are optional except Transfer, which supplies bounded bulk delivery to most
 * other families. The list is deliberately in generated family-ID order.
 */
export function yasBrowserConnectionOptions(
  clientRelease = "development",
): YasConnectionOptions {
  return {
    clientName: "yas-browser",
    clientRelease,
    receiveMaxBuffered: YAS_BROWSER_RECEIVE_MAX_BUFFERED,
    // Generic frame compression is intentionally not offered until the
    // browser FrameCodec can encode and decode it. Packed family codecs (for
    // Terminal/Surface/Media payloads) are negotiated independently.
    codecs: [],
    families: [
      {
        family: YAS_FAMILY_TRANSFER,
        versions: [YAS_TRANSFER_VERSION],
        required: true,
      },
      { family: YAS_FAMILY_RELAY, versions: [YAS_RELAY_VERSION] },
      { family: YAS_FAMILY_TERMINAL, versions: [YAS_TERMINAL_VERSION] },
      { family: YAS_FAMILY_CLIENT, versions: [YAS_CLIENT_VERSION] },
      { family: YAS_FAMILY_SURFACE, versions: [YAS_SURFACE_VERSION] },
      { family: YAS_FAMILY_SELECTION, versions: [YAS_SELECTION_VERSION] },
      { family: YAS_FAMILY_DESKTOP, versions: [YAS_DESKTOP_VERSION] },
      { family: YAS_FAMILY_MEDIA, versions: [YAS_MEDIA_VERSION] },
      { family: YAS_FAMILY_FONT, versions: [YAS_FONT_VERSION] },
      { family: YAS_FAMILY_FS, versions: [YAS_FS_VERSION] },
      { family: YAS_FAMILY_GIT, versions: [YAS_GIT_VERSION] },
      { family: YAS_FAMILY_LSP, versions: [YAS_LSP_VERSION] },
      { family: YAS_FAMILY_KV, versions: [YAS_KV_VERSION] },
      { family: YAS_FAMILY_PROCESS, versions: [YAS_PROCESS_VERSION] },
      { family: YAS_FAMILY_NET, versions: [YAS_NET_VERSION] },
      { family: YAS_FAMILY_CHANNEL, versions: [YAS_CHANNEL_VERSION] },
      { family: YAS_FAMILY_EXTENSION, versions: [YAS_EXTENSION_VERSION] },
      { family: YAS_FAMILY_EVENTS, versions: [YAS_EVENTS_VERSION] },
      { family: YAS_FAMILY_ENV, versions: [YAS_ENV_VERSION] },
    ],
  };
}
