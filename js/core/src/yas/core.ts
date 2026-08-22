import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_MAX_DECODED_FRAME,
  YAS_MAX_BUFFERED,
  YAS_MAX_DATAGRAM,
  YAS_MAX_WIRE_FRAME,
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  validateExtensionBody,
  type YasExtension,
} from "./wire";
import {
  YAS_CORE_CANCEL,
  YAS_CORE_CLIENT_UPDATE,
  YAS_CORE_DIRECTION_ACCEPTS,
  YAS_CORE_DIRECTION_SENDS,
  YAS_CORE_FAMILY_UPDATE,
  YAS_CORE_GOAWAY,
  YAS_CORE_HELLO,
  YAS_CORE_PING,
  YAS_CORE_RUNTIME_AVAILABLE,
  YAS_CORE_RUNTIME_DEGRADED,
  YAS_CORE_RUNTIME_UNAVAILABLE,
  YAS_CORE_SESSION_INFO,
  YAS_CORE_SESSION_UPDATE,
  YAS_CORE_SHUTDOWN,
  YAS_CORE_VERSION,
  YAS_CORE_SERVER_HELLO_INITIAL_WATCH_RESULTS_EXTENSION,
  YAS_CORE_SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION,
  YAS_CORE_SERVER_HELLO_PLATFORM_EXTENSION,
  YAS_CORRELATED_HEADER_BYTES,
  YAS_EVENTS_LIMIT_MAX_RING_BYTES,
  YAS_EVENTS_LIMIT_MIN_RING_BYTES,
  YAS_EVENT_HEADER_BYTES,
  YAS_FAMILY_DEPENDENCIES,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_LIMIT_POLICIES,
  YAS_FAMILY_CORE,
  YAS_FAMILY_KV,
  YAS_FAMILY_RELAY,
  YAS_KV_LIMIT_MAX_INLINE_BYTES,
  YAS_KV_LIMIT_MAX_VALUE_BYTES,
  YAS_OPERATION_DIRECTION_MASKS,
  YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION,
  YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
} from "./generated";

export {
  YAS_CORE_CANCEL,
  YAS_CORE_CLIENT_UPDATE,
  YAS_CORE_FAMILY_UPDATE,
  YAS_CORE_GOAWAY,
  YAS_CORE_HELLO,
  YAS_CORE_PING,
  YAS_CORE_SESSION_INFO,
  YAS_CORE_SESSION_UPDATE,
  YAS_CORE_SHUTDOWN,
  YAS_CORE_VERSION,
  YAS_FAMILY_CORE,
  YAS_FAMILY_FONT,
  YAS_FAMILY_RELAY,
  YAS_FAMILY_TRANSFER,
} from "./generated";

export const YAS_RUNTIME_AVAILABLE = YAS_CORE_RUNTIME_AVAILABLE;
export const YAS_RUNTIME_DEGRADED = YAS_CORE_RUNTIME_DEGRADED;
export const YAS_RUNTIME_UNAVAILABLE = YAS_CORE_RUNTIME_UNAVAILABLE;
export const YAS_DIRECTION_SERVER_ACCEPTS = YAS_CORE_DIRECTION_ACCEPTS;
export const YAS_DIRECTION_SERVER_SENDS = YAS_CORE_DIRECTION_SENDS;

export interface YasFamilyOffer {
  family: number;
  versions: readonly number[];
  required?: boolean;
}

export interface YasOperation {
  direction: number;
  class: number;
  kind: number;
}

export interface YasFamilyDescriptor {
  family: number;
  version: number;
  runtimeState: number;
  operations: readonly YasOperation[];
  limits: readonly YasExtension[];
}

export interface YasClientHelloOptions {
  minMinor?: number;
  maxMinor?: number;
  receiveMaxFrame?: number;
  receiveMaxDecoded?: number;
  receiveMaxDatagram?: number;
  receiveMaxBuffered?: bigint;
  clientInstance: Uint8Array;
  clientName?: string;
  clientRelease?: string;
  families?: readonly YasFamilyOffer[];
  codecs?: readonly number[];
  extensions?: readonly YasExtension[];
}

