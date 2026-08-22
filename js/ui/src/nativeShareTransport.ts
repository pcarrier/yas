import { createShareTransport } from "@yas-run/core";
import type { YasDebug, YasTransport } from "@yas-run/core";

export function shareTransport(
  hubUrl: string,
  passphrase: string,
  debug?: YasDebug,
): YasTransport {
  // Workspace constructs one typed YAS session over this raw WebRTC stream.
  return createShareTransport(hubUrl, passphrase, debug);
}
