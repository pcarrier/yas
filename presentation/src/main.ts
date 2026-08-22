import "./style.css";
import mermaid from "mermaid";
import { explanationFor, type ExplanationKind } from "./explanations";
import { LaunchpadMiniMk3 } from "./launchpad";
import { sections, slideIndexFromPath, slides } from "./slides";

const root = document.querySelector<HTMLDivElement>("#app");
if (!root) throw new Error("Missing #app");

root.innerHTML = `
  <main class="deck" aria-live="polite">
    <header class="topline">
      <button class="wordmark" data-action="first" aria-label="First slide">YAS</button>
      <div class="section-line" aria-label="Sections"></div>
      <button class="midi-status" data-action="midi" aria-label="Connect Launchpad Mini Mk3">
        <span class="midi-dot"></span><span class="midi-copy">Launchpad</span>
      </button>
    </header>
    <section class="stage" aria-label="Presentation">
      <article class="slide-shell">
        <div class="slide-content"></div>
        <footer class="slide-footer">
          <span class="slide-section"></span>
          <span class="slide-position"></span>
        </footer>
      </article>
    </section>
    <div class="progress" aria-hidden="true"><div class="progress-value"></div></div>
    <nav class="controls" aria-label="Presentation controls">
      <button data-action="previous" aria-label="Previous slide">←</button>
      <button data-action="overview" aria-label="Slide overview">GRID</button>
      <button data-action="next" aria-label="Next slide">→</button>
      <button data-action="blackout" aria-label="Blackout">BLACK</button>
      <button data-action="fullscreen" aria-label="Fullscreen">FULL</button>
      <button data-action="help" aria-label="Keyboard and Launchpad help">?</button>
    </nav>
  </main>
  <section class="overview" aria-label="Slide overview" hidden>
    <div class="overview-head"><div><span>YAS</span><h2>Slide grid</h2></div><button data-action="overview">Close</button></div>
    <div class="overview-grid"></div>
  </section>
  <section class="help" aria-label="Controls help" hidden>
    <div class="help-panel">
      <div class="help-head"><div><span>CONTROL SURFACE</span><h2>Keyboard + Launchpad</h2></div><button data-action="help">Close</button></div>
      <div class="help-columns">
        <div><h3>Keyboard</h3><dl>
          <div><dt>← → / Space</dt><dd>Previous / next</dd></div>
          <div><dt>[ ]</dt><dd>Previous / next section</dd></div>
          <div><dt>Home / End</dt><dd>First / last</dd></div>
          <div><dt>O</dt><dd>Overview</dd></div>
          <div><dt>B</dt><dd>Blackout</dd></div>
          <div><dt>F</dt><dd>Fullscreen</dd></div>
          <div><dt>M</dt><dd>Connect controller</dd></div>
        </dl></div>
        <div><h3>Launchpad Mini Mk3</h3><dl>
          <div><dt>Upper grid pads</dt><dd>Direct slide access</dd></div>
          <div><dt>Bottom grid row</dt><dd>Eight section starts</dd></div>
          <div><dt>Every edge button</dt><dd>Untouched</dd></div>
          <div><dt>White pad</dt><dd>Current slide or section</dd></div>
        </dl></div>
      </div>
      <div class="midi-connect-row">
        <button class="primary" data-action="midi">Connect Launchpad Mini Mk3</button>
        <p>Uses only the unlabeled 8×8 grid in DAW Session mode. Disconnecting restores Standalone mode. Web MIDI requires a supporting browser on HTTPS or localhost.</p>
      </div>
    </div>
  </section>
  <div class="blackout" aria-label="Blackout" hidden><span>Press B to return</span></div>
  <aside class="line-tooltip" role="tooltip" hidden></aside>
`;

const deck = root.querySelector<HTMLElement>(".deck")!;
const shell = root.querySelector<HTMLElement>(".slide-shell")!;
const content = root.querySelector<HTMLElement>(".slide-content")!;
const sectionLabel = root.querySelector<HTMLElement>(".slide-section")!;
const positionLabel = root.querySelector<HTMLElement>(".slide-position")!;
const progress = root.querySelector<HTMLElement>(".progress-value")!;
const sectionLine = root.querySelector<HTMLElement>(".section-line")!;
const overview = root.querySelector<HTMLElement>(".overview")!;
const overviewGrid = root.querySelector<HTMLElement>(".overview-grid")!;
const help = root.querySelector<HTMLElement>(".help")!;
const blackout = root.querySelector<HTMLElement>(".blackout")!;
const midiCopy = root.querySelector<HTMLElement>(".midi-copy")!;
const midiDot = root.querySelector<HTMLElement>(".midi-dot")!;
const lineTooltip = root.querySelector<HTMLElement>(".line-tooltip")!;

