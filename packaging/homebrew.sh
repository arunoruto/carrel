#!/bin/sh
# Prints the Homebrew formula for a release, from its archives' checksums:
#   sh packaging/homebrew.sh v0.3.0 > Formula/carrel.rb
# The release workflow runs it and pushes the result to github.com/arunoruto/homebrew-tap.
set -eu

tag=$1
url=https://github.com/arunoruto/carrel/releases/download/$tag
sha() { curl -fsSL "$url/carrel-$tag-$1.tar.gz.sha256" | cut -d' ' -f1; }

cat <<EOF
# Written by carrel's release workflow (packaging/homebrew.sh); edits here are overwritten.
class Carrel < Formula
  desc "Research paper library for you and your AI"
  homepage "https://arunoruto.github.io/carrel"
  version "${tag#v}"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    depends_on arch: :arm64
    url "$url/carrel-$tag-aarch64-apple-darwin.tar.gz"
    sha256 "$(sha aarch64-apple-darwin)"
  end
  on_linux do
    on_intel do
      url "$url/carrel-$tag-x86_64-unknown-linux-musl.tar.gz"
      sha256 "$(sha x86_64-unknown-linux-musl)"
    end
    on_arm do
      url "$url/carrel-$tag-aarch64-unknown-linux-musl.tar.gz"
      sha256 "$(sha aarch64-unknown-linux-musl)"
    end
  end

  # Runs the text extractor (PyMuPDF), so papers get their text without further setup.
  depends_on "uv"

  def install
    bin.install "carrel"
  end

  def caveats
    <<~EOS
      Join your group's carrel server, if it has one:
        carrel connect https://<server>
      Run carrel in the background (the web UI, and papers from Downloads):
        carrel service install web
        carrel service install consume
      The full setup (search, Docling, recommendations): https://arunoruto.github.io/carrel/docs/setup/
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/carrel --version")
  end
end
EOF
