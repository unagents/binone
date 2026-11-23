#!/usr/bin/env bash
# Release the binone client: build -> register the dmg as a release asset -> update the tap's cask.
#
# Usage: client/scripts/release.sh [--dry-run] [--dmg <path>] [--skip-upload]
#   --dry-run      Build (or take --dmg), compute the sha256, render the cask and print what would
#                  be done. Nothing is sent to the release host or to the tap.
#   --dmg <path>   Use an already built dmg instead of building here (e.g. one built through a
#                  host's build service). Its name must be binone_<version>_aarch64.dmg.
#   --skip-upload  The release v<version> with the dmg was already registered by another tool;
#                  do not create it here. The download is still verified before the tap is updated.
#
# The version is the one in client/package.json (bump it in a commit before releasing).
#
# Where things go is given by the environment; nothing host-specific lives in this repository:
#   BINONE_TAP_URL        git URL of the tap repository (required unless --dry-run)
#   BINONE_TAP_BRANCH     branch of the tap to update (default: main)
#   BINONE_DIST_BASE_URL  web URL of the repository whose release holds the dmg
#                         (default: this clone's origin, i.e. binone's own repository)
#   BINONE_DIST_FORGE     gitea | github (default: github when the host is github.com, else gitea)
#
# Credentials: the token for the release API comes from `git credential fill` at run time and is
# never written to a file or printed. The tap is pushed with git's own credential handling.
set -euo pipefail

usage() {
  sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

DRY_RUN=0
SKIP_UPLOAD=0
DMG_ARG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --skip-upload) SKIP_UPLOAD=1 ;;
    --dmg)
      shift
      DMG_ARG="${1:-}"
      [[ -n "$DMG_ARG" ]] || usage
      ;;
    *) usage ;;
  esac
  shift
done

step() { echo "== $*"; }
die() {
  echo "error: $*" >&2
  exit 1
}

# Homebrew's node/bun/cargo first when present (a version manager's shim may be too old for Nuxt).
[[ -d /opt/homebrew/bin ]] && export PATH="/opt/homebrew/bin:$PATH"

CLIENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR="$(cd "$CLIENT_DIR/.." && pwd)"
CASK_SOURCE="$CLIENT_DIR/Casks/binone.rb"
WORK="$CLIENT_DIR/dist/release"

for tool in git node curl shasum ruby; do
  command -v "$tool" >/dev/null || die "$tool is not installed"
done

VERSION="$(node -p "require('$CLIENT_DIR/package.json').version")"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version in package.json must look like 0.1.0: $VERSION"
grep -q "^version = \"$VERSION\"\$" "$CLIENT_DIR/src-tauri/Cargo.toml" ||
  die "src-tauri/Cargo.toml does not have version $VERSION"
TAG="v$VERSION"
DMG_NAME="binone_${VERSION}_aarch64.dmg"

# --- where the dmg and the cask go -------------------------------------------------------------
strip_url() { # drop credentials, trailing slashes and a trailing .git
  printf '%s' "$1" | sed -E 's#^(https?://)[^/@]*@#\1#; s#/+$##; s#\.git$##'
}
DIST_BASE_URL="${BINONE_DIST_BASE_URL:-}"
if [[ -z "$DIST_BASE_URL" ]]; then
  DIST_BASE_URL="$(git -C "$REPO_DIR" remote get-url origin)"
fi
DIST_BASE_URL="$(strip_url "$DIST_BASE_URL")"
[[ "$DIST_BASE_URL" =~ ^https?://[^/]+(/[^/]+)*/[^/]+/[^/]+$ ]] ||
  die "BINONE_DIST_BASE_URL must be an http(s) URL ending in <owner>/<repo>: $DIST_BASE_URL"
DIST_REPO="$(printf '%s' "$DIST_BASE_URL" | sed -E 's#^.*/([^/]+/[^/]+)$#\1#')"
DIST_ROOT="${DIST_BASE_URL%/"$DIST_REPO"}"
DIST_HOST="$(printf '%s' "$DIST_ROOT" | sed -E 's#^https?://([^/]+).*$#\1#')"
DIST_PROTO="${DIST_ROOT%%://*}"
DIST_FORGE="${BINONE_DIST_FORGE:-}"
if [[ -z "$DIST_FORGE" ]]; then
  if [[ "$DIST_HOST" == "github.com" ]]; then DIST_FORGE=github; else DIST_FORGE=gitea; fi
fi
case "$DIST_FORGE" in
  github) API="https://api.github.com/repos/$DIST_REPO" ;;
  gitea) API="$DIST_ROOT/api/v1/repos/$DIST_REPO" ;;
  *) die "BINONE_DIST_FORGE must be gitea or github: $DIST_FORGE" ;;
