import "./share.css";
import { mountYasWorkspace, shareTransport } from "@yas-run/ui/embed";
import { MONO_CATALOG, MONO_STACK } from "./lib/fonts";
import {
  decryptPassphrase,
  encryptPassphrase,
  isEncrypted,
} from "./lib/passphrase-crypto";
import { initWasm } from "./lib/wasm";

const HUB_URL = "wss://yas.run";
const LAST_SHARE_KEY = "yas-share-last-psk";

type PassphraseResult =
  | { ok: true; passphrase: string; readOnly: boolean; debug: boolean }
  | { ok: false; error: string };

const ok = (passphrase: string, debug: boolean): PassphraseResult => ({
  ok: true,
  passphrase,
  readOnly: passphrase.endsWith(".ro"),
  debug,
});

function resolvePassphrase(): PassphraseResult {
  const parts = location.hash.slice(1).split("&").filter(Boolean);
  const debug = parts.includes("debug");
  const secrets = parts.filter((part) => part !== "debug");
  const stored = localStorage.getItem(LAST_SHARE_KEY);

  if (!secrets.length) {
    return stored
      ? ok(stored, debug)
      : { ok: false, error: "No share link specified." };
  }

  const named = secrets
    .map((part) => part.split("="))
    .find(([name]) => decodeURIComponent(name) === "psk");
  const bare = decodeURIComponent(secrets[0]);
  const plaintext = named
    ? decodeURIComponent(named.slice(1).join("="))
    : isEncrypted(bare)
      ? null
      : bare;

  if (plaintext !== null) {
    localStorage.setItem(LAST_SHARE_KEY, plaintext);
    if (!plaintext.endsWith(".ro")) {
      const hash = [
        encodeURIComponent(encryptPassphrase(plaintext)),
        debug && "debug",
      ]
        .filter(Boolean)
        .join("&");
      history.replaceState(null, "", `/s#${hash}`);
    }
    return ok(plaintext, debug);
  }

  const decrypted = decryptPassphrase(bare);
  if (decrypted) return ok(decrypted, debug);
  if (stored) return ok(stored, debug);
  return { ok: false, error: "This link belongs to a different browser." };
}

const state = document.querySelector<HTMLElement>("#state")!;
const app = document.querySelector<HTMLElement>("#app")!;
let dispose: (() => void) | undefined;

function showError(message: string) {
  app.hidden = true;
  state.hidden = false;
  state.replaceChildren();

  const logo = document.createElement("img");
  logo.src = "/logo.svg";
  logo.alt = "";
  const title = document.createElement("h1");
  title.textContent = "Cannot connect";
  const body = document.createElement("p");
  body.textContent = message;
  const home = document.createElement("a");
  home.href = "/";
  home.textContent = "← yas.run";
  state.append(logo, title, body, home);
}

async function main() {
  try {
    const result = resolvePassphrase();
    if (!result.ok) return showError(result.error);

    const wasm = await initWasm();
    state.hidden = true;
    app.hidden = false;
    dispose = mountYasWorkspace(app, {
      wasm,
      fontFamily: MONO_STACK,
      fonts: MONO_CATALOG,
      connections: [
        {
          id: "share",
          label: "shared terminal",
          transport: shareTransport(
            HUB_URL,
            result.passphrase,
            result.debug ? console : undefined,
          ),
          readOnly: result.readOnly,
        },
      ],
      onAuthError: () => {
        dispose?.();
        dispose = undefined;
        showError("The share was refused or revoked.");
      },
    });
  } catch (error) {
    showError(String(error));
  }
}

void main();
if (import.meta.hot) import.meta.hot.dispose(() => dispose?.());
