import { createSignal, createEffect, onCleanup } from "solid-js";
import {
  YAS_FONT_FAMILY_MONOSPACE,
  YAS_FONT_FACE_FETCHABLE,
  YAS_FONT_STYLE_ITALIC,
  YAS_FONT_STYLE_OBLIQUE,
  yasFontHashHex,
} from "@yas-run/core";
import type {
  YasFontDescription,
  YasFontFaceData,
  YasFontFamily,
} from "@yas-run/core";
import {
  boundedFontList,
  FONT_LIST_MAX_FAMILIES,
  FONT_LIST_MAX_FAMILY_CHARS,
  FONT_LIST_MAX_TOTAL_CHARS,
  FONT_STORE_BUDGET_BYTES,
  forgetFontFace,
  loadFontFace,
  saveFontFace,
} from "./fontStore";

export const FONT_STACK_MAX_FAMILIES = 16;
export const FONT_STACK_MAX_CHARS = 8 * 1024;
export const FONT_RUNTIME_MAX_FACES = 16;
export const FONT_RUNTIME_MAX_FACE_BYTES = FONT_STORE_BUDGET_BYTES;
export const FONT_RUNTIME_MAX_TOTAL_FACE_BYTES = 128 * 1024 * 1024;

/** The small YasConnection surface the font UI needs. Keeping this
 * structural avoids coupling the UI to the concrete connection class. */
export interface FontProtocolConnection {
  listFonts(): Promise<readonly FontFamilySummary[]>;
  describeFont(family: string): Promise<FontDescription>;
  fetchFont(contentHash: Uint8Array): Promise<FontFaceData>;
}

export type FontFamilySummary = Pick<
  YasFontFamily,
  "flags" | "faceCount" | "family" | "display"
>;
export type FontDescription = YasFontDescription;
export type FontFaceData = YasFontFaceData;

/** Reactive identity and negotiated capability of the active server. */
export interface FontProtocolSource {
  key: string;
  connected: boolean;
  connection: FontProtocolConnection | null;
  /** BLAKE3 implementation supplied by the browser runtime. Global cache
   * reuse is disabled until exact bytes can be verified locally. */
  hashFont?: (data: Uint8Array) => Uint8Array | Promise<Uint8Array>;
}

const fontConnectionIdentities = new WeakMap<object, number>();
let nextFontConnectionIdentity = 0;

/** A route can replace its YasConnection while retaining the same UI id and
 * restarting its per-instance generation at zero. Include object identity so
 * late catalogue/face replies from the removed server cannot be accepted for
 * its replacement. */
export function fontProtocolSourceKey(
  id: string,
  generation: number,
  connection: object,
): string {
  let identity = fontConnectionIdentities.get(connection);
  if (identity === undefined) {
    identity = ++nextFontConnectionIdentity;
    fontConnectionIdentities.set(connection, identity);
  }
  return `${id}:${generation}:${identity}`;
}

/** Terminal picker keys from a LIST response. The protocol's `family` is the
 * opaque DESCRIBE key; `display` is only a label and cannot replace it. */
export function protocolFontFamilies(
  families: readonly FontFamilySummary[],
): string[] {
  const monospace: string[] = [];
  let chars = 0;
  for (const family of families) {
    if ((family.flags & YAS_FONT_FAMILY_MONOSPACE) === 0) continue;
    const name = family.family.trim();
    if (
      name.length === 0 ||
      name.length > FONT_LIST_MAX_FAMILY_CHARS ||
      chars + name.length > FONT_LIST_MAX_TOTAL_CHARS
    )
      continue;
    monospace.push(name);
    chars += name.length;
    if (monospace.length >= FONT_LIST_MAX_FAMILIES) break;
  }
  return boundedFontList(monospace).sort((left, right) =>
    left.localeCompare(right),
  );
}

const CSS_GENERIC = new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "system-ui",
  "ui-serif",
  "ui-sans-serif",
  "ui-monospace",
  "ui-rounded",
  "math",
  "emoji",
  "fangsong",
]);

