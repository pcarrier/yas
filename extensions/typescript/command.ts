import { type YasHost, encodeUtf8, errorMessage } from "./yas";

export interface CommandOption {
  readonly names: readonly string[];
  readonly takes_value?: boolean;
  readonly help?: string;
}

export interface CommandDescription {
  readonly path: readonly string[];
  readonly summary?: string;
  readonly usage?: string;
  readonly options?: readonly CommandOption[];
}

export interface CommandDescriptor {
  readonly protocol: "yas.cli.v1";
  readonly summary: string;
  readonly commands: readonly CommandDescription[];
}

export interface InvocationRequest {
  readonly args: readonly string[];
  readonly streamsStdin: boolean;
}

export type CommandBytes = string | Uint8Array;

export interface CommandResponse {
  readonly stdout?: CommandBytes;
  readonly stderr?: CommandBytes;
  readonly result?: {
    readonly contentType: string;
    readonly data: CommandBytes;
  };
  readonly code?: number;
  readonly detail?: string;
}

export type CommandHandler = (
  request: InvocationRequest,
) => CommandResponse | undefined;

function bytes(value: CommandBytes): Uint8Array {
  return typeof value === "string" ? encodeUtf8(value) : value;
}

function validContentType(value: string): boolean {
  return /^[a-z0-9][a-z0-9!#$&^_.+-]*\/[a-z0-9][a-z0-9!#$&^_.+-]*$/.test(value);
}

function sendResponse(host: YasHost, response: CommandResponse): void {
  if (response.stdout !== undefined) host.commandStdout(bytes(response.stdout));
  if (response.stderr !== undefined) host.commandStderr(bytes(response.stderr));
  if (response.result !== undefined) {
    if (!validContentType(response.result.contentType)) {
      throw new Error(
        "result content type is not a canonical lowercase media type",
      );
    }
    host.commandResult(
      response.result.contentType,
      bytes(response.result.data),
    );
  }
  const code = response.code ?? 0;
  if (!Number.isInteger(code) || code < -0x80000000 || code > 0x7fffffff) {
    throw new Error("command exit code is outside i32");
  }
  host.commandExit(code, response.detail ?? "");
}

/**
 * Register and synchronously serve a small `yas.cli.v1` command surface.
 *
 * The QuickJS host owns native Channel/Transfer framing and exact opaque
 * handles. JavaScript sees only typed command invocations and responses.
 */
export function serveCommands(
  descriptor: CommandDescriptor,
  handler: CommandHandler,
  host: YasHost = yas,
): number {
  if (!host.context.persistent || !host.context.name) {
    throw new Error("command providers require a named persistent extension");
  }
  host.registerCommand(JSON.stringify(descriptor));

  while (true) {
    const request = host.acceptCommand();
    if (request === undefined) return 0;
    try {
      sendResponse(host, handler(request) ?? {});
    } catch (error) {
      try {
        host.commandStderr(
          encodeUtf8(`extension command failed: ${errorMessage(error)}\n`),
        );
        host.commandExit(1, "command handler failed");
      } catch {
        host.commandCancel();
      }
    }
  }
}
