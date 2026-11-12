#!/bin/sh
# Print the Homebrew formula for a released version, with real sha256 sums:
#   sh packaging/homebrew/update.sh v0.3.0 > ../homebrew-tap/Formula/gitnext.rb
set -e
TAG=${1:?usage: update.sh vX.Y.Z}
REPO=${GITNEXT_REPO:-pnnnx/gitnext}
HERE=$(cd "$(dirname "$0")" && pwd)
sed_args=""
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  sum=$(curl -fsSL "https://github.com/$REPO/releases/download/$TAG/gitnext-$t.tar.gz.sha256" | awk '{print $1}')
  [ ${#sum} -eq 64 ] || { echo "no sha256 for $t in release $TAG — has the release workflow finished?" >&2; exit 1; }
  key=$(echo "SHA256_$t" | tr 'a-z-' 'A-Z_')
  sed_args="$sed_args -e s/$key/$sum/"
done
sed $sed_args -e "s/v0\.2\.0/$TAG/g" -e "s/version \"0\.2\.0\"/version \"${TAG#v}\"/" -e "s#pnnnx/gitnext#$REPO#g" "$HERE/gitnext.rb"
