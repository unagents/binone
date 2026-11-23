#!/usr/bin/env bash
# Release the binone client: make the dmg -> register it as a release asset -> update the tap's cask.
#
# Usage: client/scripts/release.sh (--app <path> | --dmg <path>) [--dry-run] [--skip-upload]
#                                  [--allow-insecure-dist]
#   --app <path>   The signed binone.app to release. Build it first with the host's build service
#                  (bundle type `app` only) and pass the path it reports. The script packs it into
#                  binone_<version>_aarch64.dmg with hdiutil (no Finder, no AppleScript).
#   --dmg <path>   Use an already made dmg instead of --app. Its name must be
#                  binone_<version>_aarch64.dmg.
#   --dry-run      Make the dmg (or take --dmg), compute the sha256, render the cask and print what
#                  would be done. Nothing is sent to the release host or to the tap.
#   --skip-upload  The release v<version> with the dmg was already registered and published by
#                  another tool; do not create it here. The download is still verified before the
#                  tap is updated.
#   --allow-insecure-dist
#                  Accept a BINONE_DIST_BASE_URL that is not https or whose host is an IP address.
#                  Without this the script stops (exit code 2) before anything is sent, because
#                  that URL is written into the cask that users install from.
#
# The version is the one in client/package.json (bump it in a commit before releasing).
#
# Where things go is given by the environment; nothing host-specific lives in this repository
# and there is no default taken from this clone:
#   BINONE_DIST_BASE_URL  web URL of the repository whose release holds the dmg, ending in
#                         <owner>/<repo> (required unless --dry-run)
#   BINONE_TAP_URL        git URL of the tap repository (required unless --dry-run)
#   BINONE_TAP_BRANCH     branch of the tap to update (default: main)
#   BINONE_DIST_FORGE     gitea | github (default: github when the host is github.com, else gitea)
#
# Credentials: the token for the release API comes from `git credential fill` at run time and is
# never written to a file or printed. The tap is pushed with git's own credential handling.
# URLs are printed without the credentials they may carry.
#
# Exit codes: 0 done, 1 a step failed, 2 wrong usage or settings (nothing was sent).
set -euo pipefail

usage() {
  sed -n '2,35p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

DRY_RUN=0
SKIP_UPLOAD=0
ALLOW_INSECURE_DIST=0
APP_ARG=""
DMG_ARG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --skip-upload) SKIP_UPLOAD=1 ;;
    --allow-insecure-dist) ALLOW_INSECURE_DIST=1 ;;
    --app)
      shift
      APP_ARG="${1:-}"
      [[ -n "$APP_ARG" ]] || usage
      ;;
    --dmg)
      shift
      DMG_ARG="${1:-}"
      [[ -n "$DMG_ARG" ]] || usage
      ;;
    *) usage ;;
  esac
  shift
done

# Drops the credentials of every URL in the text (scheme://user:secret@host -> scheme://host).
redact() {
  sed -E 's#([A-Za-z][A-Za-z0-9+.-]*://)[^/[:space:]]*@#\1#g'
}
step() { echo "== $*"; }
die() {
  printf 'error: %s\n' "$*" | redact >&2
  exit 1
}
# Wrong usage or settings: stops before anything is sent.
refuse() {
  printf 'error: %s\n' "$*" | redact >&2
  exit 2
}

if [[ -n "$APP_ARG" && -n "$DMG_ARG" ]]; then
  refuse "give --app or --dmg, not both"
fi
if [[ -z "$APP_ARG" && -z "$DMG_ARG" ]]; then
  refuse "give the signed app with --app <path> (or a dmg made from it with --dmg <path>); see docs/release.md"
fi

# Homebrew's node first when present (a version manager's shim may be too old).
[[ -d /opt/homebrew/bin ]] && export PATH="/opt/homebrew/bin:$PATH"

CLIENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR="$(cd "$CLIENT_DIR/.." && pwd)"
CASK_SOURCE="$CLIENT_DIR/Casks/binone.rb"
# Under src-tauri/target: not tracked, and outside `dist` (a link into .output that a build replaces).
WORK="$CLIENT_DIR/src-tauri/target/release-work"

for tool in git node curl shasum ruby hdiutil codesign ditto lipo; do
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
  printf '%s' "$1" | redact | sed -E 's#/+$##; s#\.git$##'
}
DIST_BASE_URL="${BINONE_DIST_BASE_URL:-}"
DIST_PLACEHOLDER=0
if [[ -z "$DIST_BASE_URL" ]]; then
  [[ "$DRY_RUN" == 1 ]] ||
    refuse "BINONE_DIST_BASE_URL is not set (web URL of the repository whose release holds the dmg)"
  # Rehearsal without a destination: the cask is rendered with a URL that leads nowhere.
  DIST_BASE_URL="https://dist.invalid/OWNER/REPO"
  DIST_PLACEHOLDER=1
fi
DIST_BASE_URL="$(strip_url "$DIST_BASE_URL")"
[[ "$DIST_BASE_URL" =~ ^https?://[^/]+(/[^/]+)*/[^/]+/[^/]+$ ]] ||
  refuse "BINONE_DIST_BASE_URL must be an http(s) URL ending in <owner>/<repo>: $DIST_BASE_URL"
DIST_REPO="$(printf '%s' "$DIST_BASE_URL" | sed -E 's#^.*/([^/]+/[^/]+)$#\1#')"
DIST_ROOT="${DIST_BASE_URL%/"$DIST_REPO"}"
DIST_HOST="$(printf '%s' "$DIST_ROOT" | sed -E 's#^https?://([^/]+).*$#\1#')" # may carry a port
DIST_PROTO="${DIST_ROOT%%://*}"
DIST_FORGE="${BINONE_DIST_FORGE:-}"
if [[ -z "$DIST_FORGE" ]]; then
  if [[ "$DIST_HOST" == "github.com" ]]; then DIST_FORGE=github; else DIST_FORGE=gitea; fi
fi
case "$DIST_FORGE" in
  github) API="https://api.github.com/repos/$DIST_REPO" ;;
  gitea) API="$DIST_ROOT/api/v1/repos/$DIST_REPO" ;;
  *) refuse "BINONE_DIST_FORGE must be gitea or github: $DIST_FORGE" ;;
esac
DMG_URL="$DIST_BASE_URL/releases/download/$TAG/$DMG_NAME"