let index = slideIndexFromPath(window.location.pathname);
let overviewOpen = false;
let helpOpen = false;
let blackedOut = false;
let pointerStart: { x: number; y: number } | null = null;
let suppressClick = false;
let controlsTimer = 0;
let renderRevision = 0;
let explainedLine: HTMLElement | null = null;

const launchpad = new LaunchpadMiniMk3({
  goToSlide: (target) => go(target),
  goToSection: (target) => go(sections[target]?.start ?? index),
  status: (message, connected) => {
    midiCopy.textContent = message;
    midiDot.classList.toggle("connected", connected);
  },
});

function go(target: number): void {
  const next = Math.max(0, Math.min(slides.length - 1, target));
  if (next === index && !overviewOpen && !helpOpen && !blackedOut) return;
  index = next;
  setOverview(false);
  setHelp(false);
  setBlackout(false);
  render();
}

function moveSection(delta: number): void {
  const current = slides[index].sectionIndex;
  const target = Math.max(0, Math.min(sections.length - 1, current + delta));
  go(sections[target].start);
}

function setOverview(open: boolean): void {
  overviewOpen = open;
  overview.hidden = !open;
  deck.setAttribute("aria-hidden", String(open));
  if (open) setHelp(false);
  syncController();
}

function setHelp(open: boolean): void {
  helpOpen = open;
  help.hidden = !open;
  deck.setAttribute("aria-hidden", String(open));
  if (open) setOverview(false);
  syncController();
}

function setBlackout(active: boolean): void {
  blackedOut = active;
  blackout.hidden = !active;
  if (active) {
    setOverview(false);
    setHelp(false);
  }
  syncController();
}

async function toggleFullscreen(): Promise<void> {
  if (document.fullscreenElement) await document.exitFullscreen();
  else await document.documentElement.requestFullscreen();
}

async function toggleMidi(): Promise<void> {
  if (launchpad.connected) launchpad.disconnect();
  else await launchpad.connect();
}

function syncController(): void {
  launchpad.update({
    slideIndex: index,
    slideMidiColors: slides.map((slide) => slide.midiColor),
    sectionStarts: sections.map((section) => section.start),
    sectionMidiColors: sections.map((section) => section.midiColor),
  });
}

function hideExplanation(): void {
  explainedLine = null;
  lineTooltip.hidden = true;
}

function positionExplanation(clientX: number, clientY: number): void {
  if (lineTooltip.hidden) return;
  const gap = 18;
  const edge = 14;
  const rect = lineTooltip.getBoundingClientRect();
  const left = Math.max(
    edge,
    Math.min(clientX + gap, window.innerWidth - rect.width - edge),
  );
  const below = clientY + gap;
  const top =
    below + rect.height <= window.innerHeight - edge
      ? below
      : Math.max(edge, clientY - rect.height - gap);
  lineTooltip.style.left = `${left}px`;
  lineTooltip.style.top = `${top}px`;
}

function annotateLines(slideId: string, slideTitle: string): void {
  content
    .querySelectorAll<HTMLElement>("pre > code:not(.language-mermaid)")
    .forEach((code) => {
      const lines = (code.textContent ?? "").replace(/\n$/, "").split("\n");
      code.replaceChildren(
        ...lines.flatMap((line, lineIndex) => {
          const span = document.createElement("span");
          span.className = "code-line";
          span.textContent = line || " ";
          return lineIndex < lines.length - 1
            ? [span, document.createTextNode("\n")]
            : [span];
        }),
      );
    });

  const targets = content.querySelectorAll<HTMLElement>(
    ":scope > h1, :scope > p, :scope li, :scope > .mermaid, :scope .code-line",
  );
  let textLineIndex = 0;
  targets.forEach((target) => {
    const kind: ExplanationKind = target.matches("h1")
      ? "title"
      : target.matches(".mermaid")
        ? "diagram"
        : target.matches(".code-line")
          ? "code"
          : "text";
    target.classList.add("explainable-line");
    const lineIndex = kind === "text" ? textLineIndex++ : undefined;
    target.dataset.explanation = explanationFor({
      slideId,
      slideTitle,
      kind,
      text: target.textContent ?? "",
      lineIndex,
    });
  });
}