export interface YasServerHello {
  minor: number;
  bootId: Uint8Array;
  sessionId: Uint8Array;
  receiveMaxFrame: number;
  receiveMaxDecoded: number;
  receiveMaxDatagram: number;
  receiveMaxBuffered: bigint;
  serverMonotonicNs: bigint;
  catalogRevision: bigint;
  serverName: string;
  serverRelease: string;
  families: readonly YasFamilyDescriptor[];
  extensions: readonly YasExtension[];
}

export interface YasReceiveLimits {
  receiveMaxFrame: number;
  receiveMaxDecoded: number;
  receiveMaxDatagram: number;
  receiveMaxBuffered: bigint;
}

export function validateReceiveLimits(
  receiveMaxFrame: number,
  receiveMaxDecoded: number,
  receiveMaxDatagram: number,
  receiveMaxBuffered: bigint,
  endpoint = "peer",
): void {
  if (
    receiveMaxFrame < YAS_CORRELATED_HEADER_BYTES ||
    receiveMaxFrame > YAS_MAX_WIRE_FRAME
  )
    throw new YasProtocolError(
      `${endpoint} receive_max_frame exceeds YAS limits`,
    );
  if (
    receiveMaxDecoded < receiveMaxFrame ||
    receiveMaxDecoded > YAS_MAX_DECODED_FRAME
  )
    throw new YasProtocolError(
      `${endpoint} receive_max_decoded exceeds YAS limits`,
    );
  if (
    (receiveMaxDatagram !== 0 && receiveMaxDatagram < YAS_EVENT_HEADER_BYTES) ||
    receiveMaxDatagram > YAS_MAX_DATAGRAM
  )
    throw new YasProtocolError(
      `${endpoint} receive_max_datagram exceeds YAS limits`,
    );
  if (receiveMaxBuffered === 0n || receiveMaxBuffered > YAS_MAX_BUFFERED)
    throw new YasProtocolError(
      `${endpoint} receive_max_buffered exceeds YAS limits`,
    );
}

export function validateReceiveLimitUpdate(
  next: YasReceiveLimits,
  previous: YasReceiveLimits,
  endpoint = "peer",
): void {
  validateReceiveLimits(
    next.receiveMaxFrame,
    next.receiveMaxDecoded,
    next.receiveMaxDatagram,
    next.receiveMaxBuffered,
    endpoint,
  );
  if (
    next.receiveMaxFrame < previous.receiveMaxFrame ||
    next.receiveMaxDecoded < previous.receiveMaxDecoded
  )
    throw new YasProtocolError(
      `${endpoint} SESSION_UPDATE reduced a frame limit`,
    );
}

