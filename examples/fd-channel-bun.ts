#!/usr/bin/env bun
// Pass a native YAS client stream to the server through YAS_FD_CHANNEL.
// SCM_RIGHTS transports an accepted stream; the stream itself uses the same
// native YAS preface, HELLO, frames, and correlation as every local client.

import { spawn } from "bun";
import { dlopen, FFIType, ptr } from "bun:ffi";

const YAS_SERVER = process.env.YAS_SERVER ?? "yas";
const DARWIN = process.platform === "darwin";
const libc = dlopen(DARWIN ? "libSystem.B.dylib" : "libc.so.6", {
  socketpair: {
    args: [FFIType.i32, FFIType.i32, FFIType.i32, FFIType.ptr],
    returns: FFIType.i32,
  },
  sendmsg: {
    args: [FFIType.i32, FFIType.ptr, FFIType.i32],
    returns: FFIType.i64,
  },
  close: { args: [FFIType.i32], returns: FFIType.i32 },
  read: { args: [FFIType.i32, FFIType.ptr, FFIType.u64], returns: FFIType.i64 },
  write: {
    args: [FFIType.i32, FFIType.ptr, FFIType.u64],
    returns: FFIType.i64,
  },
});

const AF_UNIX = 1;
const SOCK_STREAM = 1;
const SCM_RIGHTS = 1;
const SOL_SOCKET = DARWIN ? 0xffff : 1;
const SIGTERM = 15;
const SIGKILL = 9;
const PREFACE = Uint8Array.from([0x59, 0x41, 0x53, 0, 1, 0, 0x0d, 0x0a]);
const CORE = 0;
const HELLO = 0;
const SESSION_INFO = 3;
const CLASS_REQUEST = 1;
const CLASS_RESULT = 2;
const META_COMPRESSED = 4;
const RECOMMENDED_WIRE_FRAME = 1_048_576;
const RECOMMENDED_DECODED_FRAME = 4_194_304;
const RECOMMENDED_BUFFERED = 16_777_216n;

// Linux: CMSG_LEN=20, CMSG_SPACE=24, fd offset=16, msghdr=56.
// Darwin: CMSG_LEN=16, CMSG_SPACE=16, fd offset=12, msghdr=48.
const CMSG_LEN = DARWIN ? 16 : 20;
const CMSG_SPACE = DARWIN ? 16 : 24;
const CMSG_FD_OFF = DARWIN ? 12 : 16;
const MSGHDR_SIZE = DARWIN ? 48 : 56;

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function socketpair(): [number, number] {
  const fds = new Int32Array(2);
  if (libc.symbols.socketpair(AF_UNIX, SOCK_STREAM, 0, ptr(fds)) < 0)
    throw new Error("socketpair failed");
  return [fds[0]!, fds[1]!];
}

function sendFd(channel: number, clientFd: number) {
  const iovByte = new Uint8Array(1);
  const iov = new BigUint64Array([BigInt(ptr(iovByte)), 1n]);
  const control = new DataView(new ArrayBuffer(CMSG_SPACE));
  if (DARWIN) {
    control.setUint32(0, CMSG_LEN, true);
    control.setUint32(4, SOL_SOCKET, true);
    control.setUint32(8, SCM_RIGHTS, true);
  } else {
    control.setBigUint64(0, BigInt(CMSG_LEN), true);
    control.setUint32(8, SOL_SOCKET, true);
    control.setUint32(12, SCM_RIGHTS, true);
  }
  control.setInt32(CMSG_FD_OFF, clientFd, true);

  const message = new DataView(new ArrayBuffer(MSGHDR_SIZE));
  const iovPtr = BigInt(ptr(new Uint8Array(iov.buffer)));
  const controlPtr = BigInt(ptr(new Uint8Array(control.buffer)));
  message.setBigUint64(16, iovPtr, true);
  if (DARWIN) {
    message.setUint32(24, 1, true);
    message.setBigUint64(32, controlPtr, true);
    message.setUint32(40, CMSG_SPACE, true);
  } else {
    message.setBigUint64(24, 1n, true);
    message.setBigUint64(32, controlPtr, true);
    message.setBigUint64(40, BigInt(CMSG_SPACE), true);
  }
  const result = libc.symbols.sendmsg(
    channel,
    ptr(new Uint8Array(message.buffer)),
    0,
  );
  if (Number(result) < 0) throw new Error("sendmsg failed");
}

