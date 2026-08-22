import type {
  ConnectionStatus,
  YasTransportEventMap,
  YasTransportMessage,
} from "../types";
import {
  YasRelayClient,
  type YasRelayLink,
  type YasRelayRoute,
  type YasRelayTunnelTransport,
} from "./relay";
import type { YasTransport } from "./session";

/** Reconnectable raw YAS tunnel for one Relay route.
 *
 * It forwards the nested byte stream unchanged. The single typed
 * `YasConnection` constructed above it owns framing, HELLO and all families.
 */
export class YasNativeRelayTransport implements YasTransport {
  readonly yasFraming = "stream" as const;
  readonly authRejected = false;
  readonly maxDatagramSize = 0;
  private readonly messages = new Set<(message: YasTransportMessage) => void>();
  private readonly statuses = new Set<(status: ConnectionStatus) => void>();
  private active: YasRelayTunnelTransport | null = null;
  private link: YasRelayLink | null = null;
  private removeMessage: (() => void) | null = null;
  private removeStatus: (() => void) | null = null;
  private generation = 0;
  private connecting = false;
  private disposed = false;
  private _status: ConnectionStatus = "disconnected";
  private _lastError: string | null = null;

  constructor(
    private readonly relay: YasRelayClient,
    readonly route: YasRelayRoute,
  ) {}

  get status(): ConnectionStatus {
    return this._status;
  }

  get lastError(): string | null {
    return this._lastError;
  }

  get bufferedAmount(): number {
    return this.active?.bufferedAmount ?? 0;
  }

  connect(): void {
    if (this.disposed || this.connecting || this.active) return;
    this.connecting = true;
    this.setStatus("connecting");
    const generation = ++this.generation;
    void this.relay.connect(this.route).then(
      (link) => {
        if (this.disposed || generation !== this.generation) {
          link.transport.close();
          void this.relay.disconnect(link.relayHandle, "stale browser link");
          return;
        }
        this.connecting = false;
        this.attach(link);
      },
      (error) => {
        if (this.disposed || generation !== this.generation) return;
        this.connecting = false;
        this._lastError =
          error instanceof Error ? error.message : String(error);
        this.setStatus("error");
      },
    );
  }

  reconnect(): void {
    if (this.disposed) return;
    this.stop("browser reconnect");
    this.connect();
  }

  suspend(): void {
    if (this.disposed) return;
    this.stop("browser suspend");
    this.setStatus("disconnected");
  }

  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.stop("browser close");
    this.setStatus("closed");
  }

  send(data: Uint8Array): void {
    const active = this.active;
    if (!active) throw new Error("nested Relay YAS transport is not connected");
    active.send(data);
  }

  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messages.add(listener as (message: YasTransportMessage) => void);
    else if (type === "statuschange")
      this.statuses.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messages.delete(listener as (message: YasTransportMessage) => void);
    else if (type === "statuschange")
      this.statuses.delete(listener as (status: ConnectionStatus) => void);
  }

  private attach(link: YasRelayLink): void {
    const active = link.transport;
    this.link = link;
    this.active = active;
    const onMessage = (message: YasTransportMessage) => {
      if (this.active !== active) return;
      for (const listener of this.messages) listener(message);
    };
    const onStatus = (status: ConnectionStatus) => {
      if (this.active !== active) return;
      this._lastError = active.lastError;
      this.setStatus(status);
      if (status === "closed" || status === "error") this.detach();
    };
    active.addEventListener("message", onMessage);
    active.addEventListener("statuschange", onStatus);
    this.removeMessage = () => active.removeEventListener("message", onMessage);
    this.removeStatus = () =>
      active.removeEventListener("statuschange", onStatus);
    active.connect();
  }

  private stop(reason: string): void {
    this.generation++;
    this.connecting = false;
    const link = this.link;
    this.detach();
    link?.transport.close();
    if (link)
      void this.relay
        .disconnect(link.relayHandle, reason)
        .catch(() => undefined);
  }

  private detach(): void {
    this.removeMessage?.();
    this.removeStatus?.();
    this.removeMessage = null;
    this.removeStatus = null;
    this.active = null;
    this.link = null;
  }

  private setStatus(status: ConnectionStatus): void {
    if (this._status === status) return;
    this._status = status;
    for (const listener of this.statuses) listener(status);
  }
}