esac
DMG_URL="$DIST_BASE_URL/releases/download/$TAG/$DMG_NAME"

TAP_URL="${BINONE_TAP_URL:-}"
TAP_BRANCH="${BINONE_TAP_BRANCH:-main}"
if [[ -z "$TAP_URL" && "$DRY_RUN" == 0 ]]; then
  die "BINONE_TAP_URL is not set (git URL of the tap repository)"
fi

# --- release preconditions (not needed to rehearse with --dry-run) ------------------------------
HEAD_SHA="$(git -C "$REPO_DIR" rev-parse HEAD)"
if [[ "$DRY_RUN" == 0 ]]; then
  [[ -z "$(git -C "$REPO_DIR" status --porcelain)" ]] || die "the working tree has uncommitted changes"
  git -C "$REPO_DIR" fetch -q origin
  [[ -n "$(git -C "$REPO_DIR" branch -r --contains "$HEAD_SHA")" ]] ||
    die "HEAD ($HEAD_SHA) is not pushed to origin; the release tag needs it there"
fi

mkdir -p "$WORK"

# --- build ------------------------------------------------------------------------------------
if [[ -n "$DMG_ARG" ]]; then
  step "use the given dmg"
  [[ -s "$DMG_ARG" ]] || die "dmg not found: $DMG_ARG"
  [[ "$(basename "$DMG_ARG")" == "$DMG_NAME" ]] || die "the dmg must be named $DMG_NAME: $DMG_ARG"
  DMG="$(cd "$(dirname "$DMG_ARG")" && pwd)/$DMG_NAME"
else
  step "build $VERSION"
  for tool in bun cargo; do
    command -v "$tool" >/dev/null || die "$tool is not installed"
  done
  (
    cd "$CLIENT_DIR"
    bun install --frozen-lockfile
    bun run typecheck
    bun run lint
    bun run test
    (cd src-tauri && cargo test)
    # CI=true: no Finder scripting while the dmg is laid out (works without a GUI session)
    CI=true bun run tauri build
  )
  DMG="$CLIENT_DIR/src-tauri/target/release/bundle/dmg/$DMG_NAME"
  [[ -s "$DMG" ]] || die "build output missing: $DMG"
fi
DMG_SHA="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"
echo "dmg: $DMG"
echo "sha256: $DMG_SHA"

# --- render the cask ----------------------------------------------------------------------------
step "render the cask"
CASK="$WORK/binone.rb"
# the leading comment block of the source explains the placeholders; the tap's copy does not need it
sed -E '/^cask /,$!d' "$CASK_SOURCE" |
  sed -e "s|@VERSION@|$VERSION|g" -e "s|@SHA256@|$DMG_SHA|g" -e "s|@DIST_BASE_URL@|$DIST_BASE_URL|g" >"$CASK"
if grep -q '@[A-Z0-9_]*@' "$CASK"; then
  die "the rendered cask still has placeholders: $CASK"
fi
ruby -c "$CASK" >/dev/null || die "the rendered cask is not valid Ruby: $CASK"
cat "$CASK"

if [[ "$DRY_RUN" == 1 ]]; then
  step "dry-run: nothing is sent"
  if [[ "$SKIP_UPLOAD" == 1 ]]; then
    echo "would expect release $TAG with $DMG_NAME to exist already on $DIST_BASE_URL (--skip-upload)"
  else
    echo "would create release $TAG at $HEAD_SHA on $DIST_BASE_URL ($DIST_FORGE API) and attach $DMG_NAME"
  fi
  echo "would verify that $DMG_URL downloads without credentials and matches the sha256"
  echo "would write Casks/binone.rb to ${TAP_URL:-<BINONE_TAP_URL is not set>} ($TAP_BRANCH) and push it"
  echo "dry-run ok: $VERSION (dmg sha256 $DMG_SHA); rendered cask: $CASK"
  exit 0