export function encodeClientHello(options: YasClientHelloOptions): Uint8Array {
  if (options.clientInstance.length !== 16)
    throw new YasProtocolError("YAS client instance must contain 16 bytes");
  const minMinor = options.minMinor ?? 0;
  const maxMinor = options.maxMinor ?? 0;
  if (minMinor > maxMinor)
    throw new YasProtocolError("invalid Core minor range");
  const receiveMaxFrame = options.receiveMaxFrame ?? 1024 * 1024;
  const receiveMaxDecoded = options.receiveMaxDecoded ?? 4 * 1024 * 1024;
  if (
    receiveMaxFrame < YAS_CORRELATED_HEADER_BYTES ||
    receiveMaxFrame > YAS_MAX_WIRE_FRAME
  )
    throw new YasProtocolError("receive_max_frame is outside YAS hard limits");
  if (
    receiveMaxDecoded < receiveMaxFrame ||
    receiveMaxDecoded > YAS_MAX_DECODED_FRAME
  )
    throw new YasProtocolError(
      "receive_max_decoded is outside YAS hard limits",
    );
  const receiveMaxDatagram = options.receiveMaxDatagram ?? 0;
  const receiveMaxBuffered = options.receiveMaxBuffered ?? 16n * 1024n * 1024n;
  validateReceiveLimits(
    receiveMaxFrame,
    receiveMaxDecoded,
    receiveMaxDatagram,
    receiveMaxBuffered,
    "client",
  );
  const families = [...(options.families ?? [])];
  let previousFamily = -1;
  const writer = new YasWriter()
    .u16(minMinor)
    .u16(maxMinor)
    .u32(receiveMaxFrame)
    .u32(receiveMaxDecoded)
    .u32(receiveMaxDatagram)
    .u64(receiveMaxBuffered)
    .bytes(options.clientInstance)
    .utf8U16(options.clientName ?? "yas-browser")
    .utf8U16(options.clientRelease ?? "development")
    .u16(families.length);
  for (const offer of families) {
    if (offer.family === YAS_FAMILY_CORE || offer.family <= previousFamily)
      throw new YasProtocolError(
        "family offers must exclude Core and be ordered by ID",
      );
    if (offer.versions.length === 0 || offer.versions.length > 0xff)
      throw new YasProtocolError("family offer has an invalid version count");
    previousFamily = offer.family;
    writer
      .u16(offer.family)
      .u8(offer.versions.length)
      .u8(offer.required ? 1 : 0);
    let previousVersion = 0x1_0000;
    for (const version of offer.versions) {
      if (version === 0 || version >= previousVersion)
        throw new YasProtocolError(
          "offered family versions must be unique and descending",
        );
      previousVersion = version;
      writer.u16(version);
    }
  }
  const codecs = [...(options.codecs ?? [])];
  if (codecs.length > 0xff)
    throw new YasProtocolError("too many compression codecs");
  writer.u8(codecs.length);
  let previousCodec = -1;
  for (const codec of codecs) {
    if (codec === 0 || codec <= previousCodec)
      throw new YasProtocolError("codecs must be unique and ascending");
    previousCodec = codec;
    writer.u16(codec);
  }
  return writer.bytes(encodeExtensions(options.extensions)).finish();
}

export function encodeNegotiatedCodecs(codecs: readonly number[]): Uint8Array {
  if (codecs.length > 0xff)
    throw new YasProtocolError("too many negotiated compression codecs");
  const writer = new YasWriter().u8(codecs.length);
  let previous = -1;
  for (const codec of codecs) {
    if (codec === 0 || codec <= previous)
      throw new YasProtocolError(
        "negotiated codecs must be nonzero, unique and ascending",
      );
    previous = codec;
    writer.u16(codec);
  }
  return writer.finish();
}

export function decodeNegotiatedCodecs(body: Uint8Array): number[] {
  const cursor = new YasCursor(body);
  const count = cursor.u8("negotiated codec count");
  const codecs: number[] = [];
  for (let index = 0; index < count; index += 1)
    codecs.push(cursor.u16("negotiated codec"));
  cursor.end("negotiated codecs");
  encodeNegotiatedCodecs(codecs);
  return codecs;
}

export function negotiatedCodecs(
  extensions: readonly YasExtension[],
): number[] {
  const extension = extensions.find(
    (candidate) =>
      candidate.tag === YAS_CORE_SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION,
  );
  return extension ? decodeNegotiatedCodecs(extension.value) : [];
}

/** What a peer runs on, in Rust's names: `linux`, `x86_64`, `musl`. */
export interface YasPlatform {
  os: string;
  arch: string;
  /** Platform flavour — `musl`, `gnu`, `msvc` — empty where there is none. */
  env: string;
}

export function decodePlatform(body: Uint8Array): YasPlatform {
  const cursor = new YasCursor(body);
  const value = {
    os: cursor.utf8U16("platform OS"),
    arch: cursor.utf8U16("platform architecture"),
    env: cursor.utf8U16("platform environment"),
  };
  cursor.end("platform");
  return value;
}

/**
 * The server's platform, or null when it did not say.
 *
 * Optional by design: an older server simply omits the extension, and a client
 * that needs the answer — to pick an extension build, or to name the host in a
 * list — asks again rather than assuming.
 */
export function serverPlatform(
  extensions: readonly YasExtension[],
): YasPlatform | null {
  const extension = extensions.find(
    (candidate) => candidate.tag === YAS_CORE_SERVER_HELLO_PLATFORM_EXTENSION,
  );
  if (!extension) return null;
  try {
    return decodePlatform(extension.value);
  } catch {
    // A platform nobody can parse is a platform nobody was told.
    return null;
  }
}

