import { YAS_FAMILY_FONT } from "./core";
import {
  YAS_FONT_DESCRIBE,
  YAS_FONT_DELIVERY_INLINE,
  YAS_FONT_DELIVERY_TRANSFER,
  YAS_FONT_DESCRIPTION_CONTENT_KIND,
  YAS_FONT_FACE_BYTES_CONTENT_KIND,
  YAS_FONT_FACE_COLOR,
  YAS_FONT_FACE_FETCHABLE,
  YAS_FONT_FACE_VARIABLE,
  YAS_FONT_FAMILY_COLOR,
  YAS_FONT_FAMILY_FETCHABLE,
  YAS_FONT_FAMILY_MONOSPACE,
  YAS_FONT_FAMILY_VARIABLE,
  YAS_FONT_FETCH,
  YAS_FONT_FORMAT_SFNT_CFF,
  YAS_FONT_FORMAT_SFNT_TRUETYPE,
  YAS_FONT_FORMAT_WOFF,
  YAS_FONT_FORMAT_WOFF2,
  YAS_FONT_LIMIT_MAX_CONCURRENT_FETCHES,
  YAS_FONT_LIMIT_MAX_DESCRIPTION_BYTES,
  YAS_FONT_LIMIT_MAX_FACE_BYTES,
  YAS_FONT_LIMIT_MAX_FACES_PER_FAMILY,
  YAS_FONT_LIMIT_MAX_FAMILIES,
  YAS_FONT_LIMIT_MAX_SCAN_DURATION_NS,
  YAS_FONT_LIMIT_REFRESH_INTERVAL_NS,
  YAS_FONT_MAX_CONCURRENT_FETCHES,
  YAS_FONT_MAX_DESCRIPTION_BYTES,
  YAS_FONT_MAX_FACE_BYTES,
  YAS_FONT_MAX_FACES_PER_FAMILY,
  YAS_FONT_MAX_FAMILIES,
  YAS_FONT_MAX_REFRESH_INTERVAL_NS,
  YAS_FONT_MAX_SCAN_DURATION_NS,
  YAS_FONT_STATE,
  YAS_FONT_STATE_ACK,
  YAS_FONT_STYLE_ITALIC,
  YAS_FONT_STYLE_NORMAL,
  YAS_FONT_STYLE_OBLIQUE,
  YAS_FONT_UNWATCH,
  YAS_FONT_VERSION,
  YAS_FONT_WATCH,
} from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateCatalogueRetention,
  YasStateSubscription,
  estimateStateRetainedBytes,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  transfersFor,
} from "./transfer";
import {
  equalBytes,
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FONT_DESCRIBE,
  YAS_FONT_FACE_COLOR,
  YAS_FONT_FACE_FETCHABLE,
  YAS_FONT_FACE_VARIABLE,
  YAS_FONT_FAMILY_COLOR,
  YAS_FONT_FAMILY_FETCHABLE,
  YAS_FONT_FAMILY_MONOSPACE,
  YAS_FONT_FAMILY_VARIABLE,
  YAS_FONT_FETCH,
  YAS_FONT_FORMAT_WOFF,
  YAS_FONT_FORMAT_WOFF2,
  YAS_FONT_STATE,
  YAS_FONT_STATE_ACK,
  YAS_FONT_STYLE_ITALIC,
  YAS_FONT_STYLE_NORMAL,
  YAS_FONT_STYLE_OBLIQUE,
  YAS_FONT_UNWATCH,
  YAS_FONT_VERSION,
  YAS_FONT_WATCH,
} from "./generated";

export const YAS_FONT_FORMAT_TRUETYPE = YAS_FONT_FORMAT_SFNT_TRUETYPE;
export const YAS_FONT_FORMAT_CFF = YAS_FONT_FORMAT_SFNT_CFF;

export interface YasFontFamily {
  handle: bigint;
  generation: bigint;
  flags: number;
  faceCount: number;
  family: string;
  display: string;
  extensions: readonly YasExtension[];
}

