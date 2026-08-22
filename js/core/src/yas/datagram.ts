/** Static validation for frames carried on an optional unreliable YAS path. */

import {
  YAS_CLASS_EVENT,
  YAS_DATAGRAM_FORBIDDEN,
  YAS_DATAGRAM_MEDIA_FRAME,
  YAS_DATAGRAM_NET_NATIVE_FLOW,
  YAS_DATAGRAM_SURFACE_FRAME,
  YAS_MEDIA_FRAME_CODEC_CONFIG,
  YAS_MEDIA_FRAME_DISCARDABLE,
  YAS_MEDIA_FRAME_END_OF_STREAM,
  YAS_MEDIA_FRAME_KEYFRAME,
  YAS_SURFACE_FRAME_CODEC_CONFIG,
  YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE,
  YAS_SURFACE_FRAME_DISCARDABLE,
  YAS_SURFACE_FRAME_END_OF_STREAM,
  YAS_SURFACE_FRAME_KEYFRAME,
} from "./generated";
import { decodeMediaFrame, type YasMediaFrame } from "./media";
import { decodeNetDatagram, type YasNetDatagram } from "./net";
import { decodeSurfaceFrame, type YasSurfaceFrame } from "./surface";
import { YasProtocolError, yasOperationPolicy, type YasFrame } from "./wire";

export type YasValidatedDatagram =
  | {
      predicate: typeof YAS_DATAGRAM_NET_NATIVE_FLOW;
      value: YasNetDatagram;
    }
  | {
      predicate: typeof YAS_DATAGRAM_SURFACE_FRAME;
      value: YasSurfaceFrame;
    }
  | {
      predicate: typeof YAS_DATAGRAM_MEDIA_FRAME;
      value: YasMediaFrame;
    };

/**
 * Validate the operation-independent and generated family predicate for one
 * optional-path datagram. NET_NATIVE_FLOW callers must additionally verify
 * that `value.flowHandle` resolved to a flow selecting NATIVE_DATAGRAM before
 * dispatch. This helper deliberately does not add an unreliable path to the
 * WebSocket or Relay transports, whose HELLO datagram limit remains zero.
 */
export function validateYasDatagramFrame(
  frame: YasFrame,
): YasValidatedDatagram {
  if (frame.class !== YAS_CLASS_EVENT)
    throw new YasProtocolError("YAS transport datagrams must contain Events");
  if (frame.compressed)
    throw new YasProtocolError("compressed YAS transport datagram");

  const predicate =
    yasOperationPolicy(frame.family, frame.class, frame.kind)?.datagram ??
    YAS_DATAGRAM_FORBIDDEN;
  if (predicate === YAS_DATAGRAM_FORBIDDEN)
    throw new YasProtocolError("YAS operation is forbidden on datagrams");

  if (predicate === YAS_DATAGRAM_NET_NATIVE_FLOW) {
    return { predicate, value: decodeNetDatagram(frame.payload) };
  }
  if (predicate === YAS_DATAGRAM_SURFACE_FRAME) {
    const value = decodeSurfaceFrame(frame.payload);
    const required =
      YAS_SURFACE_FRAME_DATAGRAM_ELIGIBLE | YAS_SURFACE_FRAME_DISCARDABLE;
    const reliableOnly =
      YAS_SURFACE_FRAME_KEYFRAME |
      YAS_SURFACE_FRAME_CODEC_CONFIG |
      YAS_SURFACE_FRAME_END_OF_STREAM;
    if ((value.flags & required) !== required || value.flags & reliableOnly)
      throw new YasProtocolError("Surface FRAME is not datagram-safe");
    return { predicate, value };
  }
  if (predicate === YAS_DATAGRAM_MEDIA_FRAME) {
    const value = decodeMediaFrame(frame.payload);
    const reliableOnly =
      YAS_MEDIA_FRAME_KEYFRAME |
      YAS_MEDIA_FRAME_CODEC_CONFIG |
      YAS_MEDIA_FRAME_END_OF_STREAM;
    if (
      !(value.flags & YAS_MEDIA_FRAME_DISCARDABLE) ||
      value.flags & reliableOnly
    )
      throw new YasProtocolError("Media FRAME is not datagram-safe");
    return { predicate, value };
  }
  throw new YasProtocolError("unknown YAS datagram predicate");
}
