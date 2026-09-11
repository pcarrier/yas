import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

function runYas(args) {
  return execFileSync("/usr/local/bin/yas", args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}

export function installExtensions(directory, run = runYas) {
  const manifest = JSON.parse(
    readFileSync(join(directory, "manifest.json"), "utf8"),
  );
  const installed = new Set(
    run(["ext", "list", "--json"])
      .split("\n")
      .filter((line) => line.trim())
      .map((line) => JSON.parse(line))
      .filter((extension) => extension.persistent)
      .map((extension) => extension.name),
  );
  for (const extension of manifest.extensions) {
    // Preserve user updates and disabled extensions across service restarts.
    if (installed.has(extension.name)) continue;
    run([
      "ext",
      "run",
      "--persist",
      "--restart",
      "always",
      extension.name,
      join(directory, extension.file),
    ]);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  installExtensions(process.argv[2] ?? "/opt/yas/extensions");
}
