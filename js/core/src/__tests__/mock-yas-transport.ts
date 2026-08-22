import type {
  ConnectionStatus,
  YasTransport,
  YasTransportMessage,
} from "../types";

/**
 * Minimal native transport fixture for component and ownership tests.
 *
 * This intentionally knows nothing about family payloads. Protocol tests use
 * typed fake YAS peers; presentation tests only need connection lifecycle and
 * a place to observe complete native frames sent by the client.
 */
export class MockYasTransport implements YasTransport {
  readonly yasFraming = "message" as const;
  readonly maxDatagramSize: number;
  private currentStatus: ConnectionStatus;
  private readonly messageListeners = new Set<
    (data: YasTransportMessage) => void
  >();
  private readonly datagramListeners = new Set<
    (data: YasTransportMessage) => void
  >();
  private readonly statusListeners = new Set<
    (status: ConnectionStatus) => void
  >();

  readonly sent: Uint8Array[] = [];
  readonly sentDatagrams: Uint8Array[] = [];
  authRejected = false;
  lastError: string | null = null;
  reconnectCount = 0;
  suspendCount = 0;

  constructor(
    initialStatus: ConnectionStatus = "connected",
    maxDatagramSize = 0,
  ) {
    this.currentStatus = initialStatus;
    this.maxDatagramSize = maxDatagramSize;
  }

  get status(): ConnectionStatus {
    return this.currentStatus;
  }

  connect(): void {
    if (this.currentStatus !== "connected") this.setStatus("connected");
  }

  reconnect(): void {
    this.reconnectCount += 1;
    this.setStatus("disconnected");
    this.setStatus("connecting");
  }

  suspend(): void {
    this.suspendCount += 1;
    this.setStatus("disconnected");
  }

  send(data: Uint8Array): void {
    this.sent.push(new Uint8Array(data));
  }

  sendDatagram(data: Uint8Array): void {
    if (this.maxDatagramSize > 0 && data.length <= this.maxDatagramSize)
      this.sentDatagrams.push(new Uint8Array(data));
  }

  close(): void {
    this.setStatus("closed");
  }

  addEventListener(
    type: "message",
    listener: (data: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  addEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((data: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message")
      this.messageListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "datagram")
      this.datagramListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener(
    type: "message",
    listener: (data: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  removeEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((data: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message")
      this.messageListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "datagram")
      this.datagramListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
  }

  setStatus(status: ConnectionStatus): void {
    this.currentStatus = status;
    for (const listener of this.statusListeners) listener(status);
  }

  /** Deliver one complete native YAS frame to the client. */
  receive(frame: Uint8Array): void {
    const copy = frame.slice().buffer;
    for (const listener of this.messageListeners) listener(copy);
  }

  /** Deliver a borrowed native frame view, as a BYOB transport may. */
  receiveBorrowed(frame: Uint8Array): void {
    for (const listener of this.messageListeners) listener(frame);
  }

  /** Deliver one complete unreliable YAS Event to the client. */
  receiveDatagram(frame: Uint8Array): void {
    const copy = frame.slice().buffer;
    for (const listener of this.datagramListeners) listener(copy);
  }
}