# The dist URL ends up in the cask, in a public tap. A plain-http URL lets the download be
# replaced on the way, and an IP address is specific to one network and should not be published.
DIST_HOSTNAME="$(printf '%s' "$DIST_HOST" | sed -E 's#^(\[[^]]*\]|[^:]*)(:[0-9]*)?$#\1#')"
DIST_INSECURE=""
[[ "$DIST_PROTO" == https ]] || DIST_INSECURE="it is not https"
# IPv4 in any numeric spelling (192.0.2.1, 0xc0.0.2.1, 3221225985) or a bracketed IPv6 address.
IPV4_PART='(0[xX][0-9a-fA-F]+|[0-9]+)'
if [[ "$DIST_HOSTNAME" == \[* || "$DIST_HOSTNAME" =~ ^$IPV4_PART(\.$IPV4_PART)*\.?$ ]]; then
  DIST_INSECURE="${DIST_INSECURE:+$DIST_INSECURE and }its host is an IP address"
fi
if [[ -n "$DIST_INSECURE" ]]; then
  if [[ "$ALLOW_INSECURE_DIST" == 1 ]]; then
    echo "warning: BINONE_DIST_BASE_URL is accepted although $DIST_INSECURE (--allow-insecure-dist): $DIST_BASE_URL"
  else
    refuse "BINONE_DIST_BASE_URL is refused because $DIST_INSECURE: $DIST_BASE_URL
       This URL is written into the cask. Use an https URL with a host name, or pass
       --allow-insecure-dist to release to it anyway. Nothing was sent."
  fi
fi

TAP_URL="${BINONE_TAP_URL:-}"
TAP_BRANCH="${BINONE_TAP_BRANCH:-main}"
if [[ -z "$TAP_URL" && "$DRY_RUN" == 0 ]]; then
  refuse "BINONE_TAP_URL is not set (git URL of the tap repository)"
fi
# For printing only; git gets $TAP_URL itself.
TAP_URL_SHOWN="$(printf '%s' "$TAP_URL" | redact)"

if [[ "$DRY_RUN" == 1 ]]; then
  [[ "$DIST_PLACEHOLDER" == 0 ]] ||
    echo "note: BINONE_DIST_BASE_URL is not set; the cask is rendered with the placeholder $DIST_BASE_URL"
  [[ -n "$TAP_URL" ]] || echo "note: BINONE_TAP_URL is not set"
fi

# --- release preconditions (not needed to rehearse with --dry-run) ------------------------------
HEAD_SHA="$(git -C "$REPO_DIR" rev-parse HEAD)"
if [[ "$DRY_RUN" == 0 ]]; then
  [[ -z "$(git -C "$REPO_DIR" status --porcelain)" ]] || die "the working tree has uncommitted changes"
  git -C "$REPO_DIR" fetch -q origin 2>&1 | redact >&2 || die "git fetch origin failed"
  [[ -n "$(git -C "$REPO_DIR" branch -r --contains "$HEAD_SHA")" ]] ||
    die "HEAD ($HEAD_SHA) is not pushed to origin; the release tag needs it there"
fi

mkdir -p "$WORK"
# What must not outlive the run: the tap's clone (its config holds the tap's URL), the copy of
# the dmg fetched for the check, the staging folder of the dmg and the API's answers.
STAGE_DIR=""
TAP_DIR=""
cleanup() {
  [[ -z "$STAGE_DIR" ]] || rm -rf "$STAGE_DIR"
  [[ -z "$TAP_DIR" ]] || rm -rf "$TAP_DIR"
  rm -f "$WORK/download.dmg" "$WORK/release.json" "$WORK/asset.json" "$WORK/publish.json"
}
trap cleanup EXIT

# --- the dmg ------------------------------------------------------------------------------------
if [[ -n "$DMG_ARG" ]]; then
  step "use the given dmg"
  [[ -s "$DMG_ARG" ]] || die "dmg not found: $DMG_ARG"
  [[ "$(basename "$DMG_ARG")" == "$DMG_NAME" ]] || die "the dmg must be named $DMG_NAME: $DMG_ARG"
  DMG="$(cd "$(dirname "$DMG_ARG")" && pwd)/$DMG_NAME"
else
  step "make $DMG_NAME from the app"
  APP="${APP_ARG%/}"
  [[ -d "$APP" ]] || die "app not found: $APP"
  [[ "$(basename "$APP")" == "binone.app" ]] || die "the app must be named binone.app (the cask installs that name): $APP"
  codesign --verify --deep --strict "$APP" || die "the app's signature does not verify: $APP"
  APP_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
  [[ "$APP_VERSION" == "$VERSION" ]] ||
    die "the app is version $APP_VERSION, but package.json says $VERSION; build the app again"
  APP_EXECUTABLE="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
  APP_ARCHS="$(lipo -archs "$APP/Contents/MacOS/$APP_EXECUTABLE")"
  [[ " $APP_ARCHS " == *" arm64 "* ]] || die "the app is not built for arm64 (it has: $APP_ARCHS)"

  # A plain folder turned into a compressed image: the app and a link to /Applications to drag it
  # onto. No window layout is set, so neither Finder nor AppleScript is involved.
  STAGE_DIR="$(mktemp -d "$WORK/dmg-root.XXXXXX")"
  ditto "$APP" "$STAGE_DIR/binone.app"
  ln -s /Applications "$STAGE_DIR/Applications"
  codesign --verify --deep --strict "$STAGE_DIR/binone.app" || die "the copy of the app for the dmg does not verify"
  DMG="$WORK/$DMG_NAME"
  rm -f "$DMG"
  hdiutil create -quiet -volname binone -srcfolder "$STAGE_DIR" -fs HFS+ -format UDZO -ov "$DMG" ||
    die "hdiutil could not create $DMG"
  hdiutil verify -quiet "$DMG" || die "the new dmg does not verify: $DMG"
  rm -rf "$STAGE_DIR"
  STAGE_DIR=""
fi
DMG_SHA="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"
DMG_SIZE="$(stat -f %z "$DMG")"
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
    echo "would expect the published release $TAG with $DMG_NAME to exist already on $DIST_BASE_URL (--skip-upload)"
  else
    echo "would create release $TAG at $HEAD_SHA on $DIST_BASE_URL ($DIST_FORGE API) as a draft, attach $DMG_NAME and publish it"
  fi
  echo "would verify that $DMG_URL downloads without credentials and matches the sha256"
  echo "would write Casks/binone.rb to ${TAP_URL_SHOWN:-<BINONE_TAP_URL is not set>} ($TAP_BRANCH) and push it"
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
  curl -sS -K <(auth_config "$token") "$@"
}
# The curl config line that carries the token. Inside the double quotes of a curl config a
# backslash and a double quote are written with a backslash before them.
auth_config() {
  local token="$1" backslash='\' quote='"'
  token="${token//"$backslash"/$backslash$backslash}"
  token="${token//"$quote"/$backslash$quote}"
  printf 'header = "Authorization: token %s"\n' "$token"
}
# The forge's reason for a refusal, from its JSON answer (nothing else of the answer is shown).
api_message() {
  node -e '
    try { const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).message;
          if (typeof m === "string") console.log(m.slice(0, 300)); } catch {}' "$1" 2>/dev/null | redact
}
set_draft() { # <true|false>; prints the HTTP status
  api -o "$WORK/publish.json" -w '%{http_code}' -X PATCH -H 'Content-Type: application/json' \
    -d "{\"draft\": $1}" "$API/releases/$RELEASE_ID"
}

