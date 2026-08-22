import { createSignal, onCleanup } from "solid-js";
import {
  PALETTES,
  DEFAULT_FONT,
  DEFAULT_TEXT_GAMMA,
  type CameraQuality,
} from "@yas-run/core";
import type { TerminalPalette } from "@yas-run/core";

export const HOST_KEY = "yas.host";
export const PALETTE_KEY = "yas.palette";
export const FONT_KEY = "yas.fontFamily";
export const FONT_SIZE_KEY = "yas.fontSize";
/** Glyph antialiasing coverage gamma — see DEFAULT_TEXT_GAMMA. */
export const TEXT_GAMMA_KEY = "yas.textGamma";
// Preferences round-trip through localStorage only. Every one of the media
// settings below is a statement
// about the machine in front of you rather than about the account — what the
// link between here and the server will carry, how much CPU the far end
// should spend on it, whether this device has speakers worth unmuting, and
// how large this screen needs the picture.  Syncing them meant a phone on
// mobile data dictating the bitrate to a desktop on the same account, and
// the desktop dictating it back on the next change.
export const AUDIO_BITRATE_KEY = "yas.audioBitrate";
export const AUDIO_MUTED_KEY = "yas.audioMuted";
export const VIDEO_BANDWIDTH_KEY = "yas.videoBandwidth";
export const VIDEO_SPEED_KEY = "yas.videoSpeed";
export const SURFACE_STREAMING_KEY = "yas.surfaceStreaming";
/** Whether decoded surface frames may be held to smooth transport jitter. */
export const SURFACE_SMOOTHING_KEY = "yas.surfaceSmoothing";
/** Per-surface source and delivery cadence ceiling. 0 means uncapped. */
export const SURFACE_MAX_FPS_KEY = "yas.surfaceMaxFps";
/** Surface zoom value, stored as an integer percentage. Its interpretation is
 *  selected by SURFACE_ZOOM_MODE_KEY. */
export const SURFACE_ZOOM_KEY = "yas.surfaceZoom";
/** "relative" multiplies display DPI; "exact" names an absolute scale. */
export const SURFACE_ZOOM_MODE_KEY = "yas.surfaceZoomMode";
export type SurfaceZoomMode = "relative" | "exact";
/** How browser touch contacts are presented to Wayland surface apps. */
export const SURFACE_TOUCH_MODE_KEY = "yas.surfaceTouchMode";
export type SurfaceTouchMode = "pointer" | "direct";
/** Whether fresh Wayland text-input enables may open the device keyboard. */
export const WAYLAND_KEYBOARD_REQUESTS_KEY = "yas.waylandKeyboardRequests";
// Codec preferences belong to the same device-local family, and for the same
// reason twice over: which codecs are on offer is a fact about this browser
// and this GPU, and the answer is not portable to the next machine on the
// account.  Both directions are stored, since a viewer receives surface video
// and sends camera and microphone.
/** Codecs accepted for surface video, as a CODEC_SUPPORT_* mask.  0 means
 *  "no opinion" — whatever the browser's decode probe found. */
