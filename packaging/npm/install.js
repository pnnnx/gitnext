#!/usr/bin/env node
// Downloads the gitnext binary for this machine from the GitHub release that
// matches this package's version, checks its sha256 and puts it in vendor/.
// Runs on postinstall, and again from bin/gitnext.js if postinstall was skipped
// (npm --ignore-scripts). Overrides: GITNEXT_VERSION=v0.2.0, GITNEXT_REPO=owner/name,
// GITNEXT_DOWNLOAD_BASE=<url of a folder with the release assets> (mirrors, tests).
"use strict";
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { execFileSync } = require("child_process");

const TARGETS = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "linux-x64": "x86_64-unknown-linux-gnu",
};
const VENDOR = path.join(__dirname, "vendor");
const BIN = path.join(VENDOR, "gitnext");

function fail(msg) {
  const err = new Error(msg);
  err.gitnext = true;
  throw err;
}

function get(url, redirects = 5) {
  const lib = url.startsWith("https:") ? require("https") : require("http");
  return new Promise((resolve, reject) => {
    lib
      .get(url, { headers: { "User-Agent": "gitnext-npm" } }, (res) => {
        if ([301, 302, 303, 307, 308].includes(res.statusCode) && res.headers.location && redirects > 0) {
          res.resume();
          return resolve(get(new URL(res.headers.location, url).toString(), redirects - 1));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
      })
      .on("error", reject);
  });
}

async function install({ quiet = false } = {}) {
  if (fs.existsSync(BIN)) return BIN;
  const key = `${process.platform}-${process.arch}`;
  const target = TARGETS[key];
  if (!target) {
    fail(`gitnext has no prebuilt binary for ${key}. Build it from source: cargo install --git https://github.com/pnnnx/gitnext gitnext-cli`);
  }
  const version = process.env.GITNEXT_VERSION || `v${require("./package.json").version}`;
  const repo = process.env.GITNEXT_REPO || "pnnnx/gitnext";
  const base = process.env.GITNEXT_DOWNLOAD_BASE || `https://github.com/${repo}/releases/download/${version}`;
  const name = `gitnext-${target}.tar.gz`;
  if (!quiet) process.stderr.write(`gitnext: downloading ${name} (${version})…\n`);
  let tarball, sums;
  try {
    [tarball, sums] = await Promise.all([get(`${base}/${name}`), get(`${base}/${name}.sha256`)]);
  } catch (e) {
    fail(`gitnext: could not download ${base}/${name}: ${e.message}`);
  }
  const want = sums.toString().trim().split(/\s+/)[0];
  const got = crypto.createHash("sha256").update(tarball).digest("hex");
  if (want !== got) fail(`gitnext: checksum mismatch for ${name} (expected ${want}, got ${got})`);
  fs.mkdirSync(VENDOR, { recursive: true });
  const tmp = path.join(VENDOR, name);
  fs.writeFileSync(tmp, tarball);
  try {
    execFileSync("tar", ["-xzf", tmp, "-C", VENDOR]);
  } finally {
    fs.rmSync(tmp, { force: true });
  }
  if (!fs.existsSync(BIN)) fail(`gitnext: ${name} did not contain a gitnext binary`);
  fs.chmodSync(BIN, 0o755);
  if (!quiet) process.stderr.write(`gitnext: installed ${BIN}\n`);
  return BIN;
}

module.exports = { install, BIN };

if (require.main === module) {
  install({ quiet: process.argv.includes("--quiet") }).catch((e) => {
    process.stderr.write(`${e.message}\n`);
    process.exit(1);
  });
}
