import type { SessionId } from "./types";

export const LSP_STATUS_OK = 0;
export const LSP_STATUS_UNKNOWN_ID = 1;
export const LSP_STATUS_NOT_FOUND = 2;
export const LSP_STATUS_WRONG_TYPE = 3;
export const LSP_STATUS_PERMISSION = 4;
export const LSP_STATUS_TOO_LARGE = 5;
export const LSP_STATUS_BUDGET = 6;
export const LSP_STATUS_INVALID = 7;
export const LSP_STATUS_CANCELLED = 8;
export const LSP_STATUS_OTHER = 9;
export const LSP_STATUS_WARMING = 10;

export function lspStatusText(status: number): string {
  switch (status) {
    case LSP_STATUS_OK:
      return "ok";
    case LSP_STATUS_UNKNOWN_ID:
      return "unknown attachment";
    case LSP_STATUS_NOT_FOUND:
      return "not found";
    case LSP_STATUS_WRONG_TYPE:
      return "wrong type";
    case LSP_STATUS_PERMISSION:
      return "permission denied";
    case LSP_STATUS_TOO_LARGE:
      return "too large";
    case LSP_STATUS_BUDGET:
      return "budget exhausted";
    case LSP_STATUS_INVALID:
      return "invalid request";
    case LSP_STATUS_CANCELLED:
      return "cancelled";
    case LSP_STATUS_OTHER:
      return "backend error";
    case LSP_STATUS_WARMING:
      return "warming up";
    default:
      return `unknown status ${status}`;
  }
}

export const LSP_CLOSED_CLIENT_REQUEST = 0;
export const LSP_CLOSED_ROOT_GONE = 1;
export const LSP_CLOSED_PERMISSION_LOST = 2;
export const LSP_CLOSED_BACKEND_FAILED = 3;
export const LSP_CLOSED_RESOURCE_LIMIT = 4;
export const LSP_CLOSED_CONNECTION_LOST = -1;

export const LSP_PHASE_SPAWNING = 0;
export const LSP_PHASE_INITIALIZING = 1;
export const LSP_PHASE_INDEXING = 2;
export const LSP_PHASE_READY = 3;
export const LSP_PHASE_FAILED = 4;

export const LSP_COMPLETION_DEPRECATED = 1 << 0;
export const LSP_COMPLETION_SNIPPET = 1 << 1;
export const LSP_COMPLETION_PRESELECT = 1 << 2;

export const LSP_SEVERITY_ERROR = 1;
export const LSP_SEVERITY_WARNING = 2;
export const LSP_SEVERITY_INFO = 3;
export const LSP_SEVERITY_HINT = 4;

export const LSP_MARKUP_PLAIN = 0;
export const LSP_MARKUP_MARKDOWN = 1;

/** Product-level options for a native LSP workspace attachment. */
export interface LspOpenOptions {
  watch?: boolean;
  diagnostics?: boolean;
  diagLatencyMs?: number;
  onClosed?: (reason: number) => void;
  fromSessionId?: SessionId;
}
