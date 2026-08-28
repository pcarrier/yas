import type { YasSurface } from "@yas-run/core";

/** Prefer the server-stamped desktop-entry identity over self-reported app_id. */
export function surfaceApplicationId(surface: YasSurface): string {
  return surface.origin?.appId || surface.appId;
}
