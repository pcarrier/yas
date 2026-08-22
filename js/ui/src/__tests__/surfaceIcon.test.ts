import { describe, expect, it } from "vitest";
import type { YasSurface } from "@yas-run/core";
import { surfaceApplicationId } from "../SurfaceIcon";

describe("surface window icons", () => {
  it("uses the stamped desktop entry id before the client's app_id", () => {
    expect(
      surfaceApplicationId({
        appId: "com.spotify.Client",
        origin: {
          sandboxEngine: "yas",
          appId: "spotify",
          instanceId: "one",
        },
      } as YasSurface),
    ).toBe("spotify");
  });

  it("falls back to app_id for surfaces without a stamped origin", () => {
    expect(surfaceApplicationId({ appId: "foot" } as YasSurface)).toBe("foot");
  });
});