RELEASE_ID=""
if [[ "$SKIP_UPLOAD" == 0 ]]; then
  # The release stays a draft (not visible, no tag) until the dmg is attached to it.
  step "create release $TAG as a draft"
  CODE="$(api -o /dev/null -w '%{http_code}' "$API/releases/tags/$TAG")"
  [[ "$CODE" == 404 ]] || die "release $TAG already exists or cannot be checked (HTTP $CODE)"
  BODY="$(node -e 'console.log(JSON.stringify({tag_name: process.argv[1], target_commitish: process.argv[2], name: process.argv[1], body: "", draft: true}))' "$TAG" "$HEAD_SHA")"
  CODE="$(api -o "$WORK/release.json" -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d "$BODY" "$API/releases")"
  [[ "$CODE" == 201 ]] || die "creating the release failed (HTTP $CODE): $(api_message "$WORK/release.json")"
  RELEASE_ID="$(node -p "require('$WORK/release.json').id")"
  [[ "$RELEASE_ID" =~ ^[0-9]+$ ]] || die "the release was created, but its id could not be read"

  step "attach $DMG_NAME"
  if [[ "$DIST_FORGE" == github ]]; then
    CODE="$(api -o "$WORK/asset.json" -w '%{http_code}' -X POST -H 'Content-Type: application/octet-stream' \
      --data-binary "@$DMG" "https://uploads.github.com/repos/$DIST_REPO/releases/$RELEASE_ID/assets?name=$DMG_NAME")"
  else
    CODE="$(api -o "$WORK/asset.json" -w '%{http_code}' -X POST \
      -F "attachment=@$DMG;type=application/octet-stream" "$API/releases/$RELEASE_ID/assets?name=$DMG_NAME")"
  fi
  [[ "$CODE" == 201 ]] ||
    die "attaching the dmg failed (HTTP $CODE): $(api_message "$WORK/asset.json")
       The draft release $TAG (id $RELEASE_ID) is left without it and is not public; delete the draft and run again."
  # What the host stored must be the file that was sent.
  ASSET_CHECK="$(node -e '
    const a = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const [name, size, sha] = process.argv.slice(2);
    if (a.name !== name) console.log(`its name is ${a.name}`);
    else if (String(a.size) !== size) console.log(`its size is ${a.size}, not ${size}`);
    else if (typeof a.digest === "string" && a.digest !== `sha256:${sha}`) console.log("its digest differs");
  ' "$WORK/asset.json" "$DMG_NAME" "$DMG_SIZE" "$DMG_SHA")"
  [[ -z "$ASSET_CHECK" ]] ||
    die "the attached dmg is not what was sent ($ASSET_CHECK). The draft release $TAG (id $RELEASE_ID) is not public; delete the draft and run again."

  step "publish release $TAG"
  CODE="$(set_draft false)"
  [[ "$CODE" == 200 ]] ||
    die "publishing the release failed (HTTP $CODE): $(api_message "$WORK/publish.json")
       The draft release $TAG (id $RELEASE_ID) has the dmg and is not public. Publish it on the host and
       run again with --dmg <the same dmg> --skip-upload, or delete the draft and run again."
