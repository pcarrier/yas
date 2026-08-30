import { createEffect } from "solid-js";
import type { YasSurface } from "@yas-run/core";
import type { Theme, UIScale } from "./theme";
import { surfaceName } from "./theme";
import { AppIcon } from "./panelKit";
import {
  applicationIcon,
  requestApplicationIcons,
} from "./sessionCatalogs";
import { surfaceApplicationId } from "./surfaceApplicationId";

/** A Wayland window's `.desktop` artwork, sharing the session-catalog cache. */
export function SurfaceIcon(props: {
  surface: YasSurface;
  theme: Theme;
  scale: UIScale;
  size: number;
}) {
  const appId = () => surfaceApplicationId(props.surface);

  createEffect(() => {
    const id = appId();
    if (!id) return;
    // Reading first makes this effect retry when the proactive catalog opens;
    // requestIcons itself is intentionally a no-op until that point.
    applicationIcon(props.surface.connectionId, id);
    requestApplicationIcons(props.surface.connectionId, [id]);
  });

  return (
    <AppIcon
      theme={props.theme}
      scale={props.scale}
      name={surfaceName(props.surface)}
      src={
        appId()
          ? applicationIcon(props.surface.connectionId, appId())
          : null
      }
      size={props.size}
    />
  );
}