function splitFontFamilies(value: string): string[] {
  if (value.length > FONT_STACK_MAX_CHARS) return [];
  return value
    .split(",")
    .map((f) => f.trim().replace(/^['"]|['"]$/g, ""))
    .filter(
      (family) =>
        family.length > 0 && family.length <= FONT_LIST_MAX_FAMILY_CHARS,
    )
    .slice(0, FONT_STACK_MAX_FAMILIES);
}

/**
 * The one family a font stack is *about*, for naming it to the user.
 *
 * Everything after the first entry is a fallback, and the generics at the tail
 * are there so text renders at all — "JetBrains Mono, ui-monospace, monospace"
 * is the JetBrains Mono choice. A stack of nothing but generics (the default)
 * has no choice behind it, so its own first entry is the honest answer.
 */
export function primaryFontFamily(stack: string): string {
  const families = splitFontFamilies(stack);
  return (
    families.find((family) => !CSS_GENERIC.has(family.toLowerCase())) ??
    families[0] ??
    ""
  );
}

function protocolFaceStyle(style: number): FontFaceDescriptors["style"] {
  if (style === YAS_FONT_STYLE_ITALIC) return "italic";
  if (style === YAS_FONT_STYLE_OBLIQUE) return "oblique";
  return "normal";
}

/** The regular upright face is the terminal's cell metric authority. Fall
 * back to the closest fixed-width face when a family has no exact regular. */
export function fontAdvanceRatio(
  description: FontDescription,
): number | undefined {
  const candidates = description.faces.filter(
    (face) => face.unitsPerEm > 0 && face.cellAdvance > 0,
  );
  candidates.sort(
    (a, b) =>
      Number(a.style !== 0) - Number(b.style !== 0) ||
      Math.abs(a.weightDefault - 400) - Math.abs(b.weightDefault - 400),
  );
  const face = candidates[0];
  if (!face) return undefined;
  const ratio = face.cellAdvance / face.unitsPerEm;
  return Number.isFinite(ratio) && ratio > 0 ? ratio : undefined;
}

function sameHash(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.length === right.length &&
    left.every((byte, index) => byte === right[index])
  );
}

async function loadProtocolFace(
  source: FontProtocolSource,
  family: string,
  face: FontDescription["faces"][number],
): Promise<FontFace | null> {
  if (!source.connection)
    throw new Error("Font protocol connection is unavailable");
  if ((face.flags & YAS_FONT_FACE_FETCHABLE) === 0) return null;
  if (
    face.byteLength <= 0n ||
    face.byteLength > BigInt(FONT_RUNTIME_MAX_FACE_BYTES)
  ) {
    return null;
  }
  if (
    typeof FontFace !== "function" ||
    typeof document.fonts?.add !== "function"
  ) {
    return null;
  }

  const hash = yasFontHashHex(face.contentHash);
  let data: Uint8Array | undefined;
  if (source.hashFont) {
    const stored = await loadFontFace(hash);
    if (stored) {
      if (BigInt(stored.data.byteLength) !== face.byteLength) {
        void forgetFontFace(hash);
      } else {
        try {
          const actual = await source.hashFont(stored.data);
          if (sameHash(actual, face.contentHash)) data = stored.data;
          else void forgetFontFace(hash);
        } catch {
          void forgetFontFace(hash);
        }
      }
    }
  }
  if (!data) {
    const fetched = await source.connection.fetchFont(face.contentHash);
    if (
      !sameHash(fetched.contentHash, face.contentHash) ||
      fetched.format !== face.format ||
      BigInt(fetched.data.byteLength) !== face.byteLength
    ) {
      throw new Error("font face response does not match its description");
    }
    data = fetched.data.slice();
    if (source.hashFont) {
      const actual = await source.hashFont(data);
      if (!sameHash(actual, face.contentHash)) {
        throw new Error("font face bytes do not match their BLAKE3 hash");
      }
      void saveFontFace(hash, data);
    }
  }

  const fontBytes = new ArrayBuffer(data.byteLength);
  new Uint8Array(fontBytes).set(data);
  const loaded = new FontFace(family, fontBytes, {
    style: protocolFaceStyle(face.style),
    weight: String(face.weightDefault),
  });
  await loaded.load();
  return loaded;
}

/**
 * Reactive font loader. Given a font accessor, resolves native YAS FONT
 * records and waits for browser font readiness. A server without FONT uses
 * only faces already supplied by the page; there is no HTTP protocol fallback.
 *
 * Returns reactive accessors for the resolved font family, loading state,
 * and advance ratio (if the server provides metrics).
 */
export function createFontLoader(
  font: () => string,
  defaultFont: string,
  protocolSource?: () => FontProtocolSource | null,
): {
  resolvedFont: () => string;
  fontLoading: () => boolean;
  advanceRatio: () => number | undefined;
} {
  const [resolvedFont, setResolvedFont] = createSignal(font());
  const [fontLoading, setFontLoading] = createSignal(false);
  const [advanceRatio, setAdvanceRatio] = createSignal<number | undefined>(
    undefined,
  );

  let requestVersion = 0;
  createEffect(() => {
    const requestedFont = font().trim() || defaultFont;
    const source = protocolSource?.() ?? null;
    const families = splitFontFamilies(requestedFont).filter(
      (family) => !CSS_GENERIC.has(family.toLowerCase()),
    );
    const version = ++requestVersion;
    let cancelled = false;

    if (families.length === 0) {
      setResolvedFont(requestedFont);
      setAdvanceRatio(undefined);
      setFontLoading(false);
      onCleanup(() => {
        cancelled = true;
      });
      return;
    }

    setFontLoading(true);
    setAdvanceRatio(undefined);

    // A supplied connection that has not completed HELLO is not evidence that
    // the server lacks FONT. Wait for capability negotiation instead of racing
    // an obsolete HTTP route against it.
    if (protocolSource && (!source || !source.connected)) {
      setResolvedFont(requestedFont);
      setFontLoading(false);
      onCleanup(() => {
        cancelled = true;
      });
      return;
    }

    const useProtocol = source?.connection !== null && source !== null;

    const ownedFaces: FontFace[] = [];

    const load = async () => {
      let ratio: number | undefined;

      if (useProtocol && source?.connection) {
        let loadedFaceCount = 0;
        let loadedFaceBytes = 0;
        for (const family of families) {
          if (cancelled || version !== requestVersion) return;
          let description: FontDescription;
          try {
            description = await source.connection.describeFont(family);
          } catch {
            continue;
          }
          if (cancelled || version !== requestVersion) return;
          ratio ??= fontAdvanceRatio(description);

          for (const face of description.faces) {
            if (cancelled || version !== requestVersion) return;
            if (
              loadedFaceCount >= FONT_RUNTIME_MAX_FACES ||
              loadedFaceBytes >= FONT_RUNTIME_MAX_TOTAL_FACE_BYTES
            ) {
              break;
            }
            if ((face.flags & YAS_FONT_FACE_FETCHABLE) === 0) continue;
            const faceBytes = Number(face.byteLength);
            if (
              !Number.isSafeInteger(faceBytes) ||
              faceBytes <= 0 ||
              faceBytes > FONT_RUNTIME_MAX_FACE_BYTES ||
              loadedFaceCount >= FONT_RUNTIME_MAX_FACES ||
              loadedFaceBytes + faceBytes > FONT_RUNTIME_MAX_TOTAL_FACE_BYTES
            ) {
              continue;
            }
            loadedFaceCount++;
            loadedFaceBytes += faceBytes;
            try {
              const loaded = await loadProtocolFace(source, family, face);
              if (!loaded || cancelled || version !== requestVersion) continue;
              document.fonts.add(loaded);
              ownedFaces.push(loaded);
            } catch {
              // A family can still render from another face (or the next
              // family in its stack); one corrupt/restricted face is local.
            }
          }

          try {
            if (typeof document.fonts?.load === "function") {
              await document.fonts.load(`16px "${family}"`, "BESbswy");
            } else if (document.fonts?.ready) {
              await document.fonts.ready;
            }
          } catch {}
        }

        if (cancelled || version !== requestVersion) return;
        setAdvanceRatio(ratio);
        setResolvedFont(requestedFont);
        setFontLoading(false);
        return;
      }

      // A page or embedder may provide its own @font-face. Without the native
      // FONT family, asking the browser to resolve that local stack is the
      // complete fallback; server HTTP routes are not part of YAS.
      for (const family of families) {
        if (cancelled || version !== requestVersion) return;

        const loadSpec = `16px "${family}"`;
        try {
          if (typeof document.fonts?.load === "function") {
            await document.fonts.load(loadSpec, "BESbswy");
          } else if (document.fonts?.ready) {
            await document.fonts.ready;
          }
        } catch {}
      }

      if (cancelled || version !== requestVersion) return;
      setAdvanceRatio(ratio);
      setResolvedFont(requestedFont);
      setFontLoading(false);
    };

    void load();
    onCleanup(() => {
      cancelled = true;
      for (const face of ownedFaces) document.fonts?.delete(face);
    });
  });

  return { resolvedFont, fontLoading, advanceRatio };
}
