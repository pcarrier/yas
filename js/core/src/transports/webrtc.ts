import type {
  YasTransport,
  YasTransportOptions,
  ConnectionStatus,
} from "../types";

export interface WebRtcDataChannelTransportOptions extends YasTransportOptions {
  /** Data channel label. Default: the canonical YAS v1 selector. */
  label?: string;
  /** Unreliable channel label. Default: `yas.v1.datagram`. */
  datagramLabel?: string;
  /** Open the paired unreliable channel. Default: true. */
  datagrams?: boolean;
}

export const YAS_WEBRTC_DATAGRAM_LABEL = "yas.v1.datagram";
export const YAS_WEBRTC_MAX_DATAGRAM_SIZE = 65_536;

export function createWebRtcDataChannelTransport(
  pc: RTCPeerConnection,
  opts?: WebRtcDataChannelTransportOptions,
): YasTransport & { waitForSync(): Promise<void> } {
  const label = opts?.label ?? "yas.v1";
  const datagramLabel = opts?.datagramLabel ?? YAS_WEBRTC_DATAGRAM_LABEL;
  const datagrams = opts?.datagrams ?? true;
  const connectTimeoutMs = opts?.connectTimeoutMs ?? 10000;
  const shouldReconnect = opts?.reconnect ?? true;
  const initialDelay = opts?.reconnectDelay ?? 500;
  const maxDelay = opts?.maxReconnectDelay ?? 10000;
  const backoff = opts?.reconnectBackoff ?? 1.5;

  const receiveTimeoutMs = 15_000;
  const maxEarlyMessages = 32;
  const maxEarlyBytes = 2 * 1024 * 1024;
  const maxMessageBytes = 16 * 1024 * 1024;

  let _status: ConnectionStatus = "connecting";
  let _lastError: string | null = null;
  let channel: RTCDataChannel | null = null;
  let datagramChannel: RTCDataChannel | null = null;
  let disposed = false;
  let suspended = false;
  let syncResolve: (() => void) | null = null;
  let syncReject: ((err: Error) => void) | null = null;
  let connectTimeout: ReturnType<typeof setTimeout> | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let receiveTimer: ReturnType<typeof setTimeout> | null = null;
  let currentDelay = initialDelay;
  let started = false;
  let earlyMessages: ArrayBuffer[] = [];
  let earlyMessageBytes = 0;

  const messageListeners = new Set<(data: ArrayBuffer) => void>();
  const datagramListeners = new Set<(data: ArrayBuffer) => void>();
  const statusListeners = new Set<(status: ConnectionStatus) => void>();

  function dispatch(data: ArrayBuffer) {
    if (!started) {
      if (
        earlyMessages.length >= maxEarlyMessages ||
        earlyMessageBytes + data.byteLength > maxEarlyBytes
      ) {
        earlyMessages = [];
        earlyMessageBytes = 0;
        _lastError = "pre-connect DataChannel receive budget exceeded";
        setStatus("error");
        channel?.close();
        return;
      }
      earlyMessages.push(data);
      earlyMessageBytes += data.byteLength;
    } else {
      for (const l of messageListeners) l(data);
    }
  }

  function clearConnectTimeout() {
    if (connectTimeout !== null) {
      clearTimeout(connectTimeout);
      connectTimeout = null;
    }
  }

  function clearReconnectTimer() {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function resetReceiveTimer() {
    if (receiveTimer !== null) clearTimeout(receiveTimer);
    if (disposed || _status !== "connected") {
      receiveTimer = null;
      return;
    }
    receiveTimer = setTimeout(() => {
      receiveTimer = null;
      if (disposed || _status !== "connected") return;
      _lastError = "receive timeout";
      setStatus("disconnected");
      scheduleReconnect();
    }, receiveTimeoutMs);
  }

  function clearReceiveTimer() {
    if (receiveTimer !== null) {
      clearTimeout(receiveTimer);
      receiveTimer = null;
    }
  }

  function isPeerConnectionAlive(): boolean {
    const s = pc.connectionState;
    return s !== "failed" && s !== "closed";
  }

  function scheduleReconnect() {
    if (disposed || suspended || !shouldReconnect || !isPeerConnectionAlive())
      return;
    clearReconnectTimer();
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (!disposed && isPeerConnectionAlive()) {
        openChannels();
      }
    }, currentDelay);
    currentDelay = Math.min(currentDelay * backoff, maxDelay);
  }

  function wireChannel(ch: RTCDataChannel) {
    if (!started) {
      earlyMessages = [];
      earlyMessageBytes = 0;
    }
    channel = ch;
    channel.binaryType = "arraybuffer";

    clearConnectTimeout();
    connectTimeout = setTimeout(() => {
      connectTimeout = null;
      if (_status === "connecting") {
        _lastError = "connect timeout";
        setStatus("error");
        scheduleReconnect();
      }
    }, connectTimeoutMs);

    ch.onopen = () => {
      if (disposed || channel !== ch) return;
      clearConnectTimeout();
      currentDelay = initialDelay;
      _lastError = null;
      setStatus("connected");
      resetReceiveTimer();
    };

    ch.onmessage = (e: MessageEvent) => {
      if (disposed || channel !== ch) return;
      resetReceiveTimer();
      const incoming = new Uint8Array(e.data as ArrayBuffer);
      if (incoming.byteLength > maxMessageBytes) {
        _lastError = "DataChannel message exceeds the YAS hard frame limit";
        setStatus("error");
        channel?.close();
        return;
      }
      dispatch(
        incoming.buffer.slice(
          incoming.byteOffset,
          incoming.byteOffset + incoming.byteLength,
        ) as ArrayBuffer,
      );
    };

    ch.onerror = () => {
      if (disposed || channel !== ch) return;
      clearConnectTimeout();
      clearReceiveTimer();
      _lastError = "Data channel error";
      setStatus("error");
      scheduleReconnect();
    };

    ch.onclose = () => {
      if (disposed || channel !== ch) return;
      clearConnectTimeout();
      clearReceiveTimer();
      setStatus("disconnected");
      scheduleReconnect();
    };
  }

  function wireDatagramChannel(ch: RTCDataChannel) {
    datagramChannel = ch;
    ch.binaryType = "arraybuffer";
    ch.onmessage = (event: MessageEvent) => {
      if (disposed || datagramChannel !== ch) return;
      const incoming = new Uint8Array(event.data as ArrayBuffer);
      if (incoming.byteLength > YAS_WEBRTC_MAX_DATAGRAM_SIZE) return;
      const owned = incoming.buffer.slice(
        incoming.byteOffset,
        incoming.byteOffset + incoming.byteLength,
      ) as ArrayBuffer;
      for (const listener of datagramListeners) listener(owned);
    };
    // The unreliable path is optional after negotiation. Its own failure does
    // not tear down or reconnect the authoritative reliable DataChannel.
    ch.onerror = () => {
      if (datagramChannel !== ch) return;
      datagramChannel = null;
      try {
        ch.close();
      } catch {
        // Already closed.
      }
    };
    ch.onclose = () => {
      if (datagramChannel === ch) datagramChannel = null;
    };
  }

  function openChannels() {
    if (disposed || suspended) return;
    setStatus("connecting");
    if (datagrams)
      wireDatagramChannel(
        pc.createDataChannel(datagramLabel, {
          ordered: false,
          maxRetransmits: 0,
        }),
      );
    wireChannel(pc.createDataChannel(label, { ordered: true }));
  }

  const transport: YasTransport & { waitForSync(): Promise<void> } = {
    yasFraming: "stream" as const,
    get maxDatagramSize() {
      return datagramChannel &&
        (datagramChannel.readyState === "connecting" ||
          datagramChannel.readyState === "open")
        ? YAS_WEBRTC_MAX_DATAGRAM_SIZE
        : 0;
    },
    connect() {
      if (disposed) return;
      if (suspended) {
        suspended = false;
        openChannels();
      }
      if (started) return;
      started = true;
      for (const msg of earlyMessages) {
        for (const l of messageListeners) l(msg);
      }
      earlyMessages = [];
      earlyMessageBytes = 0;
    },

    suspend() {
      if (disposed) return;
      suspended = true;
      clearConnectTimeout();
      clearReconnectTimer();
      clearReceiveTimer();
      const current = channel;
      const currentDatagram = datagramChannel;
      channel = null;
      datagramChannel = null;
      current?.close();
      currentDatagram?.close();
      earlyMessages = [];
      earlyMessageBytes = 0;
      currentDelay = initialDelay;
      setStatus("disconnected");
    },

    reconnect() {
      if (disposed) return;
      transport.suspend?.();
      suspended = false;
      openChannels();
    },

    get status() {
      return _status;
    },

    get authRejected() {
      return false;
    },
    get lastError() {
      return _lastError;
    },

    addEventListener(type: string, listener: (data: never) => void): void {
      if (type === "message") {
        messageListeners.add(
          listener as unknown as (data: ArrayBuffer) => void,
        );
      } else if (type === "datagram") {
        datagramListeners.add(
          listener as unknown as (data: ArrayBuffer) => void,
        );
      } else if (type === "statuschange") {
        statusListeners.add(
          listener as unknown as (status: ConnectionStatus) => void,
        );
      }
    },

    removeEventListener(type: string, listener: (data: never) => void): void {
      if (type === "message") {
        messageListeners.delete(
          listener as unknown as (data: ArrayBuffer) => void,
        );
      } else if (type === "datagram") {
        datagramListeners.delete(
          listener as unknown as (data: ArrayBuffer) => void,
        );
      } else if (type === "statuschange") {
        statusListeners.delete(
          listener as unknown as (status: ConnectionStatus) => void,
        );
      }
    },

    send(data: Uint8Array) {
      if (!channel || channel.readyState !== "open") return;
      const owned = new Uint8Array(data.byteLength);
      owned.set(data);
      channel.send(owned);
    },

    sendDatagram(data: Uint8Array) {
      if (
        !datagramChannel ||
        datagramChannel.readyState !== "open" ||
        data.byteLength > YAS_WEBRTC_MAX_DATAGRAM_SIZE
      )
        return;
      const owned = new Uint8Array(data.byteLength);
      owned.set(data);
      try {
        datagramChannel.send(owned);
      } catch {
        // Congestion, an SCTP limit, or a closing optional channel is loss.
        // None of those conditions invalidates the reliable YAS link.
      }
    },

    get bufferedAmount(): number | undefined {
      return channel?.bufferedAmount;
    },

    close() {
      disposed = true;
      clearConnectTimeout();
      clearReconnectTimer();
      clearReceiveTimer();
      if (channel) {
        try {
          channel.close();
        } catch {
          // Ignore.
        }
        channel = null;
      }
      if (datagramChannel) {
        try {
          datagramChannel.close();
        } catch {
          // Ignore.
        }
        datagramChannel = null;
      }
      pc.removeEventListener("connectionstatechange", onConnectionStateChange);
      setStatus("closed");
    },

    waitForSync() {
      if (_status === "connected") return Promise.resolve();
      if (_status === "error" || _status === "disconnected") {
        return Promise.reject(new Error(`transport ${_status}`));
      }
      return new Promise<void>((resolve, reject) => {
        syncResolve = resolve;
        syncReject = reject;
      });
    },
  };

  function setStatus(s: ConnectionStatus) {
    if (_status === s) return;
    _status = s;
    for (const l of statusListeners) l(s);
    if (s === "connected") {
      syncResolve?.();
      syncResolve = null;
      syncReject = null;
    } else if (s === "error" || s === "disconnected") {
      syncReject?.(new Error(`transport ${s}`));
      syncResolve = null;
      syncReject = null;
    }
  }

  function onConnectionStateChange() {
    if (disposed) return;
    const s = pc.connectionState;
    if (s === "disconnected" || s === "failed" || s === "closed") {
      clearReconnectTimer();
      clearReceiveTimer();
      setStatus("disconnected");
      scheduleReconnect();
    }
  }
  pc.addEventListener("connectionstatechange", onConnectionStateChange);

  openChannels();

  return transport;
}