fi

# --- register the dmg ---------------------------------------------------------------------------
# The token goes to curl through a config read from a pipe, so it never shows up in a file,
# in the process list or in this script's output.
api() {
  local token
  token="$(printf 'protocol=%s\nhost=%s\n\n' "$DIST_PROTO" "$DIST_HOST" |
    GIT_TERMINAL_PROMPT=0 git credential fill 2>/dev/null | sed -n 's/^password=//p')"
  [[ -n "$token" ]] || die "git credential fill returned no credential for $DIST_PROTO://$DIST_HOST"
  curl -sS -K <(printf 'header = "Authorization: token %s"\n' "$token") "$@"
}

if [[ "$SKIP_UPLOAD" == 0 ]]; then
  step "create release $TAG"
  CODE="$(api -o /dev/null -w '%{http_code}' "$API/releases/tags/$TAG")"
  [[ "$CODE" == 404 ]] || die "release $TAG already exists or cannot be checked (HTTP $CODE)"
  BODY="$(node -e 'console.log(JSON.stringify({tag_name: process.argv[1], target_commitish: process.argv[2], name: process.argv[1], body: ""}))' "$TAG" "$HEAD_SHA")"
  CODE="$(api -o "$WORK/release.json" -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d "$BODY" "$API/releases")"
  [[ "$CODE" == 201 ]] || die "creating the release failed (HTTP $CODE); see $WORK/release.json"
  RELEASE_ID="$(node -p "require('$WORK/release.json').id")"

  step "attach $DMG_NAME"
  if [[ "$DIST_FORGE" == github ]]; then
    CODE="$(api -o "$WORK/asset.json" -w '%{http_code}' -X POST -H 'Content-Type: application/octet-stream' \
      --data-binary "@$DMG" "https://uploads.github.com/repos/$DIST_REPO/releases/$RELEASE_ID/assets?name=$DMG_NAME")"
  else
    CODE="$(api -o "$WORK/asset.json" -w '%{http_code}' -X POST \
      -F "attachment=@$DMG;type=application/octet-stream" "$API/releases/$RELEASE_ID/assets?name=$DMG_NAME")"
  fi
  [[ "$CODE" == 201 ]] || die "attaching the dmg failed (HTTP $CODE); release $TAG now exists without it; see $WORK/asset.json"
fi

step "verify the download (no credentials)"
curl -fsSL -o "$WORK/download.dmg" "$DMG_URL" || die "could not download $DMG_URL without credentials (is the repository public?)"
GOT_SHA="$(shasum -a 256 "$WORK/download.dmg" | cut -d' ' -f1)"
[[ "$GOT_SHA" == "$DMG_SHA" ]] || die "the downloaded dmg does not match (got $GOT_SHA, expected $DMG_SHA)"

# --- update the tap -----------------------------------------------------------------------------
step "update the tap"
TAP_DIR="$(mktemp -d "$WORK/tap.XXXXXX")"
git clone -q --depth 1 --branch "$TAP_BRANCH" "$TAP_URL" "$TAP_DIR"
mkdir -p "$TAP_DIR/Casks"
cp "$CASK" "$TAP_DIR/Casks/binone.rb"
git -C "$TAP_DIR" add Casks/binone.rb
if git -C "$TAP_DIR" diff --cached --quiet; then
  echo "the tap already has this cask; nothing to push"
else
  git -C "$TAP_DIR" commit -q -m "binone $VERSION"
  git -C "$TAP_DIR" push -q origin "HEAD:$TAP_BRANCH"
  echo "pushed Casks/binone.rb ($VERSION) to the tap"
fi

# client/dist/ is not tracked; the clone of the tap and the downloaded copy stay there until the
# next run's `git clean` or a manual removal.
echo "released $VERSION ($TAG at $HEAD_SHA, dmg sha256 $DMG_SHA)"
