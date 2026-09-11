/** Noise IK / AES-GCM primitives. The browser owns every cryptographic operation.
 * Wire constants and shared vectors: crates/uplink, docs/uplink.md.
 * This module is internal; the public transport serializes reliable operations. */
export type Bytes = Uint8Array<ArrayBuffer>;
export const NOISE_PROTOCOL = "Noise_IK_25519_AESGCM_SHA256";
export const PROLOGUE = new TextEncoder().encode("YAS-UPLINK\x02");
export const REKEY_INTERVAL = 1n << 20n;
export const MAX_NONCE = (1n << 64n) - 1n;
export const MAX_PLAINTEXT = 16 * 1024;
export const DATAGRAM_OVERHEAD = 24;
const EMPTY = new Uint8Array(0);
const KEY_LABEL = new TextEncoder().encode("YAS-UPLINK-v2-datagram");

export function concat(...parts: Uint8Array[]): Bytes {
  const result = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const part of parts) {
    result.set(part, offset);
    offset += part.length;
  }
  return result;
}
export function equal(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}
export function encodeKey(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}
export function decodeKey(value: string): Bytes {
  if (!/^[A-Za-z0-9_-]{43}$/.test(value))
    throw new Error("X25519 key must be 43 characters of unpadded base64url");
  const bytes = Uint8Array.from(
    atob(value.replace(/-/g, "+").replace(/_/g, "/") + "="),
    (c) => c.charCodeAt(0),
  );
  if (encodeKey(bytes) !== value)
    throw new Error("X25519 key is not canonical base64url");
  return bytes;
}
export function publicKey(value: string): Bytes {
  const key = decodeKey(value);
  const prime = new Uint8Array(32).fill(255);
  prime[0] = 237;
  prime[31] = 127;
  let comparison = 0;
  for (let i = 31; i >= 0 && comparison === 0; i--)
    comparison = key[i]! - prime[i]!;
  if (comparison >= 0 || key.every((b) => b === 0))
    throw new Error("invalid X25519 public key");
  return key;
}
export interface NoiseIdentity {
  privateKey: CryptoKey;
  publicKey: Bytes;
}