export interface YasFontFace {
  handle: bigint;
  contentHash: Uint8Array;
  byteLength: bigint;
  format: number;
  style: number;
  flags: number;
  weightMin: number;
  weightDefault: number;
  weightMax: number;
  stretchMin: number;
  stretchDefault: number;
  stretchMax: number;
  slantTenthsDegrees: number;
  unitsPerEm: number;
  cellAdvance: number;
  ascent: number;
  descent: number;
  lineGap: number;
  subfamily: string;
  postscript: string;
  extensions: readonly YasExtension[];
}

export interface YasFontDescription {
  handle: bigint;
  generation: bigint;
  descriptionHash: Uint8Array;
  family: string;
  faces: readonly YasFontFace[];
  extensions: readonly YasExtension[];
}

export interface YasFontBytes {
  faceHandle: bigint;
  contentHash: Uint8Array;
  format: number;
  bytes: Uint8Array;
}

/** UI-oriented result preserving the current loader's `data` field. */
export interface YasFontFaceData {
  contentHash: Uint8Array;
  format: number;
  data: Uint8Array;
}

export interface YasFontSnapshot {
  revision: bigint;
  families: readonly YasFontFamily[];
}

export interface YasFontLimits {
  maxFamilies: number;
  maxFacesPerFamily: number;
  maxDescriptionBytes: bigint;
  maxFaceBytes: bigint;
  maxConcurrentFetches: number;
  maxScanDurationNs: bigint;
  refreshIntervalNs: bigint;
}

export const YAS_FONT_HARD_LIMITS: YasFontLimits = {
  maxFamilies: YAS_FONT_MAX_FAMILIES,
  maxFacesPerFamily: YAS_FONT_MAX_FACES_PER_FAMILY,
  maxDescriptionBytes: BigInt(YAS_FONT_MAX_DESCRIPTION_BYTES),
  maxFaceBytes: BigInt(YAS_FONT_MAX_FACE_BYTES),
  maxConcurrentFetches: YAS_FONT_MAX_CONCURRENT_FETCHES,
  maxScanDurationNs: BigInt(YAS_FONT_MAX_SCAN_DURATION_NS),
  refreshIntervalNs: BigInt(YAS_FONT_MAX_REFRESH_INTERVAL_NS),
};

export function fontLimitsFromExtensions(
  extensions: readonly YasExtension[],
): YasFontLimits {
  const known = new Set<number>([
    YAS_FONT_LIMIT_MAX_FAMILIES,
    YAS_FONT_LIMIT_MAX_FACES_PER_FAMILY,
    YAS_FONT_LIMIT_MAX_DESCRIPTION_BYTES,
    YAS_FONT_LIMIT_MAX_FACE_BYTES,
    YAS_FONT_LIMIT_MAX_CONCURRENT_FETCHES,
    YAS_FONT_LIMIT_MAX_SCAN_DURATION_NS,
    YAS_FONT_LIMIT_REFRESH_INTERVAL_NS,
  ]);
  if (
    extensions.some(
      (extension) => extension.required && !known.has(extension.tag),
    )
  )
    throw new YasProtocolError("unknown required Font family limit");
  const value = {
    maxFamilies: fontLimitU32(extensions, YAS_FONT_LIMIT_MAX_FAMILIES),
    maxFacesPerFamily: fontLimitU32(
      extensions,
      YAS_FONT_LIMIT_MAX_FACES_PER_FAMILY,
    ),
    maxDescriptionBytes: fontLimitU64(
      extensions,
      YAS_FONT_LIMIT_MAX_DESCRIPTION_BYTES,
    ),
    maxFaceBytes: fontLimitU64(extensions, YAS_FONT_LIMIT_MAX_FACE_BYTES),
    maxConcurrentFetches: fontLimitU32(
      extensions,
      YAS_FONT_LIMIT_MAX_CONCURRENT_FETCHES,
    ),
    maxScanDurationNs: fontLimitU64(
      extensions,
      YAS_FONT_LIMIT_MAX_SCAN_DURATION_NS,
    ),
    refreshIntervalNs: fontLimitU64(
      extensions,
      YAS_FONT_LIMIT_REFRESH_INTERVAL_NS,
    ),
  };
  validateFontLimits(value);
  return value;
}

