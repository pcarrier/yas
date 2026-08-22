import type { SessionId } from "./types";

/** Presentation flags shared by the native FS client and Workspace UI. */
export const FS_ENTRY_TYPE_MASK = 0b11;
export const FS_ENTRY_FILE = 0;
export const FS_ENTRY_DIR = 1;
export const FS_ENTRY_SYMLINK = 2;
export const FS_ENTRY_OTHER = 3;
export const FS_ENTRY_UNREADABLE = 1 << 2;
export const FS_ENTRY_NO_CONTENT = 1 << 3;
export const FS_ENTRY_UNSTABLE = 1 << 4;
export const FS_ENTRY_LINK_DIR = 1 << 5;
export const FS_ENTRY_FILTERED = 1 << 6;

/** Native root close reasons plus the browser-local disconnect sentinel. */
export const FS_CLOSED_CLIENT_REQUEST = 0;
export const FS_CLOSED_ROOT_GONE = 1;
export const FS_CLOSED_PERMISSION_LOST = 2;
export const FS_CLOSED_BACKEND_FAILED = 3;
export const FS_CLOSED_RESOURCE_LIMIT = 4;
export const FS_CLOSED_CONNECTION_LOST = -1;

export type FsFileIndex = { paths: string[]; truncated: boolean };

export interface FsGrepFile {
  path: string;
  ignored: boolean;
  matches: {
    line: number;
    col: number;
    endLine: number;
    endCol: number;
    text: string;
  }[];
}

export interface FsGrepResult {
  files: FsGrepFile[];
  truncated: boolean;
}

export interface FsGrepOptions {
  caseSensitive?: boolean;
  regex?: boolean;
  noIgnore?: boolean;
  word?: boolean;
  maxMatches?: number;
  maxPerFile?: number;
}

/** Product-level options for a native FS root subscription. */
export interface FsSyncOptions {
  recursive?: boolean;
  single?: boolean;
  content?: boolean;
  crossFilesystem?: boolean;
  ignore?: boolean;
  gitignore?: boolean;
  dotIgnore?: boolean;
  excludeGit?: boolean;
  exclude?: string[];
  latencyMs?: number;
  inlineMax?: number;
  onReset?: () => void;
  onSync?: () => void;
  onUpdate?: () => void;
  onClosed?: (reason: number) => void;
  fromSessionId?: SessionId;
  staging?: boolean;
}