function readExact(fd: number, size: number): Uint8Array {
  const data = new Uint8Array(size);
  let offset = 0;
  while (offset < size) {
    const count = Number(
      libc.symbols.read(fd, ptr(data.subarray(offset)), BigInt(size - offset)),
    );
    if (count <= 0) throw new Error(`native YAS stream read returned ${count}`);
    offset += count;
  }
  return data;
}

function writeAll(fd: number, data: Uint8Array) {
  let offset = 0;
  while (offset < data.length) {
    const count = Number(
      libc.symbols.write(
        fd,
        ptr(data.subarray(offset)),
        BigInt(data.length - offset),
      ),
    );
    if (count <= 0)
      throw new Error(`native YAS stream write returned ${count}`);
    offset += count;
  }
}

function readFrame(fd: number): Uint8Array {
  const lengthBytes = readExact(fd, 4);
  const length = new DataView(lengthBytes.buffer).getUint32(0, true);
  assert(
    length >= 5 && length <= RECOMMENDED_WIRE_FRAME,
    `invalid frame length ${length}`,
  );
  return readExact(fd, length);
}

function writeFrame(fd: number, frame: Uint8Array) {
  assert(
    frame.length <= RECOMMENDED_WIRE_FRAME,
    "frame exceeds advertised limit",
  );
  const output = new Uint8Array(4 + frame.length);
  new DataView(output.buffer).setUint32(0, frame.length, true);
  output.set(frame, 4);
  writeAll(fd, output);
}

function request(
  kind: number,
  requestId: number,
  payload = new Uint8Array(),
): Uint8Array {
  assert(requestId !== 0, "YAS request IDs are nonzero");
  const frame = new Uint8Array(9 + payload.length);
  const view = new DataView(frame.buffer);
  view.setUint16(0, CORE, true);
  view.setUint16(2, kind, true);
  view.setUint8(4, CLASS_REQUEST);
  view.setUint32(5, requestId, true);
  frame.set(payload, 9);
  return frame;
}

function readResult(
  fd: number,
  kind: number,
  requestId: number,
  limit = 32,
): Uint8Array {
  for (let index = 0; index < limit; index++) {
    const frame = readFrame(fd);
    assert(frame.length >= 5, "truncated YAS frame header");
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    const family = view.getUint16(0, true);
    const frameKind = view.getUint16(2, true);
    const meta = view.getUint8(4);
    assert(
      (meta & META_COMPRESSED) === 0,
      "example did not negotiate compression",
    );
    if ((meta & 3) === CLASS_RESULT) {
      assert(frame.length >= 9, "truncated correlated YAS header");
      const correlated = view.getUint32(5, true);
      if (family === CORE && frameKind === kind && correlated === requestId)
        return frame.subarray(9);
    }
  }
  throw new Error(`no Core Result ${kind}/${requestId} within ${limit} frames`);
}

function stringU16(value: string): Uint8Array {
  const encoded = new TextEncoder().encode(value);
  assert(encoded.length <= 0xffff, "YAS string_u16 overflow");
  const output = new Uint8Array(2 + encoded.length);
  new DataView(output.buffer).setUint16(0, encoded.length, true);
  output.set(encoded, 2);
  return output;
}