export function fontLimitsExtensions(value: YasFontLimits): YasExtension[] {
  validateFontLimits(value);
  return [
    fontLimit32(YAS_FONT_LIMIT_MAX_FAMILIES, value.maxFamilies),
    fontLimit32(YAS_FONT_LIMIT_MAX_FACES_PER_FAMILY, value.maxFacesPerFamily),
    fontLimit64(
      YAS_FONT_LIMIT_MAX_DESCRIPTION_BYTES,
      value.maxDescriptionBytes,
    ),
    fontLimit64(YAS_FONT_LIMIT_MAX_FACE_BYTES, value.maxFaceBytes),
    fontLimit32(
      YAS_FONT_LIMIT_MAX_CONCURRENT_FETCHES,
      value.maxConcurrentFetches,
    ),
    fontLimit64(YAS_FONT_LIMIT_MAX_SCAN_DURATION_NS, value.maxScanDurationNs),
    fontLimit64(YAS_FONT_LIMIT_REFRESH_INTERVAL_NS, value.refreshIntervalNs),
  ];
}

export type YasHashBytes = (
  bytes: Uint8Array,
) => Uint8Array | Promise<Uint8Array>;

export function decodeFontFamily(body: Uint8Array): YasFontFamily {
  const cursor = new YasCursor(body);
  const handle = cursor.u64("font handle");
  const generation = cursor.u64("font generation");
  const flags = cursor.u16("font family flags");
  const faceCount = cursor.u16("font face count");
  const family = cursor.utf8U16("font family");
  const display = cursor.utf8U16("font display name");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "font family extensions",
  );
  cursor.end("Font family record");
  if (handle === 0n || generation === 0n)
    throw new YasProtocolError("font handle or generation is zero");
  if (
    flags &
    ~(
      YAS_FONT_FAMILY_MONOSPACE |
      YAS_FONT_FAMILY_VARIABLE |
      YAS_FONT_FAMILY_COLOR |
      YAS_FONT_FAMILY_FETCHABLE
    )
  )
    throw new YasProtocolError("reserved font family flags are nonzero");
  return { handle, generation, flags, faceCount, family, display, extensions };
}

function encodeFontFamilyRecord(value: YasFontFamily): Uint8Array {
  return new YasWriter()
    .u64(value.handle)
    .u64(value.generation)
    .u16(value.flags)
    .u16(value.faceCount)
    .utf8U16(value.family)
    .utf8U16(value.display)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFontDescription(
  bytes: Uint8Array,
  header: Pick<YasFontDescription, "handle" | "generation" | "descriptionHash">,
): YasFontDescription {
  if (bytes.length > YAS_FONT_MAX_DESCRIPTION_BYTES)
    throw new YasProtocolError("font description exceeds hard limit");
  const cursor = new YasCursor(bytes);
  const family = cursor.utf8U16("font family");
  const count = cursor.u16("font face count");
  if (count > YAS_FONT_MAX_FACES_PER_FAMILY)
    throw new YasProtocolError("font description has too many faces");
  const faces: YasFontFace[] = [];
  const handles = new Set<bigint>();
  for (let i = 0; i < count; i++) {
    const face = decodeFontFace(
      cursor.sub(cursor.u32("face record length"), "face record"),
    );
    if (handles.has(face.handle))
      throw new YasProtocolError("duplicate font face handle");
    handles.add(face.handle);
    faces.push(face);
  }
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "font description extensions",
  );
  cursor.end("font description");
  return { ...header, family, faces, extensions };
}