export function encodeFamilyDescriptor(value: YasFamilyDescriptor): Uint8Array {
  validateFamilyDescriptor(value);
  const body = new YasWriter()
    .u16(value.family)
    .u16(value.version)
    .u8(value.runtimeState)
    .u8(0)
    .u16(value.operations.length);
  for (const operation of value.operations)
    body.u8(operation.direction).u8(operation.class).u16(operation.kind);
  body.bytes(encodeExtensions(value.limits));
  return new YasWriter().bytesU32(body.finish()).finish();
}

function validateFamilyDescriptor(value: YasFamilyDescriptor): void {
  if (value.version === 0)
    throw new YasProtocolError("family descriptor version is zero");
  if (value.runtimeState > YAS_RUNTIME_UNAVAILABLE)
    throw new YasProtocolError("unknown family runtime state");
  if (value.operations.length > 0xffff)
    throw new YasProtocolError("too many family operations");
  const seen = new Set<string>();
  for (const operation of value.operations) {
    if (operation.direction === 0 || operation.direction & ~3)
      throw new YasProtocolError("invalid operation direction");
    if (
      operation.class !== YAS_CLASS_EVENT &&
      operation.class !== YAS_CLASS_REQUEST
    )
      throw new YasProtocolError("invalid family operation class");
    const key = `${operation.class}/${operation.kind}`;
    if (seen.has(key)) throw new YasProtocolError("duplicate family operation");
    seen.add(key);
    const canonicalDirection =
      YAS_OPERATION_DIRECTION_MASKS[
        `${value.family}/${operation.class}/${operation.kind}`
      ];
    if (
      canonicalDirection !== undefined &&
      (operation.direction & ~canonicalDirection) !== 0
    )
      throw new YasProtocolError(
        "family operation advertises a forbidden direction",
      );
  }
  encodeExtensions(value.limits);
  validateFamilyLimits(value);
}

function validateFamilyLimits(value: YasFamilyDescriptor): void {
  const policies = YAS_FAMILY_LIMIT_POLICIES[value.family];
  // Private families define their own limit policy. Canonical families are all
  // present in the generated table, including families with no limits.
  if (policies === undefined) return;

  const policyByTag = new Map(policies.map((policy) => [policy[0], policy]));
  const values = new Map<number, bigint>();
  for (const extension of value.limits) {
    const policy = policyByTag.get(extension.tag);
    if (!policy) {
      if (extension.required)
        throw new YasProtocolError(
          `unknown required family limit ${extension.tag}`,
        );
      continue;
    }
    const [tag, width, , hardMin, hardMax] = policy;
    if (extension.value.length !== width)
      throw new YasProtocolError(`family limit ${tag} must be ${width} bytes`);
    const cursor = new YasCursor(extension.value);
    const limit =
      width === 4
        ? BigInt(cursor.u32(`family limit ${tag}`))
        : cursor.u64(`family limit ${tag}`);
    if (limit < hardMin || limit > hardMax)
      throw new YasProtocolError(
        `family limit ${tag} is outside its canonical bounds`,
      );
    values.set(tag, limit);
  }
  for (const [tag, , required] of policies) {
    if (required && !values.has(tag))
      throw new YasProtocolError(`missing required family limit ${tag}`);
  }
  if (
    value.family === YAS_FAMILY_KV &&
    values.get(YAS_KV_LIMIT_MAX_INLINE_BYTES)! >
      values.get(YAS_KV_LIMIT_MAX_VALUE_BYTES)!
  )
    throw new YasProtocolError(
      "KV inline byte limit exceeds its value byte limit",
    );
  if (
    value.family === YAS_FAMILY_EVENTS &&
    values.get(YAS_EVENTS_LIMIT_MIN_RING_BYTES)! >
      values.get(YAS_EVENTS_LIMIT_MAX_RING_BYTES)!
  )
    throw new YasProtocolError(
      "Events minimum ring byte limit exceeds its maximum",
    );
  if (
    value.family === YAS_FAMILY_RELAY &&
    values.get(YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS)! >
      values.get(YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION)!
  )
    throw new YasProtocolError(
      "Relay pending-connect limit exceeds its per-session link limit",
    );
}

