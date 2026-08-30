import { marked } from "marked";

const SECTION_META = [
  { id: "01-intro", name: "Premise", color: "#d7ff49", midi: 17 },
  { id: "02-protocol", name: "Protocol", color: "#52f2c2", midi: 21 },
  { id: "03-terminal", name: "Terminal", color: "#48c7ff", midi: 37 },
  { id: "04-wayland", name: "Wayland", color: "#7d8cff", midi: 45 },
  { id: "05-desktop", name: "Desktop", color: "#c08bff", midi: 53 },
  { id: "06-workspace", name: "Workspace", color: "#ff70bf", midi: 57 },
  { id: "07-execution", name: "Execution", color: "#ff805f", midi: 5 },
  { id: "08-delivery", name: "Delivery", color: "#ffc247", midi: 13 },
] as const;

export interface Slide {
  id: string;
  path: string;
  title: string;
  layout: string;
  sectionId: string;
  sectionName: string;
  sectionIndex: number;
  sectionSlideIndex: number;
  color: string;
  midiColor: number;
  markdown: string;
  html: string;
}

export interface Section {
  id: string;
  name: string;
  color: string;
  midiColor: number;
  index: number;
  start: number;
  count: number;
}

interface Frontmatter {
  [key: string]: string;
}

const rawSlides = import.meta.glob<string>("../slides/*/*.md", {
  eager: true,
  query: "?raw",
  import: "default",
});

function frontmatter(raw: string): {
  attributes: Frontmatter;
  markdown: string;
} {
  if (!raw.startsWith("---\n")) return { attributes: {}, markdown: raw };
  const end = raw.indexOf("\n---\n", 4);
  if (end < 0) return { attributes: {}, markdown: raw };
  const attributes: Frontmatter = {};
  for (const line of raw.slice(4, end).split("\n")) {
    const colon = line.indexOf(":");
    if (colon < 1) continue;
    attributes[line.slice(0, colon).trim()] = line
      .slice(colon + 1)
      .trim()
      .replace(/^['"]|['"]$/g, "");
  }
  return { attributes, markdown: raw.slice(end + 5).trim() };
}

function titleOf(markdown: string): string {
  return markdown.match(/^#\s+(.+)$/m)?.[1]?.replace(/[`*_]/g, "") ?? "Untitled";
}

export const slides: Slide[] = Object.entries(rawSlides)
  .sort(([left], [right]) => left.localeCompare(right))
  .map(([path, raw]) => {
    const match = path.match(/\.\.\/slides\/([^/]+)\/([^/]+)\.md$/);
    if (!match) throw new Error(`Unexpected slide path: ${path}`);
    const [, sectionId, file] = match;
    const sectionIndex = SECTION_META.findIndex((item) => item.id === sectionId);
    if (sectionIndex < 0) throw new Error(`Unknown section: ${sectionId}`);
    const section = SECTION_META[sectionIndex];
    const { attributes, markdown } = frontmatter(raw);
    return {
      id: `${sectionId}/${file}`,
      path,
      title: titleOf(markdown),
      layout: attributes.layout ?? "default",
      sectionId,
      sectionName: section.name,
      sectionIndex,
      sectionSlideIndex: 0,
      color: section.color,
      midiColor: section.midi,
      markdown,
      html: marked.parse(markdown) as string,
    };
  });

export const sections: Section[] = SECTION_META.map((meta, index) => {
  const matching = slides.filter((slide) => slide.sectionId === meta.id);
  const start = slides.findIndex((slide) => slide.sectionId === meta.id);
  matching.forEach((slide, sectionSlideIndex) => {
    slide.sectionSlideIndex = sectionSlideIndex;
  });
  return {
    id: meta.id,
    name: meta.name,
    color: meta.color,
    midiColor: meta.midi,
    index,
    start,
    count: matching.length,
  };
});

export function slideIndexFromPath(pathname: string): number {
  let id: string;
  try {
    id = decodeURIComponent(pathname.replace(/^\/+|\/+$/g, ""));
  } catch {
    return 0;
  }
  if (!id) return 0;
  const found = slides.findIndex((slide) => slide.id === id);
  if (found >= 0) return found;
  const number = Number.parseInt(id, 10);
  return Number.isFinite(number) ? Math.max(0, Math.min(slides.length - 1, number - 1)) : 0;
}
