// @vitest-environment node
import { createCipheriv, webcrypto } from "node:crypto";
import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it, vi } from "vitest";
import {
  NoiseCipher,
  NoiseDatagrams,
  NoiseInitiator,
  MAX_NONCE,
  REKEY_INTERVAL,
  decodeKey,
  encodeKey,
  generateUplinkKeyPair,
  importIdentity,
  publicKey,
  uplinkPublicKey,
} from "../transports/uplink-crypto";

beforeAll(() => {
  vi.stubGlobal("crypto", webcrypto);
});
const fromHex = (s: string) => new Uint8Array(Buffer.from(s, "hex"));
const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");
const vector = JSON.parse(
  readFileSync(
    new URL(
      "../../../../crates/uplink/test-vectors/noise-ik.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

describe("browser uplink crypto", () => {
  it("matches the upstream Cacophony handshake and transport vectors", async () => {
    const identity = await importIdentity(fromHex(vector.init_static));
    const ephemeral = await importIdentity(fromHex(vector.init_ephemeral));
    const handshake = await NoiseInitiator.initialize(
      identity,
      fromHex(vector.init_remote_static),
      ephemeral,
      fromHex(vector.init_prologue),
    );
    expect(
      hex(await handshake.first(fromHex(vector.messages[0].payload))),
    ).toBe(vector.messages[0].ciphertext);
    const { send, receive, payload, hash } = await handshake.finish(
      fromHex(vector.messages[1].ciphertext),
    );
    expect(hex(payload)).toBe(vector.messages[1].payload);
    expect(hex(hash)).toBe(vector.handshake_hash);
    for (let i = 2; i < vector.messages.length; i++) {
      const msg = vector.messages[i];
      if (i % 2 === 0)
        expect(hex(await send.encrypt(fromHex(msg.payload)))).toBe(
          msg.ciphertext,
        );
      else
        expect(hex(await receive.decrypt(fromHex(msg.ciphertext)))).toBe(
          msg.payload,
        );
    }
    await expect(handshake.first()).rejects.toThrow();
    await expect(
      handshake.finish(fromHex(vector.messages[1].ciphertext)),
    ).rejects.toThrow();
  });

  it("keeps compact key inputs and derives the native X25519 public pin", async () => {
    const pair = await generateUplinkKeyPair();
    expect(pair.privateKey).toHaveLength(43);
    expect(pair.publicKey).toHaveLength(43);
    expect(await uplinkPublicKey(pair.privateKey)).toBe(pair.publicKey);
    expect(
      await uplinkPublicKey("3iAcr2-GUIpQfQsOaabe-eD6uK8O53exaB9pKnVfotI"),
    ).toBe("XRb2vVZJepyosoYqoXX24-lMYpkj9SekAq8ViT7RC1Q");
    for (const value of [
      "",
      "A".repeat(42),
      "A".repeat(44),
      "A".repeat(42) + "B",
      pair.privateKey + "=",
      "/tmp/key",
    ])
      expect(() => decodeKey(value)).toThrow();
    expect(() => publicKey("A".repeat(43))).toThrow();
    expect(() => publicKey(encodeKey(new Uint8Array(32).fill(255)))).toThrow();
  });

  it("authenticates datagrams independently across loss, reordering, and duplicate races", async () => {
    const material = Uint8Array.from({ length: 64 }, (_, i) => i);
    const token = new Uint8Array(16).fill(7);
    const sender = new NoiseDatagrams(material, token);
    const receiver = new NoiseDatagrams(material, token, false);
    const packets = await Promise.all(
      [0, 1, 2, 3].map((i) => sender.seal(new Uint8Array([i]))),
    );
    const forged = packets[3]!.slice();
    forged[0] ^= 128;
    expect(await receiver.open(forged)).toBeNull();
    expect(await receiver.open(packets[2]!)).toEqual(new Uint8Array([2]));
    expect(await receiver.open(packets[0]!)).toEqual(new Uint8Array([0]));
    const duplicates = await Promise.all([
      receiver.open(packets[1]!),
      receiver.open(packets[1]!),
    ]);
    expect(duplicates.filter(Boolean)).toHaveLength(1);
    expect(await receiver.open(packets[3]!)).toEqual(new Uint8Array([3]));
    expect(await receiver.open(packets[2]!)).toBeNull();
    expect(
      await new NoiseDatagrams(material, token).open(packets[0]!),
    ).toBeNull();
    expect(
      await new NoiseDatagrams(
        material,
        new Uint8Array(16).fill(8),
        false,
      ).open(packets[0]!),
    ).toBeNull();
    expect(
      await new NoiseDatagrams(new Uint8Array(64), token, false).open(
        packets[0]!,
      ),
    ).toBeNull();
    sender.close();
    receiver.close();
    expect(await sender.seal(new Uint8Array([1]))).toBeNull();
    expect(await receiver.open(packets[0]!)).toBeNull();
  });
});

it("matches native/OpenSSL datagram vectors, including reordered epochs", async () => {
  const v = JSON.parse(
    readFileSync(
      new URL(
        "../../../../crates/uplink/test-vectors/datagrams.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  for (const direction of [0, 1]) {
    const packets = v.packets.filter(
      (p: { direction: number }) => p.direction === direction,
    );
    const receiver = new NoiseDatagrams(
      fromHex(v.material),
      fromHex(v.token),
      direction !== 0,
    );
    // Receive a later epoch before the last packet of the previous epoch.
    for (const i of [0, 1, 3, 2, 4, 5]) {
      const packet = packets[i];
      expect(await receiver.open(fromHex(packet.ciphertext))).toEqual(
        fromHex(v.plaintext),
      );
    }
    for (const packet of packets) {
      const sender = new NoiseDatagrams(
        fromHex(v.material),
        fromHex(v.token),
        direction === 0,
      );
      // Exercise nonce/epoch boundaries without encrypting a million packets.
      Reflect.set(sender, "sendCounter", BigInt(packet.counter));
      expect(hex((await sender.seal(fromHex(v.plaintext)))!)).toBe(
        packet.ciphertext,
      );
    }
  }
});

it("matches OpenSSL across a reliable rekey and rejects nonce exhaustion", async () => {
  const root = Uint8Array.from({ length: 32 }, (_, i) => i);
  const key = await crypto.subtle.importKey("raw", root, "AES-GCM", false, [
    "encrypt",
    "decrypt",
  ]);
  const sender = new NoiseCipher(key),
    receiver = new NoiseCipher(key);
  sender.counter = receiver.counter = REKEY_INTERVAL - 1n;
  const encrypt = (key: Uint8Array, counter: bigint, bytes: Uint8Array) => {
    const nonce = Buffer.alloc(12);
    nonce.writeBigUInt64BE(counter, 4);
    const cipher = createCipheriv("aes-256-gcm", key, nonce);
    return Buffer.concat([
      cipher.update(bytes),
      cipher.final(),
      cipher.getAuthTag(),
    ]);
  };
  const nextKey = encrypt(root, MAX_NONCE, new Uint8Array(32)).subarray(0, 32);
  for (const [counter, key] of [
    [REKEY_INTERVAL - 1n, root],
    [REKEY_INTERVAL, nextKey],
  ] as const) {
    const plaintext = new Uint8Array([0, 42]);
    const packet = await sender.encrypt(plaintext);
    expect(Buffer.from(packet)).toEqual(encrypt(key, counter, plaintext));
    expect(await receiver.decrypt(packet)).toEqual(plaintext);
  }
  sender.counter = receiver.counter = MAX_NONCE;
  await expect(sender.encrypt(new Uint8Array([0]))).rejects.toThrow(
    /exhausted/,
  );
  await expect(receiver.decrypt(new Uint8Array(17))).rejects.toThrow(
    /exhausted/,
  );
});
