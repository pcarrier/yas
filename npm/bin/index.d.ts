/**
 * Absolute filesystem path to the prebuilt `yas` executable for the current
 * platform. Resolved eagerly on import; throws if no matching prebuilt package
 * is installed.
 *
 * @example
 * import yas from "@yas-run/bin";
 * import { spawn } from "node:child_process";
 * spawn(yas, ["open"], { stdio: "inherit" });
 */
declare const yasPath: string;
export default yasPath;

/** Resolve the absolute path to the platform `yas` binary (throws if none installed). */
export declare function binaryPath(): string;
/** Executable filename for this platform (`yas` or `yas.exe`). */
export declare function binaryName(): string;
/** Candidate npm package names for this platform, in resolution order. */
export declare function candidatePackages(): string[];
/** Whether the current Linux runtime uses musl libc. */
export declare function isMusl(): boolean;