export function encodeServerHello(value: YasServerHello): Uint8Array {
  validateReceiveLimits(
    value.receiveMaxFrame,
    value.receiveMaxDecoded,
    value.receiveMaxDatagram,
    value.receiveMaxBuffered,
    "server",
  );
  if (
    value.bootId.length !== 16 ||
    value.sessionId.length !== 16 ||
    value.families.length === 0 ||
    value.families[0]!.family !== YAS_FAMILY_CORE
  )
    throw new YasProtocolError("invalid HELLO Result identity or families");
  let previousFamily = -1;
  const writer = new YasWriter()
    .u16(value.minor)
    .u16(0)
    .bytes(value.bootId)
    .bytes(value.sessionId)
    .u32(value.receiveMaxFrame)
    .u32(value.receiveMaxDecoded)
    .u32(value.receiveMaxDatagram)
    .u64(value.receiveMaxBuffered)
    .u64(value.serverMonotonicNs)
    .u64(value.catalogRevision)
    .utf8U16(value.serverName)
    .utf8U16(value.serverRelease)
    .u16(value.families.length);
  for (const family of value.families) {
    if (family.family <= previousFamily)
      throw new YasProtocolError("HELLO family descriptors are not ordered");
    previousFamily = family.family;
    writer.bytes(encodeFamilyDescriptor(family));
  }
  validateCoreOutputExtensions(value.extensions, "HELLO Result extensions");
  for (const extension of value.extensions) {
    if (extension.tag === YAS_CORE_SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION)
      decodeNegotiatedCodecs(extension.value);
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeServerHello(body: Uint8Array): YasServerHello {
  const cursor = new YasCursor(body);
  const minor = cursor.u16("Core minor");
  if (cursor.u16("HELLO reserved") !== 0)
    throw new YasProtocolError("HELLO reserved field is nonzero");
  const bootId = new Uint8Array(cursor.take(16, "boot ID"));
  const sessionId = new Uint8Array(cursor.take(16, "session ID"));
  const receiveMaxFrame = cursor.u32("server receive_max_frame");
  const receiveMaxDecoded = cursor.u32("server receive_max_decoded");
  const receiveMaxDatagram = cursor.u32("server receive_max_datagram");
  const receiveMaxBuffered = cursor.u64("server receive_max_buffered");
  const serverMonotonicNs = cursor.u64("server monotonic time");
  const catalogRevision = cursor.u64("catalog revision");
  const serverName = cursor.utf8U16("server name");
  const serverRelease = cursor.utf8U16("server release");
  const familyCount = cursor.u16("family count");
  const families: YasFamilyDescriptor[] = [];
  let previous = -1;
  for (let i = 0; i < familyCount; i++) {
    const descriptor = decodeFamilyDescriptor(cursor);
    if (descriptor.family <= previous)
      throw new YasProtocolError("HELLO family descriptors are not ordered");
    previous = descriptor.family;
    families.push(descriptor);
  }
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "HELLO Result extensions",
  );
  cursor.end("HELLO Result");
  if (families.length === 0 || families[0]!.family !== YAS_FAMILY_CORE)
    throw new YasProtocolError("HELLO Result does not select Core");
  validateCoreOutputExtensions(extensions, "HELLO Result extensions");
  for (const extension of extensions) {
    if (extension.tag === YAS_CORE_SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION)
      decodeNegotiatedCodecs(extension.value);
  }
  validateReceiveLimits(
    receiveMaxFrame,
    receiveMaxDecoded,
    receiveMaxDatagram,
    receiveMaxBuffered,
    "server",
  );
  return {
    minor,
    bootId,
    sessionId,
    receiveMaxFrame,
    receiveMaxDecoded,
    receiveMaxDatagram,
    receiveMaxBuffered,
    serverMonotonicNs,
    catalogRevision,
    serverName,
    serverRelease,
    families,
    extensions,
  };
}

export function decodeFamilyDescriptor(cursor: YasCursor): YasFamilyDescriptor {
  const descriptor = cursor.sub(
    cursor.u32("family descriptor length"),
    "family descriptor",
  );
  const family = descriptor.u16("descriptor family");
  const version = descriptor.u16("descriptor version");
  const runtimeState = descriptor.u8("runtime state");
  if (runtimeState > YAS_RUNTIME_UNAVAILABLE)
    throw new YasProtocolError("unknown family runtime state");
  if (descriptor.u8("descriptor reserved") !== 0)
    throw new YasProtocolError("family descriptor reserved field is nonzero");
  const operationCount = descriptor.u16("operation count");
  const operations: YasOperation[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < operationCount; i++) {
    const direction = descriptor.u8("operation direction");
    const frameClass = descriptor.u8("operation class");
    const kind = descriptor.u16("operation kind");
    if (direction === 0 || direction & ~3)
      throw new YasProtocolError("invalid operation direction");
    if (frameClass !== YAS_CLASS_EVENT && frameClass !== YAS_CLASS_REQUEST)
      throw new YasProtocolError("invalid operation class");
    const key = `${frameClass}/${kind}`;
    if (seen.has(key)) throw new YasProtocolError("duplicate family operation");
    seen.add(key);
    operations.push({ direction, class: frameClass, kind });
  }
  const limitsBody = descriptor.take(
    descriptor.u32("family limits length"),
    "family limits",
  );
  validateExtensionBody(limitsBody, "family limits");
  const limits = decodeExtensionBody(limitsBody);
  descriptor.end("family descriptor");
  const value = { family, version, runtimeState, operations, limits };
  validateFamilyDescriptor(value);
  return value;
}

function decodeExtensionBody(body: Uint8Array): YasExtension[] {
  const wrapped = new YasWriter().bytesU32(body).finish();
  const cursor = new YasCursor(wrapped);
  return decodeExtensions(cursor);
}

export function validateServerHello(
  hello: YasServerHello,
  options: YasClientHelloOptions,
): Map<number, YasFamilyDescriptor> {
  if (
    hello.minor < (options.minMinor ?? 0) ||
    hello.minor > (options.maxMinor ?? 0)
  )
    throw new YasProtocolError("server selected an unoffered Core minor");
  const offers = new Map(
    (options.families ?? []).map((offer) => [offer.family, offer]),
  );
  const selected = new Map<number, YasFamilyDescriptor>();
  for (const descriptor of hello.families) {
    if (descriptor.family === YAS_FAMILY_CORE) {
      if (descriptor.version !== YAS_CORE_VERSION)
        throw new YasProtocolError(
          "server selected an unsupported Core version",
        );
    } else {
      const offer = offers.get(descriptor.family);
      if (!offer || !offer.versions.includes(descriptor.version))
        throw new YasProtocolError(
          "server selected an unoffered family version",
        );
    }
    selected.set(descriptor.family, descriptor);
  }
  if (!selected.has(YAS_FAMILY_CORE))
    throw new YasProtocolError("server omitted the mandatory Core family");
  for (const offer of offers.values()) {
    if (offer.required && !selected.has(offer.family))
      throw new YasProtocolError(
        `server omitted required family 0x${offer.family.toString(16)}`,
      );
  }
  const offeredCodecs = options.codecs ?? [];
  if (
    negotiatedCodecs(hello.extensions).some(
      (codec) => !offeredCodecs.includes(codec),
    )
  )
    throw new YasProtocolError("server selected an unoffered codec");
  for (const family of selected.keys())
    for (const dependency of YAS_FAMILY_DEPENDENCIES[family] ?? [])
      if (!selected.has(dependency))
        throw new YasProtocolError(
          `server selected family 0x${family.toString(16)} without dependency 0x${dependency.toString(16)}`,
        );
  return selected;
}

export interface YasSessionInfoBody {
  sessionId: Uint8Array;
  catalogRevision: bigint;
  receiveMaxFrame: number;
  receiveMaxDecoded: number;
  receiveMaxDatagram: number;
  receiveMaxBuffered: bigint;
  serverMonotonicNs: bigint;
  families: readonly YasFamilyDescriptor[];
  extensions: readonly YasExtension[];
}

export function encodeSessionInfo(value: YasSessionInfoBody): Uint8Array {
  validateReceiveLimits(
    value.receiveMaxFrame,
    value.receiveMaxDecoded,
    value.receiveMaxDatagram,
    value.receiveMaxBuffered,
    "server",
  );
  if (
    value.sessionId.length !== 16 ||
    value.families.length === 0 ||
    value.families[0]!.family !== YAS_FAMILY_CORE
  )
    throw new YasProtocolError("invalid SESSION_INFO identity or families");
  let previous = -1;
  const writer = new YasWriter()
    .bytes(value.sessionId)
    .u64(value.catalogRevision)
    .u32(value.receiveMaxFrame)
    .u32(value.receiveMaxDecoded)
    .u32(value.receiveMaxDatagram)
    .u64(value.receiveMaxBuffered)
    .u64(value.serverMonotonicNs)
    .u16(value.families.length);
  for (const family of value.families) {
    if (family.family <= previous)
      throw new YasProtocolError("SESSION_INFO families are not ordered");
    previous = family.family;
    writer.bytes(encodeFamilyDescriptor(family));
  }
  validateCoreOutputExtensions(value.extensions, "SESSION_INFO extensions");
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeSessionInfo(body: Uint8Array): YasSessionInfoBody {
  const cursor = new YasCursor(body);
  const sessionId = new Uint8Array(cursor.take(16, "session ID"));
  const catalogRevision = cursor.u64("catalog revision");
  const receiveMaxFrame = cursor.u32("receive_max_frame");
  const receiveMaxDecoded = cursor.u32("receive_max_decoded");
  const receiveMaxDatagram = cursor.u32("receive_max_datagram");
  const receiveMaxBuffered = cursor.u64("receive_max_buffered");
  const serverMonotonicNs = cursor.u64("server monotonic time");
  const count = cursor.u16("family count");
  const families: YasFamilyDescriptor[] = [];
  let previous = -1;
  for (let i = 0; i < count; i++) {
    const descriptor = decodeFamilyDescriptor(cursor);
    if (descriptor.family <= previous)
      throw new YasProtocolError("SESSION_INFO families are not ordered");
    previous = descriptor.family;
    families.push(descriptor);
  }
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "SESSION_INFO extensions",
  );
  cursor.end("SESSION_INFO");
  const value = {
    sessionId,
    catalogRevision,
    receiveMaxFrame,
    receiveMaxDecoded,
    receiveMaxDatagram,
    receiveMaxBuffered,
    serverMonotonicNs,
    families,
    extensions,
  };
  validateCoreOutputExtensions(extensions, "SESSION_INFO extensions");
  encodeSessionInfo(value);
  return value;
}

export function encodePing(senderMonotonicNs: bigint): Uint8Array {
  return new YasWriter().u64(senderMonotonicNs).finish();
}

export function decodePing(body: Uint8Array): bigint {
  const cursor = new YasCursor(body);
  const senderMonotonicNs = cursor.u64("sender monotonic time");
  cursor.end("PING");
  return senderMonotonicNs;
}

export function encodePingResult(value: {
  receiverReceiveNs: bigint;
  receiverSendNs: bigint;
}): Uint8Array {
  return new YasWriter()
    .u64(value.receiverReceiveNs)
    .u64(value.receiverSendNs)
    .finish();
}

export function decodePingResult(body: Uint8Array): {
  receiverReceiveNs: bigint;
  receiverSendNs: bigint;
} {
  const cursor = new YasCursor(body);
  const result = {
    receiverReceiveNs: cursor.u64("receiver receive time"),
    receiverSendNs: cursor.u64("receiver send time"),
  };
  cursor.end("PING Result");
  return result;
}

export function encodeCancel(targetRequestId: number): Uint8Array {
  if (!Number.isInteger(targetRequestId) || targetRequestId <= 0)
    throw new YasProtocolError("CANCEL target request ID is zero or invalid");
  return new YasWriter().u32(targetRequestId).finish();
}

export function decodeCancel(body: Uint8Array): number {
  const cursor = new YasCursor(body);
  const targetRequestId = cursor.u32("CANCEL target request ID");
  cursor.end("CANCEL");
  encodeCancel(targetRequestId);
  return targetRequestId;
}

export interface YasShutdown {
  operationId: Uint8Array;
  graceNs: bigint;
  reason: string;
}

export function encodeShutdown(value: YasShutdown): Uint8Array {
  if (
    value.operationId.length !== 16 ||
    value.operationId.every((byte) => byte === 0)
  )
    throw new YasProtocolError("SHUTDOWN operation ID is invalid");
  return new YasWriter()
    .bytes(value.operationId)
    .u64(value.graceNs)
    .utf8U32(value.reason)
    .finish();
}

export function decodeShutdown(body: Uint8Array): YasShutdown {
  const cursor = new YasCursor(body);
  const value = {
    operationId: new Uint8Array(cursor.take(16, "SHUTDOWN operation ID")),
    graceNs: cursor.u64("SHUTDOWN grace"),
    reason: cursor.utf8U32("SHUTDOWN reason"),
  };
  cursor.end("SHUTDOWN");
  encodeShutdown(value);
  return value;
}

export interface YasGoAway {
  status: number;
  closeDeadlineServerNs: bigint;
  detail: readonly YasExtension[];
}

export function encodeGoAway(value: YasGoAway): Uint8Array {
  validateCoreOutputExtensions(value.detail, "GOAWAY detail");
  return new YasWriter()
    .u16(value.status)
    .u16(0)
    .u64(value.closeDeadlineServerNs)
    .bytes(encodeExtensions(value.detail))
    .finish();
}

export function decodeGoAway(body: Uint8Array): YasGoAway {
  const cursor = new YasCursor(body);
  const status = cursor.u16("GOAWAY status");
  if (cursor.u16("GOAWAY reserved") !== 0)
    throw new YasProtocolError("GOAWAY reserved field is nonzero");
  const closeDeadlineServerNs = cursor.u64("GOAWAY deadline");
  const detail = decodeExtensions(cursor, undefined, "GOAWAY detail");
  cursor.end("GOAWAY");
  const value = { status, closeDeadlineServerNs, detail };
  validateCoreOutputExtensions(detail, "GOAWAY detail");
  return value;
}

export interface YasSessionUpdate extends YasReceiveLimits {
  catalogRevision: bigint;
  extensions: readonly YasExtension[];
}

export function encodeSessionUpdate(value: YasSessionUpdate): Uint8Array {
  validateReceiveLimits(
    value.receiveMaxFrame,
    value.receiveMaxDecoded,
    value.receiveMaxDatagram,
    value.receiveMaxBuffered,
    "server",
  );
  validateCoreOutputExtensions(value.extensions, "SESSION_UPDATE extensions");
  return new YasWriter()
    .u64(value.catalogRevision)
    .u32(value.receiveMaxFrame)
    .u32(value.receiveMaxDecoded)
    .u32(value.receiveMaxDatagram)
    .u64(value.receiveMaxBuffered)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSessionUpdate(body: Uint8Array): YasSessionUpdate {
  const cursor = new YasCursor(body);
  const value = {
    catalogRevision: cursor.u64("catalog revision"),
    receiveMaxFrame: cursor.u32("receive_max_frame"),
    receiveMaxDecoded: cursor.u32("receive_max_decoded"),
    receiveMaxDatagram: cursor.u32("receive_max_datagram"),
    receiveMaxBuffered: cursor.u64("receive_max_buffered"),
    extensions: decodeExtensions(
      cursor,
      undefined,
      "SESSION_UPDATE extensions",
    ),
  };
  cursor.end("SESSION_UPDATE");
  encodeSessionUpdate(value);
  return value;
}

export interface YasFamilyUpdate {
  catalogRevision: bigint;
  descriptor: YasFamilyDescriptor;
}

export function encodeFamilyUpdate(value: YasFamilyUpdate): Uint8Array {
  return new YasWriter()
    .u64(value.catalogRevision)
    .bytes(encodeFamilyDescriptor(value.descriptor))
    .finish();
}

export function decodeFamilyUpdate(body: Uint8Array): YasFamilyUpdate {
  const cursor = new YasCursor(body);
  const value = {
    catalogRevision: cursor.u64("catalog revision"),
    descriptor: decodeFamilyDescriptor(cursor),
  };
  cursor.end("FAMILY_UPDATE");
  return value;
}

function validateCoreOutputExtensions(
  extensions: readonly YasExtension[],
  context: string,
): void {
  if (extensions.some((extension) => extension.required))
    throw new YasProtocolError(`${context} contains a required extension`);
  encodeExtensions(extensions);
}
