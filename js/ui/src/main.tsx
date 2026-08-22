import { render } from "solid-js/web";
import { initWasm } from "./wasm";
import { App } from "./App";
import { t } from "./i18n";

// The mark from the repository's logo.svg, spelled for a data URI: "#"
// percent-encoded, and the three spokes written out rather than referenced
// through <use href="#spoke">, which not every renderer resolves inside a
// data: URI. Naked, exactly as the website draws it — no plate behind it.
const MARK_SVG =
  "<circle cx='128' cy='128' r='120' fill='none' stroke='%23000' stroke-width='16'/>" +
  "<g fill='%23000'>" +
  "<rect x='120' y='128' width='16' height='120'/>" +
  "<rect x='120' y='128' width='16' height='120' transform='rotate(120 128 128)'/>" +
  "<rect x='120' y='128' width='16' height='120' transform='rotate(240 128 128)'/>" +
  "</g>";

const ICON_SVG =
  "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 256 256'>" +
  MARK_SVG +
  "</svg>";

// The one place a plate is not a choice: a maskable icon is cropped to
// whatever shape the launcher likes, so it must be opaque edge to edge. White,
// which is the ground the mark is drawn on everywhere else.
const MASKABLE_SVG =
  "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 256 256'>" +
  "<rect width='256' height='256' fill='%23fff'/>" +
  "<g transform='translate(25.6 25.6) scale(0.8)'>" +
  MARK_SVG +
  "</g></svg>";

/**
 * The same mark, drawn with canvas commands and handed back as a PNG.
 *
 * Chromium's installed-app icon pipeline is raster-only: an SVG-only manifest
 * leaves it with nothing to rasterise, and it falls back to a letter tile — a
 * bare "Y" in the macOS Dock, which is what this exists to stop. Drawn rather
 * than decoded from the SVG above so there is no image load to await and no
 * chance of a half-painted canvas.
 *
 * Plated in white, the ground the mark is drawn on everywhere else: a Dock
 * icon is composited over whatever the wallpaper is, and a black mark on
 * transparency disappears against half of them.
 */
function markPng(size: number): string {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) return "";
  context.scale(size / 256, size / 256);
  context.fillStyle = "#fff";
  context.fillRect(0, 0, 256, 256);
  // Inset, so the mark keeps its air when a launcher crops to a circle.
  context.translate(25.6, 25.6);
  context.scale(0.8, 0.8);
  context.strokeStyle = "#000";
  context.lineWidth = 16;
  context.beginPath();
  context.arc(128, 128, 120, 0, Math.PI * 2);
  context.stroke();
  context.fillStyle = "#000";
  for (const turn of [0, 120, 240]) {
    context.save();
    context.translate(128, 128);
    context.rotate((turn * Math.PI) / 180);
    context.translate(-128, -128);
    context.fillRect(120, 128, 16, 120);
    context.restore();
  }
  return canvas.toDataURL("image/png");
}

// Inject a Web App Manifest dynamically so the app is installable even when
// served as a single inlined HTML file (no separate manifest.json).
{
  const SCREENSHOT_SVG =
    "<svg xmlns='http://www.w3.org/2000/svg' width='1280' height='800'>" +
    "<rect width='1280' height='800' fill='%23111'/>" +
    "<text x='640' y='380' text-anchor='middle' font-family='monospace' font-size='48' font-weight='bold' fill='%2358f'>YAS</text>" +
    `<text x='640' y='440' text-anchor='middle' font-family='monospace' font-size='20' fill='%23888'>${encodeURIComponent(t("app.terminalMultiplexer"))}</text>` +
    "</svg>";

  const manifest = {
    name: "YAS",
    short_name: "YAS",
    description: t("app.description"),
    start_url: location.origin + location.pathname,
    display: "standalone",
    background_color: "#000",
    theme_color: "#000",
    icons: [
      ...[192, 512].flatMap((size) => {
        const src = markPng(size);
        return src
          ? [
              {
                src,
                sizes: `${size}x${size}`,
                type: "image/png",
                purpose: "any maskable",
              },
            ]
          : [];
      }),
      {
        src: `data:image/svg+xml,${ICON_SVG}`,
        sizes: "any",
        type: "image/svg+xml",
        purpose: "any",
      },
      {
        src: `data:image/svg+xml,${MASKABLE_SVG}`,
        sizes: "any",
        type: "image/svg+xml",
        purpose: "maskable",
      },
    ],
    screenshots: [
      {
        src: `data:image/svg+xml,${SCREENSHOT_SVG}`,
        sizes: "1280x800",
        type: "image/svg+xml",
        form_factor: "wide",
        label: t("app.screenshotLabel"),
      },
    ],
  };
  const blob = new Blob([JSON.stringify(manifest)], {
    type: "application/json",
  });
  // Idempotent for the same reason the mount below is: appending a second
  // manifest link would leave the document with two.
  const link =
    document.head.querySelector<HTMLLinkElement>('link[rel="manifest"]') ??
    document.head.appendChild(document.createElement("link"));
  link.rel = "manifest";
  link.href = URL.createObjectURL(blob);

  // Safari never reads the manifest's icons for a saved app; this link is the
  // only thing it looks at.
  const touch = markPng(180);
  if (touch) {
    const apple =
      document.head.querySelector<HTMLLinkElement>(
        'link[rel="apple-touch-icon"]',
      ) ?? document.head.appendChild(document.createElement("link"));
    apple.rel = "apple-touch-icon";
    apple.href = touch;
  }
}

initWasm().then((wasm) => {
  // Mount idempotently. `render()` appends and never clears, so a second
  // execution of this module body would leave two whole app trees in
  // `#root` — two docks, two layout containers fighting over the same
  // workspace's visible sessions, and a document twice the viewport tall.
  // Nothing should re-execute the entry (see installPrompt.ts on why the
  // entry must stay importer-free), but the guard is cheap and the failure
  // mode is not.
  (import.meta.hot?.data?.dispose as (() => void) | undefined)?.();
  // Not `getElementById("root")!` — that assertion turned a missing mount
  // point into "Uncaught (in promise) Error: The `element` passed to
  // render(...) doesn't exist", which names the symptom and not the cause.
  // The usual cause is a document that is not index.html (a stray dev
  // entry, a stale tab), so say that.
  const root = document.getElementById("root");
  if (!root) {
    throw new Error(
      "yas: no #root element in this document — index.html is the only " +
        "page that hosts the app; a stale or hand-written entry will not work",
    );
  }
  const dispose = render(() => <App wasm={wasm} />, root);
  if (import.meta.hot) import.meta.hot.data.dispose = dispose;
});