export const SURFACE_CODECS_KEY = "yas.surfaceCodecs";
/** Camera upload codec. "auto" walks the browser's best-first candidates. */
export const CAMERA_CODEC_KEY = "yas.cameraCodec";
export type CameraCodecPreference = "auto" | "mjpeg" | "h264" | "av1";
/** Camera upload chroma sampling. Motion JPEG does not expose one. */
export const CAMERA_CHROMA_KEY = "yas.cameraChroma";
export type CameraChromaPreference = "auto" | "420" | "444";
/** Microphone upload codec. "auto" prefers Opus and falls back to PCM. */
export const MICROPHONE_CODEC_KEY = "yas.microphoneCodec";
export type MicrophoneCodecPreference = "auto" | "opus" | "pcm";
// Which connection a shared device goes to, and which physical devices to
// use for it. Device ids are per-browser-per-origin, so these belong to the
// device-local family for a third reason: they mean nothing anywhere else.
/** Connection id a shared camera/microphone goes to. "" = first available. */
export const MEDIA_TARGET_KEY = "yas.mediaTarget";
/** `MediaDeviceInfo.deviceId` to capture from. "" = browser default. */
export const MICROPHONE_DEVICE_KEY = "yas.microphoneDevice";
export const CAMERA_DEVICE_KEY = "yas.cameraDevice";
/** `MediaDeviceInfo.deviceId` to play remote audio on. "" = system default. */
export const SPEAKER_DEVICE_KEY = "yas.speakerDevice";
/** Camera capture height to ask the hardware for. 0 = let it choose. */
export const CAMERA_RESOLUTION_KEY = "yas.cameraResolution";
/** Camera capture cadence in fps. 0 = the codec's default. */
export const CAMERA_FRAME_RATE_KEY = "yas.cameraFrameRate";
/** How many bits the camera picture is worth. */
export const CAMERA_QUALITY_KEY = "yas.cameraQuality";
// Panel widths are UI-local for the same reason, being chrome geometry.
export const LEFT_DOCK_WIDTH_KEY = "yas.leftDockWidth";
export const PREVIEW_PANEL_WIDTH_KEY = "yas.previewPanelWidth";
/** Whether the IDE dock is open ("1"/"0"). */
export const LEFT_DOCK_OPEN_KEY = "yas.leftDockOpen";
/** Comma-separated list of collapsed dock sections. */
export const LEFT_COLLAPSED_KEY = "yas.leftCollapsed";
/** Editor soft-wrap ("1"/"0"). Persisted like the font settings — it is a
 *  reading preference, not per-machine chrome geometry. */
export const EDITOR_WRAP_KEY = "yas.editorWrap";

const BOUNDED_STORAGE_KEYS = new Set([
  PALETTE_KEY,
  FONT_KEY,
  FONT_SIZE_KEY,
  TEXT_GAMMA_KEY,
  EDITOR_WRAP_KEY,
  "yas.layouts",
]);
/** Cap variable-size preferences before retaining them in browser storage. */
export const STORAGE_VALUE_MAX_CHARS = 1024 * 1024;
export const FONT_VALUE_MAX_CHARS = 8 * 1024;

function storageValueAllowed(key: string, value: string): boolean {
  const max =
    key === "yas.layouts"
      ? STORAGE_VALUE_MAX_CHARS
      : key === FONT_KEY
        ? FONT_VALUE_MAX_CHARS
        : 256;
  return value.length <= max;
}

type StorageListener = (key: string, value: string) => void;
const listeners = new Set<StorageListener>();