function decodeFontFace(cursor: YasCursor): YasFontFace {
  const handle = cursor.u64("face handle");
  const contentHash = new Uint8Array(cursor.take(32, "font content hash"));
  const byteLength = cursor.u64("font byte length");
  const format = cursor.u8("font format");
  const style = cursor.u8("font style");
  const flags = cursor.u16("font face flags");
  const weightMin = cursor.u16("minimum font weight");
  const weightDefault = cursor.u16("default font weight");
  const weightMax = cursor.u16("maximum font weight");
  const stretchMin = cursor.u16("minimum font stretch");
  const stretchDefault = cursor.u16("default font stretch");
  const stretchMax = cursor.u16("maximum font stretch");
  const slantTenthsDegrees = cursor.i16("font slant");
  const unitsPerEm = cursor.u16("font units per em");
  const cellAdvance = cursor.i32("font cell advance");
  const ascent = cursor.i32("font ascent");
  const descent = cursor.i32("font descent");
  const lineGap = cursor.i32("font line gap");
  const subfamily = cursor.utf8U16("font subfamily");
  const postscript = cursor.utf8U16("font PostScript name");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "font face extensions",
  );
  cursor.end("font face record");
  if (
    handle === 0n ||
    byteLength === 0n ||
    format > YAS_FONT_FORMAT_WOFF2 ||
    style > YAS_FONT_STYLE_OBLIQUE
  )
    throw new YasProtocolError("invalid font face identity or enum");
  if (
    flags &
    ~(YAS_FONT_FACE_VARIABLE | YAS_FONT_FACE_COLOR | YAS_FONT_FACE_FETCHABLE)
  )
    throw new YasProtocolError("reserved font face flags are nonzero");
  if (
    weightMin < 1 ||
    weightMax > 1000 ||
    weightMin > weightDefault ||
    weightDefault > weightMax ||
    stretchMin > stretchDefault ||
    stretchDefault > stretchMax ||
    unitsPerEm === 0
  )
    throw new YasProtocolError("invalid font face metric range");
  return {
    handle,
    contentHash,
    byteLength,
    format,
    style,
    flags,
    weightMin,
    weightDefault,
    weightMax,
    stretchMin,
    stretchDefault,
    stretchMax,
    slantTenthsDegrees,
    unitsPerEm,
    cellAdvance,
    ascent,
    descent,
    lineGap,
    subfamily,
    postscript,
    extensions,
  };
}

