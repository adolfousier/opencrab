# typed: strict
# frozen_string_literal: true

# Candidate formula for submission to Homebrew/homebrew-core (#924).
#
# NOT the tap formula. The tap (packaging/homebrew/opencrabs.rb.template)
# installs the prebuilt release binary and bundles rtk. Core does not accept
# either of those: it builds from source and forbids vendoring third-party
# precompiled binaries. Users are not affected by the source build, because
# core publishes bottles that Homebrew's CI compiles once per platform.
#
# RTK is deliberately absent. OpenCrabs downloads the right rtk binary for the
# platform on first use when it is missing, so omitting it here costs a one-off
# download rather than a feature.
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

  # No opus dependency: opusic-sys vendors and statically links it. CI builds
  # Linux with --all-features installing only libasound2-dev, and the built
  # binary shows no libopus linkage. Declaring it would be a phantom dependency.

  on_linux do
    # alsa-sys, reached through rodio for local-stt/local-tts. macOS uses
    # CoreAudio and needs no equivalent.
    depends_on "alsa-lib"
  end

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/opencrabs --version")

    # --version alone would pass against a binary that cannot read its own
    # config, so exercise a subcommand that touches the config layer.
    assert_match "opencrabs", shell_output("#{bin}/opencrabs --help")
  end
end
