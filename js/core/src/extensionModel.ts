/** Parse a 64-character BLAKE3 digest. */
export function parseModuleDigest(text: string): Uint8Array | null {
  const trimmed = text.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(trimmed)) return null;
  const bytes = new Uint8Array(32);
  for (let index = 0; index < bytes.length; index++)
    bytes[index] = Number.parseInt(trimmed.slice(index * 2, index * 2 + 2), 16);
  return bytes;
}

/** Fixed-width opaque extension identity used in human-readable surfaces. */
export function formatExtensionId(extensionId: bigint): string {
  return extensionId.toString(16).padStart(16, "0");
}