export class YasFontCatalog {
  private current = new Map<bigint, YasFontFamily>();
  private currentRetention: YasStateCatalogueRetention<bigint>;
  private staging: Map<bigint, YasFontFamily> | null = null;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasFontSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private _revision = 0n;
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    private readonly limits: () => YasFontLimits = () => YAS_FONT_HARD_LIMITS,
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_FONT) {
        this.cancelPendingWatch(
          new YasProtocolError("Font catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasFontSnapshot {
    return {
      revision: this._revision,
      families: [...this.current.values()].sort((left, right) =>
        left.family.localeCompare(right.family),
      ),
    };
  }

  subscribe(listener: (snapshot: YasFontSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Font catalogue is disposed");
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      this.listeners.delete(listener);
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(options: YasWatchOptions = {}): Promise<YasFontSnapshot> {
    if (this.disposed) throw new Error("Font catalogue is disposed");
    if (this._revision !== 0n && this.subscription?.active)
      return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasFontSnapshot>((resolve, reject) => {
      rejectPending = (error) => {
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        reject(error);
      };
      this.pendingFirstSnapshots.add(rejectPending);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        resolve(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } finally {
      remove?.();
      this.pendingFirstSnapshots.delete(rejectPending);
    }
  }

  watch(options: YasWatchOptions = {}): Promise<void> {
    if (this.disposed)
      return Promise.reject(new Error("Font catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.subscription = null;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_FONT,
      YAS_FONT_WATCH,
      YAS_FONT_UNWATCH,
      YAS_FONT_STATE,
      YAS_FONT_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Font catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    let pending!: Promise<void>;
    pending = Promise.race([watched, cancelled]).finally(() => {
      if (this.pendingWatch !== pending) return;
      this.pendingWatch = null;
      if (this.pendingWatchCancel === cancel) this.pendingWatchCancel = null;
    });
    this.pendingWatch = pending;
    this.pendingWatchCancel = cancel;
    return pending;
  }

  async unwatch(): Promise<void> {
    this.cancelPendingWatch(
      new YasProtocolError("Font catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.resetLocal();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Font catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.current.clear();
    this.staging = null;
    this.stagingRetention = null;
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.currentRetention.dispose();
      this.stagingRetention?.dispose();
      this.current = new Map();
      this.currentRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      this.staging = null;
      this.stagingRetention = null;
      this._revision = 0n;
      this.emit();
    } else if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.stagingRetention?.dispose();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
    } else if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Font snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
    } else if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Font snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = this.staging;
      this.currentRetention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
    } else if (batch.phase === YAS_STATE_DELTA) {
      const nextRetention = this.currentRetention.clone();
      let next: Map<bigint, YasFontFamily>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, nextRetention, batch.records);
      } catch (error) {
        nextRetention.dispose();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = next;
      this.currentRetention = nextRetention;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
    }
  }

  private validateCatalog(families: ReadonlyMap<bigint, YasFontFamily>): void {
    if (families.size > this.limits().maxFamilies)
      throw new YasProtocolError("Font catalogue exceeds negotiated limit");
    const names = new Set<string>();
    for (const family of families.values()) {
      if (family.faceCount > this.limits().maxFacesPerFamily)
        throw new YasProtocolError("Font family exceeds negotiated face limit");
      if (names.has(family.family))
        throw new YasProtocolError(
          "Font catalogue family names are not unique",
        );
      names.add(family.family);
    }
  }

  private applyRecords(
    target: Map<bigint, YasFontFamily>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<bigint, YasFontFamily | null>();
    const remember = (key: bigint) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    try {
      for (const record of records) {
        if (
          record.kind === YAS_STATE_ADD ||
          record.kind === YAS_STATE_REPLACE
        ) {
          const decoded = decodeFontFamily(record.body);
          const encoded = encodeFontFamilyRecord(decoded);
          const family = decodeFontFamily(encoded);
          const exists = target.has(family.handle);
          if ((record.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError(
              "Font state ADD/REPLACE precondition failed",
            );
          remember(family.handle);
          retention.upsert(
            family.handle,
            Math.max(encoded.length, estimateStateRetainedBytes(family)),
          );
          target.set(family.handle, family);
        } else if (record.kind === YAS_STATE_REMOVE) {
          const cursor = new YasCursor(record.body);
          const handle = cursor.u64("removed font handle");
          const generation = cursor.u64("removed font generation");
          cursor.end("Font REMOVE record");
          const family = target.get(handle);
          if (!family || family.generation !== generation)
            throw new YasProtocolError(
              "Font REMOVE names an unknown generation",
            );
          remember(handle);
          retention.remove(handle);
          target.delete(handle);
        } else if (record.kind === YAS_STATE_PATCH) {
          throw new YasProtocolError("Font v1 does not define PATCH records");
        } else if (record.flags & 1) {
          throw new YasProtocolError("unknown required Font state record");
        }
      }
      this.validateCatalog(target);
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeFontFamilyRecord(original).length,
              estimateStateRetainedBytes(original),
            ),
          );
          target.set(key, original);
        } else target.delete(key);
      }
      throw error;
    }
  }

  private emit(): void {
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block sibling delivery or wire cleanup.
      }
    }
  }

  private resetLocal(): void {
    if (this.disposed) return;
    this.subscription = null;
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
    this.current = new Map();
    this.currentRetention = YasStateCatalogueRetention.forConnection(
      this.connection,
    );
    this._revision = 0n;
    this.emit();
  }

  private cancelPendingWatch(error: unknown): void {
    this.watchEpoch++;
    const cancel = this.pendingWatchCancel;
    this.pendingWatch = null;
    this.pendingWatchCancel = null;
    cancel?.(error);
    for (const reject of [...this.pendingFirstSnapshots]) reject(error);
    this.pendingFirstSnapshots.clear();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }
}

