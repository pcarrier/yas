import type {
  YasTransportEventMap,
  YasTransportMessage,
  YasTransportOptions,
  ConnectionStatus,
} from "../types";
import {
  YasConnection,
  type YasConnectionOptions,
  type YasTransport,
} from "./session";
import { YAS_WEBSOCKET_SUBPROTOCOL } from "./generated";

/** Exact browser contract for `/edge`: authenticate, then one YAS frame/message. */
export class YasEdgeWebSocketTransport implements YasTransport {
  readonly yasFraming = "message" as const;
  readonly maxDatagramSize = 0;
  private socket: WebSocket | null = null;
  private _status: ConnectionStatus = "disconnected";
  private disposed = false;
  private authenticated = false;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retryDelay: number;
  private messageListeners = new Set<(data: YasTransportMessage) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  authRejected = false;
  lastError: string | null = null;

  private readonly reconnectEnabled: boolean;
  private readonly initialRetryDelay: number;
  private readonly maximumRetryDelay: number;
  private readonly retryBackoff: number;

  constructor(
    private readonly url: string,
    private readonly passphrase: string,
    options: YasTransportOptions = {},
  ) {
    this.reconnectEnabled = options.reconnect ?? true;
    this.initialRetryDelay = options.reconnectDelay ?? 500;
    this.maximumRetryDelay = options.maxReconnectDelay ?? 10_000;
    this.retryBackoff = options.reconnectBackoff ?? 1.5;
    this.retryDelay = this.initialRetryDelay;
  }

  get status(): ConnectionStatus {
    return this._status;
  }

  get bufferedAmount(): number | undefined {
    return this.socket?.bufferedAmount;
  }

  connect(): void {
    if (
      this.disposed ||
      this._status === "connecting" ||
      this._status === "authenticating" ||
      this._status === "connected"
    )
      return;
    this.clearRetry();
    const socket = new WebSocket(this.url, YAS_WEBSOCKET_SUBPROTOCOL);
    socket.binaryType = "arraybuffer";
    this.socket = socket;
    this.authenticated = false;

    socket.onopen = () => {
      if (!this.isCurrent(socket)) return;
      this.setStatus("authenticating");
      if (!this.isCurrent(socket)) return;
      socket.send(this.passphrase);
    };
    socket.onmessage = (event: MessageEvent<unknown>) => {
      if (!this.isCurrent(socket)) return;
      if (!this.authenticated) {
        if (typeof event.data !== "string") {
          this.fail(socket, "edge sent binary data before authentication");
          return;
        }
        if (event.data === "ok") {
          this.authenticated = true;
          this.authRejected = false;
          this.lastError = null;
          this.retryDelay = this.initialRetryDelay;
          this.setStatus("connected");
          return;
        }
        if (event.data === "auth") {
          this.authRejected = true;
          this.fail(socket, "authentication failed", false);
          return;
        }
        if (event.data === "busy") {
          this.fail(socket, "edge busy");
          return;
        }
        this.fail(
          socket,
          event.data.startsWith("error:") ? event.data.slice(6) : event.data,
        );
        return;
      }
      if (typeof event.data === "string") {
        this.fail(socket, "unexpected text frame after YAS authentication");
        return;
      }
      if (event.data instanceof ArrayBuffer) {
        for (const listener of this.messageListeners) listener(event.data);
      } else if (event.data instanceof Blob) {
        void event.data.arrayBuffer().then((bytes) => {
          if (!this.isCurrent(socket)) return;
          for (const listener of this.messageListeners) listener(bytes);
        });
      } else {
        this.fail(socket, "unsupported WebSocket message type");
      }
    };
    socket.onerror = () => {
      if (this.isCurrent(socket) && !this.authenticated)
        this.setStatus("error");
    };
    socket.onclose = () => {
      if (!this.isCurrent(socket)) return;
      this.socket = null;
      this.authenticated = false;
      if (!this.disposed) {
        this.setStatus(this.authRejected ? "error" : "disconnected");
        if (!this.authRejected) this.scheduleRetry();
      }
    };
    // Status listeners can synchronously unmount or reconnect during HMR.
    // Own the socket and install its callbacks before notifying them.
    this.setStatus("connecting");
  }

  send(data: Uint8Array): void {
    if (!this.authenticated || this.socket?.readyState !== WebSocket.OPEN)
      throw new Error("YAS edge WebSocket is not authenticated");
    this.socket.send(data as Uint8Array<ArrayBuffer>);
  }

  reconnect(): void {
    if (this.disposed) return;
    this.clearRetry();
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.authenticated = false;
    this.authRejected = false;
    this.retryDelay = this.initialRetryDelay;
    this.setStatus("disconnected");
    this.connect();
  }

  suspend(): void {
    if (this.disposed) return;
    this.clearRetry();
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.authenticated = false;
    this.setStatus("disconnected");
  }

  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.clearRetry();
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.authenticated = false;
    this.setStatus("closed");
  }

  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
  }

  private isCurrent(socket: WebSocket): boolean {
    return this.socket === socket && !this.disposed;
  }

  private fail(socket: WebSocket, message: string, retry = true): void {
    if (!this.isCurrent(socket)) return;
    this.lastError = message;
    this.setStatus("error");
    if (!retry) this.authRejected = true;
    socket.close();
  }

  private setStatus(status: ConnectionStatus): void {
    if (this._status === status) return;
    this._status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  private scheduleRetry(): void {
    if (!this.reconnectEnabled || this.disposed || this.retryTimer) return;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.connect();
    }, this.retryDelay);
    this.retryDelay = Math.min(
      this.maximumRetryDelay,
      this.retryDelay * this.retryBackoff,
    );
  }

  private clearRetry(): void {
    if (!this.retryTimer) return;
    clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }
}

export async function connectYasEdge(
  url: string,
  passphrase: string,
  yasOptions: YasConnectionOptions,
  transportOptions: YasTransportOptions = {},
): Promise<{
  transport: YasEdgeWebSocketTransport;
  connection: YasConnection;
}> {
  const transport = new YasEdgeWebSocketTransport(
    url,
    passphrase,
    transportOptions,
  );
  const connection = new YasConnection(transport, yasOptions);
  await connection.connect();
  return { transport, connection };
}