export async function importIdentity(seed: Bytes): Promise<NoiseIdentity> {
  // RFC 8410 PKCS#8 is only an in-memory bridge into WebCrypto. No key files.
  const der = concat(
    new Uint8Array([
      0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e,
      0x04, 0x22, 0x04, 0x20,
    ]),
    seed,
  );
  try {
    const temporary = await crypto.subtle.importKey(
      "pkcs8",
      der,
      "X25519",
      true,
      ["deriveBits"],
    );
    const jwk = await crypto.subtle.exportKey("jwk", temporary);
    if (!jwk.x) throw new Error("X25519 public key unavailable");
    const privateKey = await crypto.subtle.importKey(
      "pkcs8",
      der,
      "X25519",
      false,
      ["deriveBits"],
    );
    return { privateKey, publicKey: publicKey(jwk.x) };
  } finally {
    der.fill(0);
  }
}
async function generateIdentity(): Promise<NoiseIdentity> {
  const pair = (await crypto.subtle.generateKey("X25519", false, [
    "deriveBits",
  ])) as CryptoKeyPair;
  return {
    privateKey: pair.privateKey,
    publicKey: new Uint8Array(
      await crypto.subtle.exportKey("raw", pair.publicKey),
    ),
  };
}
/** Generate once, then persist the private value in the application's secret storage. */
export async function generateUplinkKeyPair(): Promise<{
  privateKey: string;
  publicKey: string;
}> {
  const pair = (await crypto.subtle.generateKey("X25519", true, [
    "deriveBits",
  ])) as CryptoKeyPair;
  const der = new Uint8Array(
    await crypto.subtle.exportKey("pkcs8", pair.privateKey),
  );
  try {
    return {
      privateKey: encodeKey(der.subarray(der.length - 32)),
      publicKey: encodeKey(
        new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey)),
      ),
    };
  } finally {
    der.fill(0);
  }
}
export async function uplinkPublicKey(privateKey: string): Promise<string> {
  const seed = decodeKey(privateKey);
  try {
    return encodeKey((await importIdentity(seed)).publicKey);
  } finally {
    seed.fill(0);
  }
}
async function dh(privateKey: CryptoKey, publicBytes: Bytes): Promise<Bytes> {
  const publicKey = await crypto.subtle.importKey(
    "raw",
    publicBytes,
    "X25519",
    false,
    [],
  );
  const shared = new Uint8Array(
    await crypto.subtle.deriveBits(
      { name: "X25519", public: publicKey },
      privateKey,
      256,
    ),
  );
  if (shared.every((b) => b === 0)) throw new Error("invalid X25519 agreement");
  return shared;
}
async function hmac(key: Bytes, data: Bytes): Promise<Bytes> {
  const k = await crypto.subtle.importKey(
    "raw",
    key,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return new Uint8Array(await crypto.subtle.sign("HMAC", k, data));
}
async function hkdf(salt: Bytes, input: Bytes): Promise<[Bytes, Bytes]> {
  const prk = await hmac(salt, input);
  try {
    const first = await hmac(prk, new Uint8Array([1]));
    return [first, await hmac(prk, concat(first, new Uint8Array([2])))];
  } finally {
    prk.fill(0);
  }
}
async function aesKey(bytes: Bytes): Promise<CryptoKey> {
  return crypto.subtle.importKey("raw", bytes, "AES-GCM", false, [
    "encrypt",
    "decrypt",
  ]);
}
function nonce(counter: bigint): Bytes {
  const iv = new Uint8Array(12);
  new DataView(iv.buffer).setBigUint64(4, counter);
  return iv;
}
function aesParams(counter: bigint, aad: Bytes) {
  return {
    name: "AES-GCM",
    iv: nonce(counter),
    additionalData: aad,
    tagLength: 128,
  };
}

/** One direction of an ordered Noise stream. Caller serializes operations. */
export class NoiseCipher {
  counter = 0n;
  constructor(private key: CryptoKey) {}
  private async rekey(): Promise<void> {
    const bytes = new Uint8Array(
      await crypto.subtle.encrypt(
        aesParams(MAX_NONCE, EMPTY),
        this.key,
        new Uint8Array(32),
      ),
    );
    try {
      this.key = await aesKey(bytes.subarray(0, 32));
    } finally {
      bytes.fill(0);
    }
  }
  async encrypt(plaintext: Bytes): Promise<Bytes> {
    if (this.counter >= MAX_NONCE) throw new Error("Noise counter exhausted");
    const result = new Uint8Array(
      await crypto.subtle.encrypt(
        aesParams(this.counter++, EMPTY),
        this.key,
        plaintext,
      ),
    );
    if (this.counter % REKEY_INTERVAL === 0n) await this.rekey();
    return result;
  }
  async decrypt(ciphertext: Bytes): Promise<Bytes> {
    if (this.counter >= MAX_NONCE) throw new Error("Noise counter exhausted");
    const result = new Uint8Array(
      await crypto.subtle.decrypt(
        aesParams(this.counter, EMPTY),
        this.key,
        ciphertext,
      ),
    );
    this.counter++;
    if (this.counter % REKEY_INTERVAL === 0n) await this.rekey();
    return result;
  }
}

/** IK initiator; production always generates a fresh ephemeral key. */
export class NoiseInitiator {
  private h: Bytes;
  private ck: Bytes;
  private key: CryptoKey | null = null;
  private n = 0n;
  private stage = 0;
  private constructor(
    private identity: NoiseIdentity,
    private remote: Bytes,
    private ephemeral: NoiseIdentity,
  ) {
    this.h = new Uint8Array(32);
    this.h.set(new TextEncoder().encode(NOISE_PROTOCOL));
    this.ck = this.h.slice();
  }
  static async create(
    identity: NoiseIdentity,
    remote: Bytes,
  ): Promise<NoiseInitiator> {
    return this.initialize(
      identity,
      remote,
      await generateIdentity(),
      PROLOGUE,
    );
  }
  /** Internal deterministic entry point for the upstream interoperability vector. */
  static async initialize(
    identity: NoiseIdentity,
    remote: Bytes,
    ephemeral: NoiseIdentity,
    prologue: Bytes,
  ): Promise<NoiseInitiator> {
    const state = new NoiseInitiator(identity, remote, ephemeral);
    await state.mixHash(prologue);
    await state.mixHash(remote);
    return state;
  }
  private async mixHash(bytes: Bytes): Promise<void> {
    this.h = new Uint8Array(
      await crypto.subtle.digest("SHA-256", concat(this.h, bytes)),
    );
  }
  private async mixKey(shared: Bytes): Promise<void> {
    const [ck, key] = await hkdf(this.ck, shared);
    this.ck.fill(0);
    shared.fill(0);
    this.ck = ck;
    try {
      this.key = await aesKey(key);
    } finally {
      key.fill(0);
    }
    this.n = 0n;
  }
  private async encrypt(bytes: Bytes): Promise<Bytes> {
    if (!this.key) throw new Error("Noise key missing");
    const result = new Uint8Array(
      await crypto.subtle.encrypt(aesParams(this.n++, this.h), this.key, bytes),
    );
    await this.mixHash(result);
    return result;
  }
  async first(payload: Bytes = EMPTY): Promise<Bytes> {
    if (this.stage++ !== 0) throw new Error("invalid Noise handshake state");
    const e = this.ephemeral.publicKey;
    await this.mixHash(e);
    await this.mixKey(await dh(this.ephemeral.privateKey, this.remote));
    const s = await this.encrypt(this.identity.publicKey);
    await this.mixKey(await dh(this.identity.privateKey, this.remote));
    return concat(e, s, await this.encrypt(payload));
  }
  async finish(message: Bytes): Promise<{
    send: NoiseCipher;
    receive: NoiseCipher;
    hash: Bytes;
    payload: Bytes;
  }> {
    if (this.stage++ !== 1 || message.length < 48)
      throw new Error("invalid Noise handshake response");
    const e = message.slice(0, 32);
    await this.mixHash(e);
    await this.mixKey(await dh(this.ephemeral.privateKey, e));
    await this.mixKey(await dh(this.identity.privateKey, e));
    const encrypted = message.slice(32);
    const payload = new Uint8Array(
      await crypto.subtle.decrypt(
        aesParams(this.n++, this.h),
        this.key!,
        encrypted,
      ),
    );
    await this.mixHash(encrypted);
    const [send, receive] = await hkdf(this.ck, EMPTY);
    this.ck.fill(0);
    this.key = null;
    try {
      return {
        send: new NoiseCipher(await aesKey(send)),
        receive: new NoiseCipher(await aesKey(receive)),
        hash: this.h.slice(),
        payload,
      };
    } finally {
      send.fill(0);
      receive.fill(0);
    }
  }
}

/** Independent datagrams with authenticated routing and a 128-packet replay window. */
export class NoiseDatagrams {
  private sendCounter = 0n;
  private highest: bigint | null = null;
  private window = 0n;
  private readonly sendRoot: Bytes;
  private readonly receiveRoot: Bytes;
  private sendKey: { epoch: bigint; key: CryptoKey } | null = null;
  private receiveKey: { epoch: bigint; key: CryptoKey } | null = null;
  private disposed = false;
  constructor(
    material: Bytes,
    private token: Bytes,
    private client = true,
  ) {
    if (material.length !== 64 || token.length !== 16)
      throw new Error("invalid datagram configuration");
    this.token = token.slice();
    this.sendRoot = material.slice(client ? 0 : 32, client ? 32 : 64);
    this.receiveRoot = material.slice(client ? 32 : 0, client ? 64 : 32);
  }
  private async derive(
    root: Bytes,
    epoch: bigint,
    direction: number,
  ): Promise<CryptoKey> {
    const epochBytes = new Uint8Array(8);
    new DataView(epochBytes.buffer).setBigUint64(0, epoch);
    const key = await crypto.subtle.importKey("raw", root, "HKDF", false, [
      "deriveKey",
    ]);
    return crypto.subtle.deriveKey(
      {
        name: "HKDF",
        hash: "SHA-256",
        salt: this.token,
        info: concat(KEY_LABEL, new Uint8Array([direction]), epochBytes),
      },
      key,
      { name: "AES-GCM", length: 256 },
      false,
      ["encrypt", "decrypt"],
    );
  }
  async seal(plaintext: Bytes): Promise<Bytes | null> {
    if (
      this.disposed ||
      plaintext.length > 65536 - DATAGRAM_OVERHEAD ||
      this.sendCounter > MAX_NONCE
    )
      return null;
    const counter = this.sendCounter++; // reserve before any await
    const epoch = counter / REKEY_INTERVAL;
    const key =
      this.sendKey?.epoch === epoch
        ? this.sendKey.key
        : await this.derive(this.sendRoot, epoch, this.client ? 0 : 1);
    if (this.disposed) return null;
    this.sendKey = { epoch, key };
    const sequence = new Uint8Array(8);
    new DataView(sequence.buffer).setBigUint64(0, counter);
    const cipher = new Uint8Array(
      await crypto.subtle.encrypt(
        aesParams(counter, concat(this.token, sequence)),
        key,
        plaintext,
      ),
    );
    return this.disposed ? null : concat(sequence, cipher);
  }
  private acceptable(counter: bigint): boolean {
    if (this.highest === null || counter > this.highest) return true;
    const behind = this.highest - counter;
    return behind < 128n && (this.window & (1n << behind)) === 0n;
  }
  async open(packet: Bytes): Promise<Bytes | null> {
    if (
      this.disposed ||
      packet.length < DATAGRAM_OVERHEAD ||
      packet.length > 65536
    )
      return null;
    const counter = new DataView(
      packet.buffer,
      packet.byteOffset,
      packet.byteLength,
    ).getBigUint64(0);
    if (!this.acceptable(counter)) return null;
    const epoch = counter / REKEY_INTERVAL;
    try {
      const key =
        this.receiveKey?.epoch === epoch
          ? this.receiveKey.key
          : await this.derive(this.receiveRoot, epoch, this.client ? 1 : 0);
      const plaintext = new Uint8Array(
        await crypto.subtle.decrypt(
          aesParams(counter, concat(this.token, packet.subarray(0, 8))),
          key,
          packet.subarray(8),
        ),
      );
      // Recheck after asynchronous authentication: concurrent duplicates must
      // not both pass, and a forgery must not move the replay window.
      if (this.disposed || !this.acceptable(counter)) return null;
      this.receiveKey = { epoch, key };
      if (this.highest !== null && counter <= this.highest)
        this.window |= 1n << (this.highest - counter);
      else {
        const shift = this.highest === null ? 128n : counter - this.highest;
        this.window =
          shift >= 128n
            ? 1n
            : ((this.window << shift) | 1n) & ((1n << 128n) - 1n);
        this.highest = counter;
      }
      return plaintext;
    } catch {
      return null;
    }
  }
  close(): void {
    this.disposed = true;
    this.sendRoot.fill(0);
    this.receiveRoot.fill(0);
    this.sendKey = this.receiveKey = null;
  }
}