export class YasFontClient {
  readonly catalog: YasFontCatalog;
  private readonly transfers;
  private activeFetches = 0;

  constructor(
    readonly connection: YasConnection,
    private readonly hashBytes: YasHashBytes = defaultBlake3,
  ) {
    connection.family(YAS_FAMILY_FONT, YAS_FONT_VERSION);
    connection.registerFamilyLimitValidator(
      YAS_FAMILY_FONT,
      fontLimitsFromExtensions,
    );
    this.transfers = transfersFor(connection);
    this.catalog = new YasFontCatalog(connection, () => this.limits);
  }

  get limits(): YasFontLimits {
    return fontLimitsFromExtensions(
      this.connection.family(YAS_FAMILY_FONT, YAS_FONT_VERSION).limits,
    );
  }

  dispose(): void {
    this.catalog.dispose();
  }

  listFamilies(options: YasWatchOptions = {}): Promise<YasFontSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  async describe(
    family: Pick<YasFontFamily, "handle" | "generation">,
    initialReceiveCredit = 256n * 1024n,
  ): Promise<YasFontDescription> {
    const limits = this.limits;
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      32n * 1024n,
    );
    let transferAccepted = false;
    let leaseReleased = false;
    try {
      const decoded = await this.connection.requestDecoded(
        YAS_FAMILY_FONT,
        YAS_FONT_DESCRIBE,
        new YasWriter()
          .u64(family.handle)
          .u64(family.generation)
          .u64(lease.bytes)
          .bytes(encodeExtensions())
          .finish(),
        (body) => {
          const cursor = new YasCursor(body);
          const handle = cursor.u64("font handle");
          const generation = cursor.u64("font generation");
          const descriptionHash = new Uint8Array(
            cursor.take(32, "font description hash"),
          );
          const delivery = cursor.u8("font description delivery");
          if (
            cursor
              .take(3, "font description reserved")
              .some((value) => value !== 0)
          )
            throw new YasProtocolError(
              "Font DESCRIBE reserved bytes are nonzero",
            );
          if (handle !== family.handle || generation !== family.generation)
            throw new YasProtocolError(
              "Font DESCRIBE Result does not match its request",
            );
          if (delivery === YAS_FONT_DELIVERY_INLINE) {
            const bytes = new Uint8Array(
              cursor.bytesU32("inline font description"),
            );
            if (bytes.length > 32 * 1024)
              throw new YasProtocolError(
                "inline font description exceeds 32 KiB",
              );
            cursor.end("Font DESCRIBE Result");
            lease.release();
            leaseReleased = true;
            return { handle, generation, descriptionHash, bytes };
          }
          if (delivery !== YAS_FONT_DELIVERY_TRANSFER)
            throw new YasProtocolError("unknown Font DESCRIBE delivery mode");
          const expectedLength = cursor.u64("font description length");
          if (
            expectedLength === 0n ||
            expectedLength > limits.maxDescriptionBytes
          )
            throw new YasProtocolError(
              "font description exceeds negotiated limit",
            );
          const descriptor = decodeTransferDescriptor(cursor);
          cursor.end("Font DESCRIBE Result");
          this.validateServerByteTransfer(
            descriptor,
            YAS_FONT_DESCRIPTION_CONTENT_KIND,
          );
          const transfer = this.transfers.acceptServerDescriptor(
            descriptor,
            lease,
          );
          transferAccepted = true;
          return {
            handle,
            generation,
            descriptionHash,
            transfer,
            expectedLength,
          };
        },
      );
      const descriptionBytes =
        decoded.bytes !== undefined
          ? decoded.bytes
          : await decoded.transfer.collect(decoded.expectedLength);
      if (BigInt(descriptionBytes.length) > limits.maxDescriptionBytes)
        throw new YasProtocolError("font description exceeds negotiated limit");
      await verifyHash(
        this.hashBytes,
        descriptionBytes,
        decoded.descriptionHash,
        "font description",
      );
      const description = decodeFontDescription(descriptionBytes, {
        handle: decoded.handle,
        generation: decoded.generation,
        descriptionHash: decoded.descriptionHash,
      });
      if (description.faces.length > limits.maxFacesPerFamily)
        throw new YasProtocolError(
          "font description exceeds negotiated face limit",
        );
      return description;
    } catch (error) {
      if (!transferAccepted && !leaseReleased) lease.release();
      throw error;
    }
  }

  async fetch(
    face: Pick<YasFontFace, "handle" | "contentHash" | "byteLength" | "format">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasFontBytes> {
    const limits = this.limits;
    if (face.byteLength === 0n || face.byteLength > limits.maxFaceBytes)
      throw new YasProtocolError("font face exceeds negotiated limit");
    if (this.activeFetches >= limits.maxConcurrentFetches)
      throw new YasProtocolError("Font concurrent-fetch limit is exhausted");
    this.activeFetches++;
    if (face.contentHash.length !== 32)
      throw new YasProtocolError(
        "expected font content hash must contain 32 bytes",
      );
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      64n * 1024n,
    );
    let transferAccepted = false;
    try {
      const decoded = await this.connection.requestDecoded(
        YAS_FAMILY_FONT,
        YAS_FONT_FETCH,
        new YasWriter()
          .u64(face.handle)
          .bytes(face.contentHash)
          .u64(lease.bytes)
          .bytes(encodeExtensions())
          .finish(),
        (body) => {
          const cursor = new YasCursor(body);
          const faceHandle = cursor.u64("face handle");
          const contentHash = new Uint8Array(
            cursor.take(32, "font content hash"),
          );
          const byteLength = cursor.u64("font byte length");
          const format = cursor.u8("font format");
          if (
            cursor.take(3, "Font FETCH reserved").some((value) => value !== 0)
          )
            throw new YasProtocolError("Font FETCH reserved bytes are nonzero");
          const descriptor = decodeTransferDescriptor(cursor);
          cursor.end("Font FETCH Result");
          if (
            faceHandle !== face.handle ||
            !equalBytes(contentHash, face.contentHash) ||
            byteLength !== face.byteLength ||
            format !== face.format
          )
            throw new YasProtocolError(
              "Font FETCH Result does not match the described face",
            );
          if (byteLength === 0n || byteLength > limits.maxFaceBytes)
            throw new YasProtocolError("font face exceeds negotiated limit");
          this.validateServerByteTransfer(
            descriptor,
            YAS_FONT_FACE_BYTES_CONTENT_KIND,
          );
          const transfer = this.transfers.acceptServerDescriptor(
            descriptor,
            lease,
          );
          transferAccepted = true;
          return { faceHandle, contentHash, byteLength, format, transfer };
        },
      );
      const bytes = await decoded.transfer.collect(decoded.byteLength);
      await verifyHash(this.hashBytes, bytes, decoded.contentHash, "font face");
      return {
        faceHandle: decoded.faceHandle,
        contentHash: decoded.contentHash,
        format: decoded.format,
        bytes,
      };
    } catch (error) {
      if (!transferAccepted) lease.release();
      throw error;
    } finally {
      this.activeFetches--;
    }
  }

  private validateServerByteTransfer(
    descriptor: ReturnType<typeof decodeTransferDescriptor>,
    contentKind: number,
  ): void {
    if (
      descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
      descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
      descriptor.contentFamily !== YAS_FAMILY_FONT ||
      descriptor.contentKind !== contentKind ||
      descriptor.contentVersion !== YAS_FONT_VERSION
    )
      throw new YasProtocolError(
        "Font Result returned the wrong Transfer content type",
      );
  }
}

