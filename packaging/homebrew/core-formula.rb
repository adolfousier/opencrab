# typed: strict
# frozen_string_literal: true

# Candidate formula for submission to Homebrew/homebrew-core (#924).
#
# NOT the tap formula. The tap (packaging/homebrew/opencrabs.rb.template)
# installs the prebuilt release binary. Core builds from source instead, so
# this formula reuses neither the release archive nor its bundled rtk copy.
# Users are not affected by the source build, because core publishes bottles
# that Homebrew's CI compiles once per platform.
#
# Submission checklist:
#   1. Update url + sha256 to the release being submitted
#   2. brew audit --new --strict --online opencrabs
#   3. Open a PR against Homebrew/homebrew-core
class Opencrabs < Formula
  desc "Autonomous, self-improving AI agent in a single Rust binary"
  homepage "https://opencrabs.com"
  url "https://github.com/adolfousier/opencrabs/archive/refs/tags/v0.3.78.tar.gz"
  sha256 "1ced91fe756beb7b09764bd3c8864014f07bd04b5471e0f2aa82c29996744570"
  license "MIT"
  head "https://github.com/adolfousier/opencrabs.git", branch: "main"

  depends_on "cmake" => :build
  depends_on "pkgconf" => :build
  depends_on "rust" => :build

  # openssl-sys, via reqwest -> native-tls. Present transitively through other
  # dependencies, but Homebrew's build sandbox only exposes DECLARED ones, so
  # leaving it out fails with "Could not find directory of OpenSSL installation"
  # even though the library is on the machine.
  depends_on "openssl@3"

  # OpenCrabs prepends rtk to supported shell commands to cut their output.
  # Upstream archives bundle a copy; here it is a real dependency instead.
  depends_on "rtk"

  on_linux do
    # alsa-sys, reached through rodio for local-stt/local-tts. macOS uses
    # CoreAudio and needs no equivalent.
    depends_on "alsa-lib"
  end

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    # Generate a config and check it is real, parseable TOML with the section
    # the loader requires, rather than asserting on --version or --help.
    system bin/"opencrabs", "init"

    config = testpath/".opencrabs/config.toml"
    assert_path_exists config
    assert_match "[provider_registry]", config.read

    # Reading it back exercises the config loader, so a binary that writes a
    # file it cannot itself parse fails here.
    assert_match "Database:", shell_output("#{bin}/opencrabs config")
  end
end
