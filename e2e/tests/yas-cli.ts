import { execFileSync } from "child_process";
import fs from "fs";
import path from "path";

const YAS = path.resolve(__dirname, "../../target/debug/yas");

/** Point the CLI at the private server started by the E2E harness. */
function serverEnv(): Record<string, string | undefined> {
  const handoff = path.resolve(__dirname, "../.e2e-socket");
  if (!fs.existsSync(handoff))
    throw new Error(
      "E2E server socket handoff is missing; refusing to use the default server",
    );
  const sock = fs.readFileSync(handoff, "utf8").trim();
  if (!sock || !fs.existsSync(sock))
    throw new Error(
      "E2E server socket is unavailable; refusing to use the default server",
    );
  return { ...process.env, YAS_SOCK: sock };
}

export function yas(...args: string[]): string {
  return execFileSync(YAS, args, { encoding: "utf8", env: serverEnv() });
}

/** Close every native terminal so state cannot leak between serial E2E tests. */
export function closeAllTerminals(): void {
  for (const row of yas("terminal", "list").trim().split("\n").slice(1)) {
    const id = row.split("\t")[0];
    if (id) yas("terminal", "close", id);
  }
}