function fontLimitU32(
  extensions: readonly YasExtension[],
  tag: number,
): number {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Font family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u32("Font family limit");
  cursor.end("Font family limit");
  return value;
}

function fontLimitU64(
  extensions: readonly YasExtension[],
  tag: number,
): bigint {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Font family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u64("Font family limit");
  cursor.end("Font family limit");
  return value;
}

function validateFontLimits(value: YasFontLimits): void {
  if (
    value.maxFamilies <= 0 ||
    value.maxFamilies > YAS_FONT_MAX_FAMILIES ||
    value.maxFacesPerFamily <= 0 ||
    value.maxFacesPerFamily > YAS_FONT_MAX_FACES_PER_FAMILY ||
    value.maxDescriptionBytes <= 0n ||
    value.maxDescriptionBytes > BigInt(YAS_FONT_MAX_DESCRIPTION_BYTES) ||
    value.maxFaceBytes <= 0n ||
    value.maxFaceBytes > BigInt(YAS_FONT_MAX_FACE_BYTES) ||
    value.maxConcurrentFetches <= 0 ||
    value.maxConcurrentFetches > YAS_FONT_MAX_CONCURRENT_FETCHES ||
    value.maxScanDurationNs <= 0n ||
    value.maxScanDurationNs > BigInt(YAS_FONT_MAX_SCAN_DURATION_NS) ||
    value.refreshIntervalNs < 0n ||
    value.refreshIntervalNs > BigInt(YAS_FONT_MAX_REFRESH_INTERVAL_NS)
  )
    throw new YasProtocolError("invalid Font family limits");
}

