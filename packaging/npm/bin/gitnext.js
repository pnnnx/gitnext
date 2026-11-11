#!/usr/bin/env node
// `npx gitnext …` / the `gitnext` command from npm: runs the native binary,
// downloading it first if the install step was skipped.
"use strict";
const { spawnSync } = require("child_process");
const { install } = require("../install.js");

install()
  .then((bin) => {
    const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
    if (r.error) {
      process.stderr.write(`gitnext: ${r.error.message}\n`);
      process.exit(1);
    }
    process.exit(r.status === null ? 1 : r.status);
  })
  .catch((e) => {
    process.stderr.write(`${e.message}\n`);
    process.exit(1);
  });
