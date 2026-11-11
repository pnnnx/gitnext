# Homebrew formula for gitnext — lives in the tap repository pnnnx/homebrew-tap
# as Formula/gitnext.rb. Users install with:  brew install pnnnx/tap/gitnext
#
# The binaries come from the GitHub release of the same version (built by
# .github/workflows/release.yml). Do not edit the sha256 lines by hand:
#   sh packaging/homebrew/update.sh v0.2.0 > Formula/gitnext.rb
# fills them from the release's *.sha256 files.
class Gitnext < Formula
  desc "git that understands code: code map, symbol graph and plain-language git for AI agents and people"
  homepage "https://github.com/pnnnx/gitnext"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/pnnnx/gitnext/releases/download/v0.2.0/gitnext-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_AARCH64_APPLE_DARWIN"
    end
    on_intel do
      url "https://github.com/pnnnx/gitnext/releases/download/v0.2.0/gitnext-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_X86_64_APPLE_DARWIN"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/pnnnx/gitnext/releases/download/v0.2.0/gitnext-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_AARCH64_UNKNOWN_LINUX_GNU"
    end
    on_intel do
      url "https://github.com/pnnnx/gitnext/releases/download/v0.2.0/gitnext-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_X86_64_UNKNOWN_LINUX_GNU"
    end
  end

  def install
    bin.install "gitnext"
  end

  test do
    assert_match "gitnext", shell_output("#{bin}/gitnext --version")
  end
end
