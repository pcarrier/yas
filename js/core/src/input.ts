/** Browser-facing input vocabulary. Native YAS clients translate these UI
 * events to their typed Terminal and Surface family records. */

export const MOUSE_DOWN = 0;
export const MOUSE_UP = 1;
export const MOUSE_MOVE = 2;

export const SURFACE_POINTER_DOWN = 0;
export const SURFACE_POINTER_UP = 1;
export const SURFACE_POINTER_MOVE = 2;
export const SURFACE_POINTER_LEAVE = 3;

export const AXIS_SOURCE_WHEEL = 0;
export const AXIS_SOURCE_FINGER = 1;
export const AXIS_SOURCE_CONTINUOUS = 2;

export const SURFACE_TOUCH_DOWN = 0;
export const SURFACE_TOUCH_UP = 1;
export const SURFACE_TOUCH_MOTION = 2;
export const SURFACE_TOUCH_CANCEL = 3;

/** One dropped payload: its MIME type, optional source filename, and bytes. */
export interface SurfaceDragItem {
  mime: string;
  name: string;
  data: Uint8Array;
}

/** One browser scroll event before native Surface-family encoding. */
export interface SurfaceAxisEvent {
  /** Horizontal distance in composited-frame pixels, positive = right. */
  dx: number;
  /** Vertical distance in composited-frame pixels, positive = down. */
  dy: number;
  /** Horizontal wheel travel in 120ths of a detent. */
  v120x: number;
  /** Vertical wheel travel in 120ths of a detent. */
  v120y: number;
  /** An AXIS_SOURCE_* value, or null when the device is unclassified. */
  source: number | null;
  /** True when this ends the scroll sequence; deltas are ignored. */
  stop: boolean;
  /** The browser event's own timestamp in milliseconds. */
  timeMs?: number;
}

export interface SurfaceTouchPoint {
  identifier: number;
  /** Horizontal position in the composited frame's pixel space. */
  x: number;
  /** Vertical position in the composited frame's pixel space. */
  y: number;
}

/** UI admission policy for a native Client DISCONNECT reason. */
export const CLIENT_DISCONNECT_REASON_MAX_BYTES = 1024;

const inputTextEncoder = new TextEncoder();

export function clientDisconnectReasonByteLength(reason: string): number {
  return inputTextEncoder.encode(reason).length;
}
