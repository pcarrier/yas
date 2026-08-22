// ESM entry point. Default export is the absolute path to the platform `yas`
// binary; named exports expose the resolution helpers.
//
//   import yas from "@yas-run/bin";
//   import { spawn } from "node:child_process";
//   spawn(yas, ["open"], { stdio: "inherit" });
import resolve from "./resolve.js";

const yasPath = resolve.binaryPath();

export default yasPath;
export const binaryPath = resolve.binaryPath;
export const binaryName = resolve.binaryName;
export const candidatePackages = resolve.candidatePackages;
export const isMusl = resolve.isMusl;