fi

# Only a published release can be fetched without credentials, so this check comes after the
# switch. When it fails, a release made here goes back to a draft and the tap is not touched.
step "verify the download (no credentials)"
back_to_draft() {
  [[ -n "$RELEASE_ID" ]] || return 0
  if [[ "$(set_draft true 2>/dev/null)" == 200 ]]; then
    echo "release $TAG (id $RELEASE_ID) was put back to a draft; its tag $TAG may remain on the host" >&2
  else
    echo "release $TAG (id $RELEASE_ID) could not be put back to a draft: it is PUBLIC; withdraw it on the host" >&2
  fi
}
if ! curl -fsSL --proto '=https,http' -o "$WORK/download.dmg" "$DMG_URL"; then
  back_to_draft
  die "could not download $DMG_URL without credentials (is the repository public?)"
fi
GOT_SHA="$(shasum -a 256 "$WORK/download.dmg" | cut -d' ' -f1)"
if [[ "$GOT_SHA" != "$DMG_SHA" ]]; then
  back_to_draft
  die "the downloaded dmg does not match (got $GOT_SHA, expected $DMG_SHA)"
fi

# --- update the tap -----------------------------------------------------------------------------
step "update the tap"
TAP_DIR="$(mktemp -d "$WORK/tap.XXXXXX")"
git clone -q --depth 1 --branch "$TAP_BRANCH" "$TAP_URL" "$TAP_DIR" 2>&1 | redact >&2 ||
  die "could not clone the tap ($TAP_URL_SHOWN, branch $TAP_BRANCH)"
# The commit's author is fixed here, so it never comes from the personal settings of whoever runs this.
git -C "$TAP_DIR" config user.name claude.cli
git -C "$TAP_DIR" config user.email claude.cli@local.host
mkdir -p "$TAP_DIR/Casks"
cp "$CASK" "$TAP_DIR/Casks/binone.rb"
git -C "$TAP_DIR" add Casks/binone.rb
if git -C "$TAP_DIR" diff --cached --quiet; then
  echo "the tap already has this cask; nothing to push"
else
  git -C "$TAP_DIR" commit -q -m "binone $VERSION" -- Casks/binone.rb
  git -C "$TAP_DIR" push -q origin "HEAD:$TAP_BRANCH" 2>&1 | redact >&2 ||
    die "could not push to the tap ($TAP_URL_SHOWN, branch $TAP_BRANCH). The release $TAG is published;
       fix the cause and run again with --dmg <the same dmg> --skip-upload."
  echo "pushed Casks/binone.rb ($VERSION) to the tap"
fi

echo "released $VERSION ($TAG at $HEAD_SHA, dmg sha256 $DMG_SHA)"