export function onStorageChange(fn: StorageListener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function notifyListeners(key: string, value: string) {
  for (const fn of listeners) fn(key, value);
}

// ---------------------------------------------------------------------------
// Device-local preference storage.
// ---------------------------------------------------------------------------

function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function readStorage(key: string): string | null {
  return readLocal(key);
}

/** Forget one key, and tell this document's other frontends it is gone. */
export function clearStorage(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {
    return;
  }
  notifyListeners(key, "");
}

export function writeStorage(key: string, value: string) {
  if (BOUNDED_STORAGE_KEYS.has(key) && !storageValueAllowed(key, value)) return;
  try {
    localStorage.setItem(key, value);
  } catch {
    return;
  }
  // A document can host more than one Workspace (the embedding API does).
  // Publish locally so every frontend reacts in the same turn.
  notifyListeners(key, value);
}

// ---------------------------------------------------------------------------
// Solid primitive — subscribe to a local preference reactively.
// Must be called within a reactive owner (component or createRoot).
// ---------------------------------------------------------------------------

export function useStoredValue(key: string): () => string | null {
  const [value, setValue] = createSignal(readStorage(key));
  const unsub = onStorageChange((k) => {
    if (k === key) setValue(readStorage(key));
  });
  onCleanup(unsub);
  return value;
}

// ---------------------------------------------------------------------------
// Derived helpers
// ---------------------------------------------------------------------------

export function yasHost(): string {
  return readStorage(HOST_KEY) || location.hostname;
}

const edgeHost =
  (import.meta.env.VITE_YAS_EDGE as string | undefined) ?? location.host;

export const basePath = location.pathname.endsWith("/")
  ? location.pathname
  : location.pathname + "/";

export function wsUrl(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return proto + "//" + edgeHost + location.pathname;
}

// ---------------------------------------------------------------------------
// Appearance preferences: stored key, then the default.
// ---------------------------------------------------------------------------

function parsePaletteId(raw: string | null): TerminalPalette | null {
  return (raw && PALETTES.find((x) => x.id === raw)) || null;
}

function parseFontSize(raw: string | null): number | null {
  if (!raw) return null;
  const n = parseInt(raw, 10);
  return n > 0 ? n : null;
}

function parseFontFamily(raw: string | null): string | null {
  const family = raw?.trim() || null;
  return family && family.length <= FONT_VALUE_MAX_CHARS ? family : null;
}

function parseTextGamma(raw: string | null): number | null {
  if (!raw) return null;
  const n = Number(raw);
  // Past ~2.5 the thinning eats stems outright, so refuse to render
  // unreadably; below 1 it fattens, which is a legitimate light-theme want.
  return Number.isFinite(n) && n >= 0.5 && n <= 2.5 ? n : null;
}

export function preferredPalette(): TerminalPalette {
  return parsePaletteId(readStorage(PALETTE_KEY)) ?? PALETTES[0];
}

export function preferredFontSize(): number {
  return parseFontSize(readStorage(FONT_SIZE_KEY)) ?? 13;
}

/** Preferred glyph coverage gamma. See DEFAULT_TEXT_GAMMA. */
export function preferredTextGamma(): number {
  return parseTextGamma(readStorage(TEXT_GAMMA_KEY)) ?? DEFAULT_TEXT_GAMMA;
}

/**
 * The stack to use when the visitor has expressed no font preference.
 *
 * `DEFAULT_FONT` is deliberately `ui-monospace, monospace`: the app is
 * served by a yas server that ships no webfont, so the right answer there
 * is whatever the platform calls its terminal face. A host that *does* ship
 * one — yas.run self-hosts JetBrains Mono for the whole site — wants the
 * embedded workspace on the same face as the page around it, and saying so
 * from the host beats hardcoding a webfont into a client that usually has
 * no way to fetch it.
 *
 * Page-level like the shell capabilities, and for the same reason: it
 * describes the document, not a component instance, and is set once before
 * mount. A stored choice still wins over it — this replaces the fallback, not
 * the preference.
 */
let pageDefaultFont = DEFAULT_FONT;

export function setDefaultFont(family: string): void {
  pageDefaultFont = family.trim() || DEFAULT_FONT;
}

export function defaultFont(): string {
  return pageDefaultFont;
}

export function preferredFont(): string {
  return parseFontFamily(readStorage(FONT_KEY)) ?? pageDefaultFont;
}

/** Preferred audio muted state. Defaults to true (browser autoplay policy). */
export function preferredAudioMuted(): boolean {
  const s = readStorage(AUDIO_MUTED_KEY);
  if (s === "0") return false;
  // Default to muted — browsers require a user gesture before audio can play.
  return true;
}

/** Preferred audio bitrate in kbps. 0 = server default. */
export function preferredAudioBitrate(): number {
  const s = readStorage(AUDIO_BITRATE_KEY);
  if (s) {
    const n = parseInt(s, 10);
    if (n >= 0) return n;
  }
  return 0;
}

/** Preferred video bandwidth.  0 = server default, 1–4 = presets,
 *  10–255 = custom AV1 quantizer. */
export function preferredVideoBandwidth(): number {
  return readWireByte(VIDEO_BANDWIDTH_KEY);
}

/** Preferred encoder speed.  0 = server default, 1–4 = presets,
 *  10–255 = custom (10 = slowest, 255 = fastest). */
export function preferredVideoSpeed(): number {
  return readWireByte(VIDEO_SPEED_KEY);
}

function readWireByte(key: string): number {
  const s = readStorage(key);
  if (s) {
    const n = parseInt(s, 10);
    if (n >= 0 && n <= 255) return n;
  }
  return 0;
}

/** Preferred surface streaming state.  Defaults to enabled. */
export function preferredSurfaceStreaming(): boolean {
  const s = readStorage(SURFACE_STREAMING_KEY);
  if (s === "0") return false;
  return true;
}

/** Prefer interaction latency over cadence smoothing unless explicitly set. */
export function preferredSurfaceSmoothing(): boolean {
  return readStorage(SURFACE_SMOOTHING_KEY) === "1";
}

/** Bounds for the custom frame-rate control. The wire supports u16, but a
 *  four-digit cap already exceeds practical displays and keeps the UI useful. */
export const MIN_SURFACE_MAX_FPS = 1;
export const MAX_SURFACE_MAX_FPS = 1000;

/** Preferred surface frame-rate ceiling. 0 = disabled/display cadence. */
export function preferredSurfaceMaxFps(): number {
  const n = parseInt(readStorage(SURFACE_MAX_FPS_KEY) ?? "", 10);
  if (
    !Number.isFinite(n) ||
    n < MIN_SURFACE_MAX_FPS ||
    n > MAX_SURFACE_MAX_FPS
  ) {
    return 0;
  }
  return n;
}

/** Zoom bounds, in percent.  Matched by `clampZoom` in the surface view —
 *  the floor keeps the app's logical size layoutable, the ceiling keeps one
 *  pane from dictating a scale every co-viewer then has to stream. */
export const MIN_SURFACE_ZOOM = 25;
export const MAX_SURFACE_ZOOM = 400;

/** Preferred surface zoom value in percent. Defaults to 100. */
export function preferredSurfaceZoom(): number {
  const n = parseInt(readStorage(SURFACE_ZOOM_KEY) ?? "", 10);
  if (!Number.isFinite(n)) return 100;
  return Math.min(MAX_SURFACE_ZOOM, Math.max(MIN_SURFACE_ZOOM, n));
}

/** How the surface zoom value is interpreted. An absent mode is relative. */
export function preferredSurfaceZoomMode(): SurfaceZoomMode {
  return readStorage(SURFACE_ZOOM_MODE_KEY) === "exact" ? "exact" : "relative";
}

/** Direct contacts are the default; pointer gestures are the compatibility
 *  opt-out for apps which do not handle native touch as desired. */
export function preferredSurfaceTouchMode(): SurfaceTouchMode {
  return readStorage(SURFACE_TOUCH_MODE_KEY) === "pointer"
    ? "pointer"
    : "direct";
}

/** Codecs this device accepts for surface video, as a CODEC_SUPPORT_* mask.
 *  0 means "no opinion": take whatever the decode probe found. */
export function preferredSurfaceCodecs(): number {
  return readWireByte(SURFACE_CODECS_KEY);
}

/** Camera upload codec. Unknown values read as "auto" rather than failing —
 *  a preference written by a newer build must not wedge an older one. */
export function preferredCameraCodec(): CameraCodecPreference {
  const value = readStorage(CAMERA_CODEC_KEY);
  return value === "mjpeg" || value === "h264" || value === "av1"
    ? value
    : "auto";
}

/** Camera upload chroma sampling. */
export function preferredCameraChroma(): CameraChromaPreference {
  const value = readStorage(CAMERA_CHROMA_KEY);
  return value === "420" || value === "444" ? value : "auto";
}

/** Microphone upload codec. */
export function preferredMicrophoneCodec(): MicrophoneCodecPreference {
  const value = readStorage(MICROPHONE_CODEC_KEY);
  return value === "opus" || value === "pcm" ? value : "auto";
}

/** Connection a shared camera/microphone goes to. "" = first available. */
export function preferredMediaTarget(): string {
  return readStorage(MEDIA_TARGET_KEY) ?? "";
}

/** Chosen capture/playback devices. "" = whatever the browser picks. */
export function preferredMicrophoneDevice(): string {
  return readStorage(MICROPHONE_DEVICE_KEY) ?? "";
}
export function preferredCameraDevice(): string {
  return readStorage(CAMERA_DEVICE_KEY) ?? "";
}
export function preferredSpeakerDevice(): string {
  return readStorage(SPEAKER_DEVICE_KEY) ?? "";
}

/** Camera capture height in pixels, or 0 for "whatever the camera offers".
 *  Stored as a height alone: aspect ratio is the camera's to decide, and
 *  every standard mode is named by its height anyway. */
export function preferredCameraResolution(): number {
  const value = parseInt(readStorage(CAMERA_RESOLUTION_KEY) ?? "", 10);
  return value === 360 || value === 480 || value === 720 || value === 1080
    ? value
    : 0;
}

/** Camera capture cadence in fps, or 0 for the codec's own default. */
export function preferredCameraFrameRate(): number {
  const value = parseInt(readStorage(CAMERA_FRAME_RATE_KEY) ?? "", 10);
  return value === 15 || value === 24 || value === 30 || value === 60
    ? value
    : 0;
}

/** Camera picture quality. */
export function preferredCameraQuality(): CameraQuality {
  const value = readStorage(CAMERA_QUALITY_KEY);
  return value === "low" || value === "high" ? value : "balanced";
}

/** Honor fresh Wayland text-input keyboard requests unless explicitly
 *  disabled on this device. */
export function preferredWaylandKeyboardRequests(): boolean {
  return readStorage(WAYLAND_KEYBOARD_REQUESTS_KEY) !== "0";
}

/** The narrowest the right dock can be dragged. Wide enough for a card's
 *  header row (grip target, truncated title, ✕) and a legible thumbnail
 *  strip; the left dock keeps its own larger floor — its panels are trees
 *  and lists that stop working well far sooner. */
export const MIN_PREVIEW_PANEL_WIDTH = 80;

function preferredWidth(key: string, fallback: number, min = 160): number {
  const n = parseInt(readStorage(key) ?? "", 10);
  return Number.isFinite(n) && n >= min ? n : fallback;
}

export function preferredLeftDockWidth(): number {
  return preferredWidth(LEFT_DOCK_WIDTH_KEY, 260);
}

export function preferredPreviewPanelWidth(): number {
  return preferredWidth(PREVIEW_PANEL_WIDTH_KEY, 160, MIN_PREVIEW_PANEL_WIDTH);
}

/** Whether the IDE dock is open. A stored choice wins either way; first
 *  run opens it wherever the viewport can afford the width — the dock is
 *  the workspace's front door, and arriving at a bare terminal hid the
 *  files/log/problems surface behind a shortcut nobody has learned yet.
 *  On a phone it would bury the terminal instead, so it starts closed
 *  there. */
export function preferredLeftDockOpen(): boolean {
  const raw = readStorage(LEFT_DOCK_OPEN_KEY);
  if (raw != null) return raw === "1";
  return typeof window !== "undefined" && window.innerWidth >= 768;
}

type LeftSection = "explorer" | "log" | "problems";

/**
 * The set of collapsed dock sections, persisted as a comma list.
 *
 * Absent (first run) collapses Problems, so Files — with its folded-in
 * changes — shows on its own. Commit Log is deliberately *not* in that list
 * even though it starts folded in practice: it folds because the root is not
 * a repository (`noRepo`, see dockSections), and that is a different
 * statement. A user collapse is a preference and outranks the auto-unfold, so
 * seeding one here left the log folded on entering a repo — permanently, for
 * anyone who never thought to click a header they had never seen open.
 */
export function preferredCollapsedSections(): LeftSection[] {
  const raw = readStorage(LEFT_COLLAPSED_KEY);
  if (raw == null) return ["problems"];
  // An id missing here is silently dropped on every reload, so the
  // section would come back expanded forever.
  const valid = new Set(["explorer", "log", "problems"]);
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter((p): p is LeftSection => valid.has(p));
}
