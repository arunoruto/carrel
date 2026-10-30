#!/bin/sh
# Installs carrel's latest release on Linux or macOS:
#   curl -fsSL https://raw.githubusercontent.com/arunoruto/carrel/main/install.sh | sh
# CARREL_VERSION=v0.2.0 picks a release; CARREL_INSTALL_DIR the folder, ~/.local/bin by default.
# Running it again updates carrel.
set -eu

repo=arunoruto/carrel
dir=${CARREL_INSTALL_DIR:-$HOME/.local/bin}

case "$(uname -s)-$(uname -m)" in
Linux-x86_64) target=x86_64-unknown-linux-musl ;;
Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
Darwin-arm64) target=aarch64-apple-darwin ;;
*)
	echo "no carrel build for $(uname -s) on $(uname -m); try: cargo install --git https://github.com/$repo" >&2
	exit 1
	;;
esac

# The latest release's page redirects to its tag.
version=${CARREL_VERSION:-$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")}
version=${version##*/}
name=carrel-$version-$target
url=https://github.com/$repo/releases/download/$version/$name.tar.gz

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "downloading carrel $version for $target"
curl -fsSL -o "$tmp/$name.tar.gz" "$url"
curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$url.sha256"
if command -v sha256sum >/dev/null; then
	(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null)
else
	(cd "$tmp" && shasum -a 256 -c "$name.tar.gz.sha256" >/dev/null)
fi
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
cp "$tmp/$name/carrel" "$dir/carrel.new"
chmod 755 "$dir/carrel.new"
mv "$dir/carrel.new" "$dir/carrel" # a running carrel keeps its old file
echo "installed $("$dir/carrel" --version) to $dir/carrel"

case ":$PATH:" in
*":$dir:"*) ;;
*) echo "note: $dir is not on your PATH; add it in your shell's profile" ;;
esac
if ! command -v uv >/dev/null && ! command -v docling >/dev/null; then
	echo "note: carrel extracts text from PDFs with uv (or Docling), which isn't installed:"
	echo "  curl -LsSf https://astral.sh/uv/install.sh | sh"
fi
cat <<EOF

next:
  carrel add 1706.03762            # a first paper
  carrel integrate                 # connect your AI agents (claude, opencode, codex, gemini, cursor)
  carrel service install web       # the web UI in the background, at every login
  carrel service install consume   # pick up papers you download
EOF
