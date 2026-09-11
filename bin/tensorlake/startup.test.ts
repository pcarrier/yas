import assert from "node:assert/strict";
import { test } from "node:test";
import { parseShareConfiguration, sandboxTimeout, shareConfiguration } from "./startup.ts";

test("missing passphrases generate distinct share URLs", () => {
  const first = shareConfiguration();
  const second = shareConfiguration();
  assert.notEqual(first.shareUrl, second.shareUrl);
  assert.match(first.shareUrl, /^https:\/\/yas\.run\/s#psk=[\w-]{32}$/);
});

test("passphrases preserve whitespace and shell metacharacters as file data", () => {
  const passphrase = '  "secret" \\ $HOME `id` $(id)\nline two\t';
  const config = shareConfiguration(passphrase);
  assert.equal(new URL(config.shareUrl).hash, `#psk=${encodeURIComponent(passphrase)}`);
  assert.equal(
    config.environment,
    'YAS_SHARE_PASSPHRASE="  \\"secret\\" \\\\ $HOME `id` $(id)\nline two\t"\n',
  );
});

test("blank, NUL-containing, and malformed Unicode passphrases are rejected", () => {
  for (const passphrase of ["", " \t\n", "\u2003", "secret\0value", "x\ud800"]) {
    assert.throws(() => shareConfiguration(passphrase), /nonblank.*NUL/);
  }
});

test("sandbox timeout defaults to the plan maximum and accepts explicit limits", () => {
  assert.equal(sandboxTimeout(), 0);
  assert.equal(sandboxTimeout("0"), 0);
  assert.equal(sandboxTimeout("3600"), 3600);
  for (const value of ["-1", "1.5", "NaN", "Infinity", "9007199254740992"]) {
    assert.throws(() => sandboxTimeout(value), /nonnegative integer/);
  }
});

test("saved passphrases round-trip without interpreting shell syntax", () => {
  const value = ' "\\$HOME`id`$(id)\nsecond line\t';
  const original = shareConfiguration(value);
  assert.deepEqual(parseShareConfiguration(original.environment), original);
  assert.throws(() => parseShareConfiguration('YAS_SHARE_PASSPHRASE=$(id)\n'), /Invalid saved/);
});
