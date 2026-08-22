#!/usr/bin/env python3
"""Pass a native Yas client stream to the server through YAS_FD_CHANNEL.

SCM_RIGHTS transports an already accepted byte stream; the bytes on that
stream are ordinary native Yas. This example implements the small Core
handshake directly to make that boundary explicit.
"""

import os
import signal
import socket
import struct
import subprocess
import tempfile

YAS_SERVER = os.environ.get("YAS_SERVER", "yas")

PREFACE = bytes.fromhex("5941530001000d0a")
CORE = 0
HELLO = 0
SESSION_INFO = 3
CLASS_REQUEST = 1
CLASS_RESULT = 2
META_COMPRESSED = 4
RECOMMENDED_WIRE_FRAME = 1_048_576
RECOMMENDED_DECODED_FRAME = 4_194_304
RECOMMENDED_BUFFERED = 16_777_216


def read_exact(sock, size):
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise ConnectionError("native Yas stream closed")
        data.extend(chunk)
    return bytes(data)


def read_frame(sock):
    length = int.from_bytes(read_exact(sock, 4), "little")
    if length < 5 or length > RECOMMENDED_WIRE_FRAME:
        raise ValueError(f"invalid Yas frame length {length}")
    return read_exact(sock, length)


def write_frame(sock, frame):
    if len(frame) > RECOMMENDED_WIRE_FRAME:
        raise ValueError("Yas frame exceeds the advertised receive limit")
    sock.sendall(struct.pack("<I", len(frame)) + frame)


def request(kind, request_id, payload=b""):
    if request_id == 0:
        raise ValueError("Yas request IDs are nonzero")
    return struct.pack("<HHBI", CORE, kind, CLASS_REQUEST, request_id) + payload


def read_result(sock, kind, request_id, limit=32):
    """Find one correlated Core Result while tolerating Core Events."""
    for _ in range(limit):
        frame = read_frame(sock)
        if len(frame) < 5:
            raise ValueError("truncated Yas frame header")
        family, frame_kind, meta = struct.unpack_from("<HHB", frame)
        if meta & META_COMPRESSED:
            raise ValueError("example did not negotiate compression")
        if meta & 3 == CLASS_RESULT:
            if len(frame) < 9:
                raise ValueError("truncated correlated Yas header")
            correlated = struct.unpack_from("<I", frame, 5)[0]
            if family == CORE and frame_kind == kind and correlated == request_id:
                return frame[9:]
    raise AssertionError(f"no Core Result {kind}/{request_id} within {limit} frames")


def encode_string_u16(value):
    encoded = value.encode("utf-8")
    if len(encoded) > 0xFFFF:
        raise ValueError("Yas string_u16 overflow")
    return struct.pack("<H", len(encoded)) + encoded


def client_hello():
    return b"".join(
        [
            struct.pack("<HH", 1, 1),
            struct.pack(
                "<IIIQ",
                RECOMMENDED_WIRE_FRAME,
                RECOMMENDED_DECODED_FRAME,
                0,  # This local stream has no datagram sideband.
                RECOMMENDED_BUFFERED,
            ),
            os.urandom(16),
            encode_string_u16("fd-channel-python"),
            encode_string_u16("example"),
            struct.pack("<H", 0),  # No optional family offers.
            b"\x00",  # No packed codecs.
            struct.pack("<I", 0),  # No ClientHello extensions.
        ]
    )


def decode_result_prefix(payload):
    if len(payload) < 8:
        raise ValueError("truncated Yas Result prefix")
    status, flags, detail_len = struct.unpack_from("<HHI", payload)
    if flags != 0 or 8 + detail_len > len(payload):
        raise ValueError("invalid Yas Result prefix")
    if status != 0:
        raise RuntimeError(f"Yas request failed with status {status}")
    return payload[8 + detail_len :]


def decode_server_hello(body):
    # Decode only the fixed prefix needed by this example. Applications use
    # the generated codecs for the complete negotiated family catalogue.
    if len(body) < 80:
        raise ValueError("truncated Yas ServerHello")
    minor, reserved = struct.unpack_from("<HH", body)
    if reserved != 0:
        raise ValueError("invalid ServerHello reserved field")
    boot_id = body[4:20]
    session_id = body[20:36]
    receive = struct.unpack_from("<IIIQ", body, 36)
    return minor, boot_id, session_id, receive


def decode_session_info(body):
    if len(body) < 54:
        raise ValueError("truncated Yas SessionInfo")
    return (
        body[:16],
        struct.unpack_from("<Q", body, 16)[0],
        struct.unpack_from("<H", body, 52)[0],
    )


def main():
    server_root = tempfile.TemporaryDirectory(prefix="yas-fd-channel-")
    channel_theirs, channel_ours = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    env = {
        **os.environ,
        "YAS_FD_CHANNEL": str(channel_theirs.fileno()),
        "YAS_SKIP_COMPOSITOR": "1",
        "YAS_PROCESS": "0",
        "YAS_KV": "0",
        "YAS_FONTS": "0",
        "YAS_RELAY": "0",
        "YAS_EXT": "0",
        "YAS_CHANNEL": "0",
        "XDG_STATE_HOME": os.path.join(server_root.name, "state"),
        "XDG_CACHE_HOME": os.path.join(server_root.name, "cache"),
    }
    proc = subprocess.Popen(
        [
            YAS_SERVER,
            "server",
            "--socket",
            os.path.join(server_root.name, "yas.sock"),
            "--no-processes",
            "--no-persistent-extensions",
        ],
        env=env,
        pass_fds=(channel_theirs.fileno(),),
    )
    channel_theirs.close()

    client_ours, client_theirs = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    channel_ours.sendmsg(
        [b"\x00"],
        [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack("i", client_theirs.fileno()))],
    )
    client_theirs.close()

    try:
        client_ours.sendall(PREFACE)
        write_frame(client_ours, request(HELLO, 1, client_hello()))
        hello = decode_result_prefix(read_result(client_ours, HELLO, 1))
        minor, boot, session, receive = decode_server_hello(hello)
        print(
            "HELLO:",
            f"minor={minor}",
            f"boot={boot.hex()}",
            f"session={session.hex()}",
            f"receive={'/'.join(str(value) for value in receive)}",
        )

        write_frame(client_ours, request(SESSION_INFO, 2))
        info = decode_result_prefix(read_result(client_ours, SESSION_INFO, 2))
        info_session, revision, family_count = decode_session_info(info)
        assert info_session == session, "SESSION_INFO changed the session ID"
        print(f"SESSION_INFO: revision={revision} families={family_count}")
        print("PASS")
    finally:
        client_ours.close()
        channel_ours.close()
        proc.send_signal(signal.SIGTERM)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            raise
        finally:
            server_root.cleanup()


if __name__ == "__main__":
    main()