function render(): void {
  hideExplanation();
  const revision = ++renderRevision;
  const slide = slides[index];
  content.innerHTML = slide.html;
  content.querySelectorAll<HTMLAnchorElement>("a").forEach((link) => {
    link.target = "_blank";
    link.rel = "noreferrer";
  });
  const diagrams = [
    ...content.querySelectorAll<HTMLElement>("pre > code.language-mermaid"),
  ].map((code) => {
    const diagram = document.createElement("div");
    diagram.className = "mermaid";
    diagram.textContent = code.textContent;
    code.parentElement?.replaceWith(diagram);
    return diagram;
  });
  shell.dataset.layout = slide.layout;
  shell.dataset.slide = slide.id;
  shell.dataset.number = String(index + 1).padStart(2, "0");
  shell.style.setProperty("--accent", slide.color);
  deck.style.setProperty("--accent", slide.color);
  lineTooltip.style.setProperty("--accent", slide.color);
  sectionLabel.textContent = `${String(slide.sectionIndex + 1).padStart(2, "0")} · ${slide.sectionName}`;
  positionLabel.textContent = `${String(index + 1).padStart(2, "0")} / ${slides.length}`;
  progress.style.width = `${((index + 1) / slides.length) * 100}%`;
  overviewGrid.querySelectorAll("button").forEach((button, cardIndex) => {
    button.classList.toggle("active", cardIndex === index);
  });
  sectionLine.querySelectorAll("button").forEach((button, sectionIndex) => {
    button.classList.toggle("active", sectionIndex === slide.sectionIndex);
  });
  document.title = `${index + 1}/${slides.length} · ${slide.title} · YAS`;
  history.replaceState(null, "", `/${slide.id}${window.location.search}`);
  shell.classList.remove("enter");
  void shell.offsetWidth;
  shell.classList.add("enter");
  annotateLines(slide.id, slide.title);
  if (diagrams.length > 0) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      flowchart: {
        curve: "basis",
        htmlLabels: false,
        nodeSpacing: 34,
        rankSpacing: 44,
      },
      themeVariables: {
        background: "transparent",
        primaryColor: "#111411",
        primaryTextColor: "#f5f5ef",
        primaryBorderColor: slide.color,
        secondaryColor: "#111411",
        secondaryTextColor: "#f5f5ef",
        secondaryBorderColor: slide.color,
        tertiaryColor: "#090a09",
        tertiaryTextColor: "#f5f5ef",
        tertiaryBorderColor: "#4f534d",
        lineColor: slide.color,
        textColor: "#f5f5ef",
        edgeLabelBackground: "#090a09",
        clusterBkg: "#090a09",
        clusterBorder: "#4f534d",
        fontFamily: "Inter, Helvetica Neue, Arial, sans-serif",
      },
    });
    queueMicrotask(() => {
      if (revision !== renderRevision) return;
      void mermaid.run({ nodes: diagrams }).catch((error: unknown) => {
        if (revision === renderRevision)
          console.error("Mermaid render failed", error);
      });
    });
  }
  syncController();
}

function renderStaticNavigation(): void {
  sectionLine.innerHTML = sections
    .map(
      (section) => `
    <button data-section="${section.index}" style="--section-color:${section.color}" aria-label="Go to ${section.name}">
      <span>${section.name}</span>
    </button>
  `,
    )
    .join("");
  overviewGrid.innerHTML = slides
    .map(
      (slide, slideIndex) => `
    <button data-slide="${slideIndex}" style="--card-accent:${slide.color}" aria-label="Slide ${slideIndex + 1}: ${slide.title}">
      <span class="card-number">${String(slideIndex + 1).padStart(2, "0")}</span>
      <span class="card-section">${slide.sectionName}</span>
      <strong>${slide.title}</strong>
    </button>
  `,
    )
    .join("");
}

