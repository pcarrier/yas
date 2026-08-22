/** Default allocation ceiling for one size-prefixed LZ4 block. */
export const LZ4_MAX_DECOMPRESSED = 64 * 1024 * 1024;

/** Decode an lz4_flex `compress_prepend_size` payload. */
export function lz4Decompress(
  data: Uint8Array,
  maxDecompressed = LZ4_MAX_DECOMPRESSED,
): Uint8Array | null {
  if (data.length < 4) return null;
  const declared =
    (data[0] | (data[1] << 8) | (data[2] << 16) | (data[3] << 24)) >>> 0;
  if (declared > maxDecompressed) return null;
  return decompressBlock(data.subarray(4), declared);
}

function decompressBlock(src: Uint8Array, outLen: number): Uint8Array | null {
  const out = new Uint8Array(outLen);
  let source = 0;
  let destination = 0;
  if (outLen === 0) return src.length === 0 || src.length === 1 ? out : null;
  while (source < src.length) {
    const token = src[source++];
    let literalLength = token >> 4;
    if (literalLength === 15) {
      let extension: number;
      do {
        if (source >= src.length) return null;
        extension = src[source++];
        literalLength += extension;
      } while (extension === 255);
    }
    if (
      source + literalLength > src.length ||
      destination + literalLength > outLen
    )
      return null;
    out.set(src.subarray(source, source + literalLength), destination);
    source += literalLength;
    destination += literalLength;
    if (source >= src.length) break;
    if (source + 2 > src.length) return null;
    const offset = src[source] | (src[source + 1] << 8);
    source += 2;
    if (offset === 0 || offset > destination) return null;
    let matchLength = (token & 0x0f) + 4;
    if ((token & 0x0f) === 15) {
      let extension: number;
      do {
        if (source >= src.length) return null;
        extension = src[source++];
        matchLength += extension;
      } while (extension === 255);
    }
    if (destination + matchLength > outLen) return null;
    let match = destination - offset;
    if (offset >= matchLength) {
      out.copyWithin(destination, match, match + matchLength);
      destination += matchLength;
    } else {
      for (let index = 0; index < matchLength; index++)
        out[destination++] = out[match++];
    }
  }
  return destination === outLen ? out : null;
}

/** Encode a valid literal-only size-prefixed LZ4 block. */
export function lz4CompressLiteral(data: Uint8Array): Uint8Array {
  const header: number[] = [
    data.length & 0xff,
    (data.length >> 8) & 0xff,
    (data.length >> 16) & 0xff,
    (data.length >> 24) & 0xff,
  ];
  if (data.length === 0) return new Uint8Array([...header, 0]);
  let rest = data.length;
  if (rest < 15) header.push(rest << 4);
  else {
    header.push(15 << 4);
    rest -= 15;
    while (rest >= 255) {
      header.push(255);
      rest -= 255;
    }
    header.push(rest);
  }
  const out = new Uint8Array(header.length + data.length);
  out.set(header);
  out.set(data, header.length);
  return out;
}

/** Encode a greedy size-prefixed LZ4 block. */
export function lz4Compress(data: Uint8Array): Uint8Array {
  const length = data.length;
  if (length < 13) return lz4CompressLiteral(data);
  const out = new Uint8Array(5 + length + Math.ceil(length / 255) + 16);
  out[0] = length & 0xff;
  out[1] = (length >> 8) & 0xff;
  out[2] = (length >> 16) & 0xff;
  out[3] = (length >> 24) & 0xff;
  let output = 4;
  const hashShift = length < 1 << 16 ? 20 : 16;
  const table = new Int32Array(1 << (32 - hashShift)).fill(-1);
  const read32 = (index: number): number =>
    data[index] |
    (data[index + 1] << 8) |
    (data[index + 2] << 16) |
    (data[index + 3] << 24);
  const matchStartLimit = length - 12;
  const matchEndLimit = length - 5;
  let anchor = 0;
  let source = 0;
  while (source < matchStartLimit) {
    const hash = Math.imul(read32(source), 2654435761) >>> hashShift;
    const reference = table[hash];
    table[hash] = source;
    if (
      reference < 0 ||
      source - reference > 0xffff ||
      read32(reference) !== read32(source)
    ) {
      source++;
      continue;
    }
    let matchLength = 4;
    while (
      source + matchLength < matchEndLimit &&
      data[reference + matchLength] === data[source + matchLength]
    )
      matchLength++;
    const literalLength = source - anchor;
    const tokenAt = output++;
    if (literalLength >= 15) {
      let rest = literalLength - 15;
      for (; rest >= 255; rest -= 255) out[output++] = 255;
      out[output++] = rest;
    }
    out.set(data.subarray(anchor, source), output);
    output += literalLength;
    const offset = source - reference;
    out[output++] = offset & 0xff;
    out[output++] = (offset >> 8) & 0xff;
    if (matchLength - 4 >= 15) {
      let rest = matchLength - 19;
      for (; rest >= 255; rest -= 255) out[output++] = 255;
      out[output++] = rest;
    }
    out[tokenAt] =
      ((literalLength < 15 ? literalLength : 15) << 4) |
      (matchLength - 4 < 15 ? matchLength - 4 : 15);
    source += matchLength;
    anchor = source;
  }
  const literalLength = length - anchor;
  const tokenAt = output++;
  if (literalLength >= 15) {
    let rest = literalLength - 15;
    for (; rest >= 255; rest -= 255) out[output++] = 255;
    out[output++] = rest;
  }
  out.set(data.subarray(anchor), output);
  output += literalLength;
  out[tokenAt] = (literalLength < 15 ? literalLength : 15) << 4;
  const literalOnlyLength =
    5 + length + (length < 15 ? 0 : Math.floor((length - 15) / 255) + 1);
  return output >= literalOnlyLength
    ? lz4CompressLiteral(data)
    : out.subarray(0, output);
}
