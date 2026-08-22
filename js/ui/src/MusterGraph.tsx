import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import type { MusterDiagram } from "./muster";
import { renderMermaid } from "./mermaid";
import { PanelEmpty } from "./panelKit";
import { scrollbarStyle, type Theme, type UIScale } from "./theme";
import { t, tp } from "./i18n";

let renderSerial = 0;

export function MusterGraph(props: {
  diagram: MusterDiagram;
  theme: Theme;
  scale: UIScale;
}) {
  const [svg, setSvg] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);

  createEffect(() => {
    const diagram = props.diagram;
    const theme = props.theme;
    setSvg("");
    setError(null);
    if (diagram.nodes === 0) return;

    let cancelled = false;
    onCleanup(() => {
      cancelled = true;
    });
    const id = `yas-muster-graph-${++renderSerial}`;
    void renderMermaid(id, diagram.source, theme, {
      useMaxWidth: false,
    }).then(
      (rendered) => {
        if (!cancelled) setSvg(rendered);
      },
      (reason) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      },
    );
  });

  return (
    <Show
      when={props.diagram.nodes > 0}
      fallback={
        <PanelEmpty theme={props.theme} scale={props.scale}>
          {t("muster.graphEmpty")}
        </PanelEmpty>
      }
    >
      <div
        data-muster-graph=""
        style={{
          display: "flex",
          "flex-direction": "column",
          flex: "1 1 0",
          "min-height": "6em",
          gap: `${props.scale.gap}px`,
        }}
      >
        <div
          style={{
            display: "flex",
            "flex-wrap": "wrap",
            gap: `${props.scale.gap}px`,
            color: props.theme.dimFg,
            "font-size": `${props.scale.sm}px`,
          }}
        >
          <span>
            {tp(
              props.diagram.nodes === 1 ? "muster.unitOne" : "muster.unitMany",
              { count: props.diagram.nodes },
            )}
          </span>
          <span>
            {tp(
              props.diagram.edges === 1
                ? "muster.dependencyOne"
                : "muster.dependencyMany",
              { count: props.diagram.edges },
            )}
          </span>
          <span>{t("muster.dependencyDirection")}</span>
          <span style={{ color: props.theme.success }}>
            ● {t("muster.running")}
          </span>
          <span style={{ color: props.theme.warning }}>
            ◐ {t("muster.pending")}
          </span>
          <span style={{ color: props.theme.errorText }}>
            ! {t("muster.failed")}
          </span>
          <span>
            ✓ {t("muster.done")} · ○ {t("muster.inactive")}
          </span>
        </div>
        <div
          style={{
            ...scrollbarStyle(props.theme),
            overflow: "auto",
            flex: "1 1 0",
            "min-height": "0",
            border: `1px solid ${props.theme.subtleBorder}`,
            "background-color": props.theme.panelBg,
            padding: `${props.scale.gap}px`,
          }}
        >
          <Show
            when={!error()}
            fallback={
              <PanelEmpty theme={props.theme} scale={props.scale}>
                {tp("muster.graphError", { error: error() ?? "" })}
              </PanelEmpty>
            }
          >
            <Show
              when={svg()}
              fallback={
                <PanelEmpty theme={props.theme} scale={props.scale}>
                  {t("muster.graphDrawing")}
                </PanelEmpty>
              }
            >
              <div
                data-muster-graph-svg=""
                style={{ width: "max-content", "min-width": "100%" }}
                // Mermaid runs in strict mode and the source labels are escaped.
                innerHTML={svg()}
              />
            </Show>
          </Show>
        </div>
      </div>
    </Show>
  );
}
