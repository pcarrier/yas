/** Product-level options for opening a native Net flow. */
export interface NetOpenOptions {
  /** Terminate TLS toward the target (TCP only). */
  tls?: boolean;
  /** Skip certificate verification; the server must also permit it. */
  insecure?: boolean;
  /** Open a datagram flow instead of a stream. */
  udp?: boolean;
  /** SNI to present; empty or omitted uses `host`. */
  sni?: string;
  /** ALPN protocols to offer, in order. */
  alpn?: readonly string[];
}

/** Protocol-neutral socket stream exposed to browser brokers. */
export interface NetStream {
  readonly streamId: number;
  readonly opened: Promise<string>;
  write(data: Uint8Array): Promise<void>;
  shutdownWrite(): void;
  close(): void;
  read(): AsyncGenerator<Uint8Array, void, void>;
}