function fontLimit32(tag: number, value: number): YasExtension {
  return { tag, value: new YasWriter().u32(value).finish() };
}

function fontLimit64(tag: number, value: bigint): YasExtension {
  return { tag, value: new YasWriter().u64(value).finish() };
}

/**
 * Small semantic adapter for font pickers that key families by their
 * published name and faces by hash. Native requests still use the exact
 * handle/generation and face handle learned from Font state/DESCRIBE.
 */
export class YasFontProtocol {
  private families = new Map<string, YasFontFamily>();
  private faces = new Map<string, YasFontFace>();

  constructor(readonly client: YasFontClient) {}

  async listFonts(): Promise<readonly YasFontFamily[]> {
    const snapshot = await this.client.listFamilies();
    this.families = new Map(
      snapshot.families.map((family) => [family.family, family]),
    );
    return snapshot.families;
  }

  async describeFont(familyName: string): Promise<YasFontDescription> {
    let family = this.families.get(familyName);
    if (!family) {
      await this.listFonts();
      family = this.families.get(familyName);
    }
    if (!family)
      throw new YasProtocolError(`unknown Font family ${familyName}`);
    const description = await this.client.describe(family);
    for (const face of description.faces)
      this.faces.set(yasFontHashHex(face.contentHash), face);
    return description;
  }

  async fetchFont(contentHash: Uint8Array): Promise<YasFontFaceData> {
    const face = this.faces.get(yasFontHashHex(contentHash));
    if (!face)
      throw new YasProtocolError("Font face was not described before FETCH");
    const result = await this.client.fetch(face);
    return {
      contentHash: result.contentHash,
      format: result.format,
      data: result.bytes,
    };
  }

  dispose(): void {
    this.families.clear();
    this.faces.clear();
    this.client.dispose();
  }
}

export function yasFontHashHex(hash: Uint8Array): string {
  let value = "";
  for (const byte of hash) value += byte.toString(16).padStart(2, "0");
  return value;
}

async function verifyHash(
  hashBytes: YasHashBytes,
  bytes: Uint8Array,
  expected: Uint8Array,
  name: string,
): Promise<void> {
  const actual = await hashBytes(bytes);
  if (actual.length !== 32 || !equalBytes(actual, expected))
    throw new YasProtocolError(`${name} failed BLAKE3 verification`);
}

async function defaultBlake3(bytes: Uint8Array): Promise<Uint8Array> {
  const { blake3_hash } = await import("@yas-run/browser");
  return blake3_hash(bytes);
}
