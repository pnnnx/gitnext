# Packaging gitnext

Everything here downloads the binaries that `.github/workflows/release.yml`
attaches to a GitHub release (`gitnext-<target>.tar.gz` + `.sha256` for
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`,
`x86_64-unknown-linux-gnu`). Release first, then publish.

## Homebrew — `brew install pnnnx/tap/gitnext`

One time: create the tap repository.

```sh
gh repo create pnnnx/homebrew-tap --public --description "Homebrew formulae"
git clone https://github.com/pnnnx/homebrew-tap ~/Developer/homebrew-tap
mkdir -p ~/Developer/homebrew-tap/Formula
```

For each release (after the release workflow has finished and the assets are up):

```sh
cd ~/Developer/gitnext
sh packaging/homebrew/update.sh v0.3.1 > ~/Developer/homebrew-tap/Formula/gitnext.rb
cd ~/Developer/homebrew-tap
git add Formula/gitnext.rb && git commit -m "gitnext 0.3.1" && git push
brew install pnnnx/tap/gitnext && gitnext --version     # check
```

`update.sh` reads the real sha256 sums from the release; it refuses to write a
formula while any asset is missing.

## npm — `npx -y gitnext setup`

`packaging/npm` is a 3-file package: on install (or on first run, if install
scripts are disabled) it downloads the binary for the machine from the release
of the same version, checks the sha256 and runs it.

For each release:

```sh
cd ~/Developer/gitnext/packaging/npm
npm version 0.3.0 --no-git-tag-version   # must match the release tag (v0.3.1)
npm login                                  # once
npm publish
npx -y gitnext --version                   # check
```

Mirrors or tests: `GITNEXT_DOWNLOAD_BASE=<url of a folder with the assets>`;
another fork: `GITNEXT_REPO=owner/name`; another release: `GITNEXT_VERSION=v0.3.1`.

## Other ways (see the plan document)

- `curl … | sh` — `scripts/install.sh` (already works with the release).
- Claude Desktop one-click: a `.mcpb` bundle with the binary — not done yet.
- `pip` / `uvx` (maturin wheel with the binary) — not done yet.