function showControls(): void {
  deck.classList.add("controls-visible");
  window.clearTimeout(controlsTimer);
  controlsTimer = window.setTimeout(
    () => deck.classList.remove("controls-visible"),
    1800,
  );
}

root.addEventListener("click", (event) => {
  const target = event.target as HTMLElement;
  const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
  if (action) {
    event.stopPropagation();
    if (action === "previous") go(index - 1);
    if (action === "next") go(index + 1);
    if (action === "first") go(0);
    if (action === "overview") setOverview(!overviewOpen);
    if (action === "blackout") setBlackout(!blackedOut);
    if (action === "fullscreen") void toggleFullscreen();
    if (action === "help") setHelp(!helpOpen);
    if (action === "midi") void toggleMidi();
    return;
  }
  const section =
    target.closest<HTMLElement>("[data-section]")?.dataset.section;
  if (section !== undefined) {
    go(sections[Number(section)]?.start ?? index);
    return;
  }
  const slide = target.closest<HTMLElement>("[data-slide]")?.dataset.slide;
  if (slide !== undefined) {
    go(Number(slide));
    return;
  }
  if (suppressClick) {
    suppressClick = false;
    return;
  }
  if (target.closest("a, button, pre, code")) return;
  if (blackedOut) setBlackout(false);
  else if (!overviewOpen && !helpOpen)
    go(event.clientX < window.innerWidth / 2 ? index - 1 : index + 1);
});

root.addEventListener("pointerdown", (event) => {
  if ((event.target as HTMLElement).closest("a, button")) {
    pointerStart = null;
    return;
  }
  pointerStart = { x: event.clientX, y: event.clientY };
});

root.addEventListener("pointerup", (event) => {
  if (!pointerStart) return;
  const start = pointerStart;
  pointerStart = null;
  if (overviewOpen || helpOpen || blackedOut) return;
  const dx = event.clientX - start.x;
  const dy = event.clientY - start.y;
  if (Math.abs(dx) > 72 && Math.abs(dx) > Math.abs(dy) * 1.4) {
    suppressClick = true;
    go(index + (dx < 0 ? 1 : -1));
  }
});

root.addEventListener("pointercancel", () => {
  pointerStart = null;
});

root.addEventListener("pointerover", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLElement>(
    "[data-explanation]",
  );
  if (!target || target === explainedLine) return;
  explainedLine = target;
  lineTooltip.textContent = target.dataset.explanation ?? "";
  lineTooltip.hidden = false;
  positionExplanation(event.clientX, event.clientY);
});

root.addEventListener(
  "pointermove",
  (event) => {
    if (explainedLine) positionExplanation(event.clientX, event.clientY);
  },
  { passive: true },
);

root.addEventListener("pointerout", (event) => {
  if (!explainedLine) return;
  const next = event.relatedTarget;
  if (next instanceof Node && explainedLine.contains(next)) return;
  const nextLine =
    next instanceof Element ? next.closest("[data-explanation]") : null;
  if (nextLine === explainedLine) return;
  hideExplanation();
});

document.addEventListener(
  "keydown",
  (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const code = event.code;
    if (["ArrowRight", "PageDown", "Space", "Enter"].includes(code))
      go(index + 1);
    else if (["ArrowLeft", "PageUp", "Backspace"].includes(code)) go(index - 1);
    else if (code === "Home") go(0);
    else if (code === "End") go(slides.length - 1);
    else if (code === "BracketLeft") moveSection(-1);
    else if (code === "BracketRight") moveSection(1);
    else if (code === "KeyO") setOverview(!overviewOpen);
    else if (code === "KeyB") setBlackout(!blackedOut);
    else if (code === "KeyF") void toggleFullscreen();
    else if (code === "KeyM") void toggleMidi();
    else if (code === "Slash") setHelp(!helpOpen);
    else if (code === "Escape") {
      setOverview(false);
      setHelp(false);
      setBlackout(false);
    } else return;
    event.preventDefault();
    event.stopPropagation();
  },
  { capture: true },
);

window.addEventListener("mousemove", showControls, { passive: true });
window.addEventListener("popstate", () => {
  const fromPath = slideIndexFromPath(window.location.pathname);
  if (fromPath !== index) go(fromPath);
});
window.addEventListener("pagehide", () => launchpad.restoreStandaloneMode());

renderStaticNavigation();
render();
showControls();
