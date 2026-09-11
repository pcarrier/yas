#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { copyFile, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { apiKey, buildCasImage, imageName } from "./tensorlake/common.ts";

const { values } = parseArgs({
  options: {
    name: { type: "string", default: imageName },
    help: { type: "boolean", short: "h" },
  },
});
if (values.help) {
  console.log(
    "Usage: bin/tensorlake-import.ts [--name IMAGE]\nBuild local x86_64 Linux YAS, then build/register a private Ubuntu 26.04 CAS image.",
  );
} else {
  apiKey();
  const repo = fileURLToPath(new URL("../", import.meta.url));
  console.error(
    "Building local YAS for x86_64 Linux (requires a matching Nix builder).",
  );
  const output = execFileSync(
    "nix",
    [
      "build",
      `${repo}#packages.x86_64-linux.yas-release`,
      "--no-link",
      "--print-out-paths",
    ],
    { cwd: repo, encoding: "utf8", stdio: ["inherit", "pipe", "inherit"] },
  ).trim();
  const binary = join(output, "bin/yas");
  const header = (await readFile(binary)).subarray(0, 20);
  if (
    header.toString("hex", 0, 4) !== "7f454c46" ||
    header[4] !== 2 ||
    header[5] !== 1 ||
    header.readUInt16LE(18) !== 62
  ) {
    throw new Error("The YAS build must be an x86_64 Linux ELF binary.");
  }
  const context = await mkdtemp(join(tmpdir(), "yas-tensorlake-"));
  try {
    await copyFile(binary, join(context, "yas"));
    console.error("Building all bundled YAS extensions from this checkout.");
    execFileSync(
      "nix",
      ["run", `${repo}#extensions`, "--", join(context, "extensions")],
      { cwd: repo, stdio: ["inherit", process.stderr, process.stderr] },
    );
    for (const file of ["install-extensions.mjs"]) {
      await copyFile(
        new URL(`./tensorlake/${file}`, import.meta.url),
        join(context, file),
      );
    }
    await copyFile(
      new URL("./tensorlake/Dockerfile", import.meta.url),
      join(context, "Dockerfile"),
    );
    console.log(
      JSON.stringify(
        await buildCasImage(join(context, "Dockerfile"), values.name),
        null,
        2,
      ),
    );
  } finally {
    await rm(context, { recursive: true, force: true });
  }
}