function clientHello(): Uint8Array {
  const name = stringU16("fd-channel-bun");
  const release = stringU16("example");
  const output = new Uint8Array(47 + name.length + release.length);
  const view = new DataView(output.buffer);
  let offset = 0;
  view.setUint16(offset, 1, true);
  offset += 2;
  view.setUint16(offset, 1, true);
  offset += 2;
  view.setUint32(offset, RECOMMENDED_WIRE_FRAME, true);
  offset += 4;
  view.setUint32(offset, RECOMMENDED_DECODED_FRAME, true);
  offset += 4;
  view.setUint32(offset, 0, true); // No local datagram sideband.
  offset += 4;
  view.setBigUint64(offset, RECOMMENDED_BUFFERED, true);
  offset += 8;
  crypto.getRandomValues(output.subarray(offset, offset + 16));
  offset += 16;
  output.set(name, offset);
  offset += name.length;
  output.set(release, offset);
  offset += release.length;
  view.setUint16(offset, 0, true); // No optional family offers.
  offset += 2;
  view.setUint8(offset, 0); // No packed codecs.
  offset += 1;
  view.setUint32(offset, 0, true); // No ClientHello extensions.
  return output;
}

function resultBody(payload: Uint8Array): Uint8Array {
  assert(payload.length >= 8, "truncated YAS Result prefix");
  const view = new DataView(
    payload.buffer,
    payload.byteOffset,
    payload.byteLength,
  );
  const status = view.getUint16(0, true);
  const flags = view.getUint16(2, true);
  const detailLength = view.getUint32(4, true);
  assert(
    flags === 0 && 8 + detailLength <= payload.length,
    "invalid YAS Result prefix",
  );
  assert(status === 0, `YAS request failed with status ${status}`);
  return payload.subarray(8 + detailLength);
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

const [channelTheirs, channelOurs] = socketpair();
const server = spawn([YAS_SERVER, "server"], {
  env: { ...process.env, YAS_FD_CHANNEL: "3" },
  stdio: ["inherit", "inherit", "inherit", Bun.file(channelTheirs)],
});
libc.symbols.close(channelTheirs);

const [clientOurs, clientTheirs] = socketpair();
sendFd(channelOurs, clientTheirs);
libc.symbols.close(clientTheirs);

try {
  writeAll(clientOurs, PREFACE);
  writeFrame(clientOurs, request(HELLO, 1, clientHello()));
  const hello = resultBody(readResult(clientOurs, HELLO, 1));
  assert(hello.length >= 56, "truncated YAS ServerHello");
  const helloView = new DataView(
    hello.buffer,
    hello.byteOffset,
    hello.byteLength,
  );
  const minor = helloView.getUint16(0, true);
  assert(
    helloView.getUint16(2, true) === 0,
    "invalid ServerHello reserved field",
  );
  const boot = hello.subarray(4, 20);
  const session = hello.subarray(20, 36);
  const maxFrame = helloView.getUint32(36, true);
  const maxDecoded = helloView.getUint32(40, true);
  const maxDatagram = helloView.getUint32(44, true);
  const maxBuffered = helloView.getBigUint64(48, true);
  console.log(
    `HELLO: minor=${minor} boot=${hex(boot)} session=${hex(session)} ` +
      `receive=${maxFrame}/${maxDecoded}/${maxDatagram}/${maxBuffered}`,
  );

  writeFrame(clientOurs, request(SESSION_INFO, 2));
  const info = resultBody(readResult(clientOurs, SESSION_INFO, 2));
  assert(info.length >= 54, "truncated YAS SessionInfo");
  assert(
    hex(info.subarray(0, 16)) === hex(session),
    "SESSION_INFO changed session ID",
  );
  const infoView = new DataView(info.buffer, info.byteOffset, info.byteLength);
  console.log(
    `SESSION_INFO: revision=${infoView.getBigUint64(16, true)} ` +
      `families=${infoView.getUint16(52, true)}`,
  );
  console.log("PASS");
} finally {
  libc.symbols.close(clientOurs);
  libc.symbols.close(channelOurs);
  server.kill(SIGTERM);
  let forced = false;
  const timeout = setTimeout(() => {
    forced = true;
    server.kill(SIGKILL);
  }, 5_000);
  try {
    await server.exited;
  } finally {
    clearTimeout(timeout);
  }
  assert(!forced, "YAS test server did not stop after SIGTERM");
}
