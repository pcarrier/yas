# @yas-run/bin

The [yas](https://yas.run) binary, distributed via npm. Installing `@yas-run/bin`
pulls in exactly one prebuilt package for your platform
(`@yas-run/bin-<os>-<cpu>[-musl]`) through optional dependencies — nothing else.

## CLI

```sh
npm i -g @yas-run/bin
yas open
```

## Bundle the binary in your own tool

The default export is the absolute filesystem path to the `yas` executable, so
you can spawn it directly. Resolution happens on import and throws with an
actionable message if the matching prebuilt package was not installed.

### ESM

```js
import yas from "@yas-run/bin";
import { spawn } from "node:child_process";

spawn(yas, ["open"], { stdio: "inherit" });
```

### CommonJS

```js
const yas = require("@yas-run/bin");
const { spawn } = require("node:child_process");

spawn(yas, ["open"], { stdio: "inherit" });
```

### Helpers

Lower-level resolution helpers are available on the `@yas-run/bin/resolve` subpath
(and as named exports of the main entry):

```js
import {
  binaryPath,
  binaryName,
  candidatePackages,
  isMusl,
} from "@yas-run/bin";
// or: import { binaryPath } from "@yas-run/bin/resolve";
```

| export                | description                                            |
| --------------------- | ------------------------------------------------------ |
| `default`             | absolute path to the `yas` binary (resolved at import) |
| `binaryPath()`        | same path, computed lazily; throws if unavailable      |
| `binaryName()`        | `"yas"` or `"yas.exe"`                                 |
| `candidatePackages()` | platform package names, in resolution order            |
| `isMusl()`            | `true` on musl-libc Linux                              |

## Platforms

Linux x64/arm64 (glibc & musl), macOS arm64, Windows x64 — matching the
binaries the yas release pipeline builds.

## GPL flavor

[`@yas-run/bin-gpl`](https://www.npmjs.com/package/@yas-run/bin-gpl) ships the
same build with x264 (GPL-2.0-or-later) instead of openh264 for software H.264:
better compression, and 4:4:4 rather than 4:2:0. Linux only, same API, no `yas`
CLI shim so it installs alongside this package.

## License

MIT
