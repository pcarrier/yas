"use strict";

// Default (CommonJS) export: the absolute filesystem path to the prebuilt
// `yas` executable for the current platform.
//
//   const yas = require("@yas-run/bin");
//   require("child_process").spawn(yas, ["open"], { stdio: "inherit" });
//
// Throws at require time with an actionable message if no matching prebuilt
// package is installed. Named helpers live on `@yas-run/bin/resolve`.
module.exports = require("./resolve.js").binaryPath();
