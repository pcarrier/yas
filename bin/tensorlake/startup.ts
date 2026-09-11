import { randomBytes } from "node:crypto";

export function shareConfiguration(configured?: string) {
  const passphrase = configured ?? randomBytes(24).toString("base64url");
  const invalid = [...passphrase].some((character) => {
    const code = character.codePointAt(0)!;
    return code === 0 || (code >= 0xd800 && code <= 0xdfff);
  });
  if (!passphrase.trim() || invalid) {
    throw new Error("The share passphrase must be nonblank UTF-8 text without NUL bytes.");
  }
  // Keep compatibility with share.env written by older launchers. This is
  // parsed as data below, never evaluated by a shell or passed to systemd.
  const escaped = passphrase.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return {
    environment: `YAS_SHARE_PASSPHRASE="${escaped}"\n`,
    env: { YAS_SHARE_PASSPHRASE: passphrase },
    shareUrl: `https://yas.run/s#psk=${encodeURIComponent(passphrase)}`,
  };
}

export function sandboxTimeout(value = "0") {
  const seconds = Number(value);
  if (!Number.isSafeInteger(seconds) || seconds < 0) {
    throw new Error("--timeout must be a nonnegative integer in seconds (0 requests the plan maximum).");
  }
  return seconds;
}

// Read only the format we write. Never source this file as shell code.
export function parseShareConfiguration(environment: string) {
  const match = /^YAS_SHARE_PASSPHRASE="((?:[^"\\]|\\["\\])*)"\n$/.exec(environment);
  if (!match) throw new Error("Invalid saved YAS share configuration.");
  return shareConfiguration(match[1].replace(/\\(["\\])/g, "$1"));
}
