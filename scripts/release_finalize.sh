#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/release_finalize.sh --repo-dir <path> --tap-dir <path> --tap-repo <owner/repo> \
    --formula-path <path> --github-repo <owner/repo> --new-version <version> --tag <tag> \
    --tarball-path <path> --tarball-name <name> --sha256 <sha256>
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

REPO_DIR=""
TAP_DIR=""
TAP_REPO=""
FORMULA_PATH=""
GITHUB_REPO=""
NEW_VERSION=""
TAG=""
TARBALL_PATH=""
TARBALL_NAME=""
SHA256=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo-dir) REPO_DIR="$2"; shift 2 ;;
    --tap-dir) TAP_DIR="$2"; shift 2 ;;
    --tap-repo) TAP_REPO="$2"; shift 2 ;;
    --formula-path) FORMULA_PATH="$2"; shift 2 ;;
    --github-repo) GITHUB_REPO="$2"; shift 2 ;;
    --new-version) NEW_VERSION="$2"; shift 2 ;;
    --tag) TAG="$2"; shift 2 ;;
    --tarball-path) TARBALL_PATH="$2"; shift 2 ;;
    --tarball-name) TARBALL_NAME="$2"; shift 2 ;;
    --sha256) SHA256="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

for required in REPO_DIR TAP_DIR TAP_REPO FORMULA_PATH GITHUB_REPO NEW_VERSION TAG TARBALL_PATH TARBALL_NAME SHA256; do
  if [ -z "${!required}" ]; then
    echo "Missing required argument: $required" >&2
    usage >&2
    exit 1
  fi
done

require_cmd gh
require_cmd git
require_cmd shasum

if ! gh auth status >/dev/null 2>&1; then
  echo "GitHub CLI not authenticated. Run: gh auth login" >&2
  exit 1
fi

cd "$REPO_DIR"

if ! git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  echo "Tag $TAG does not exist on origin yet." >&2
  echo "Tag creation is handled by CI auto-tag. Re-run after CI finishes." >&2
  exit 1
fi

if ! gh release view "$TAG" --repo "$GITHUB_REPO" >/dev/null 2>&1; then
  echo "Release $TAG does not exist yet." >&2
  echo "Release creation is handled by CI release workflow. Re-run after it completes." >&2
  exit 1
fi

echo "Release $TAG exists. Uploading/overwriting asset."
gh release upload "$TAG" "$TARBALL_PATH" --repo "$GITHUB_REPO" --clobber

if [ ! -d "$TAP_DIR/.git" ]; then
  echo "Cloning tap repo..."
  git clone "https://github.com/$TAP_REPO.git" "$TAP_DIR"
fi

cd "$TAP_DIR"
mkdir -p Formula

cat > "$FORMULA_PATH" << RUBY
class Microclaw < Formula
  desc "Agentic AI assistant for Telegram - web search, scheduling, memory, tool execution"
  homepage "https://github.com/$GITHUB_REPO"
  url "https://github.com/$GITHUB_REPO/releases/download/$TAG/$TARBALL_NAME"
  sha256 "$SHA256"
  license "MIT"

  def install
    bin.install "microclaw"
  end

  test do
    assert_match "MicroClaw", shell_output("#{bin}/microclaw help")
  end
end
RUBY

git add .
git commit -m "microclaw homebrew release $NEW_VERSION"
git push

echo ""
echo "Done! Released $TAG and updated Homebrew tap."
echo ""
echo "Users can install with:"
echo "  brew tap everettjf/tap"
echo "  brew install microclaw"
