# Installing and releasing the client

The client app (`client/`) is distributed as a Homebrew cask for Apple Silicon Macs
(macOS 15 Sequoia or later). The app is ad-hoc signed; the cask removes the quarantine
attribute when it installs the app. The app has no auto-updater (no Tauri updater plugin, no update signing key): it checks for a newer
version and runs the brew update when asked (see "Checking and updating from the app").

## Install (on the user's Mac)

```sh
brew tap unagents/tap
brew install --cask unagents/tap/binone
```

Open binone and enter the server's IP. (Enter a port only when the server's port was changed
from the default.) The Mac must be inside the IP range the server allows; macOS may ask once
for permission to reach devices on the local network.

Upgrade to a newer release:

```sh
brew update
brew upgrade --cask unagents/tap/binone
```

`brew tap <user>/<name>` without a URL looks for `<user>/homebrew-<name>` on GitHub. For a tap
hosted elsewhere, pass its git URL as a second argument: `brew tap <user>/<name> <git URL>`.

## Checking and updating from the app

The native side asks GitHub's public API (no credentials) for the latest release of the
client's repository, once after startup and when "確認" on the settings screen is pressed; there
is no periodic check. The tag, without its leading `v`, is compared with the running version as
three numbers (`0.10.0` is newer than `0.9.0`). Drafts, prereleases and tags that are not a plain
`X.Y.Z` are not candidates. A failure (no network, rate limit, an answer of another shape)
shows as "確認できません" with the reason on the settings screen, in Japanese and by kind (not
reachable, refused with the HTTP status, no release, an answer of another shape, other), and does
not affect the connection or the other screens. When a newer version exists, every screen's header
shows "新しい版 X.Y.Z", which opens the version card of the settings screen.

How to update is found out with the check, only when a newer version exists. It looks for
Homebrew at `/opt/homebrew/bin/brew` and `/usr/local/bin/brew` (not through `PATH`), and asks each
one that exists whether it has the cask (`brew list --cask binone` succeeds, with a 10 second
limit for each call), for an app that is an app bundle. When both have it, the one whose list names
the running app's bundle is used, and `/opt/homebrew` when that cannot be told. With one, "更新" is
shown. The update runs these, as program-and-arguments and not through a shell: the tap's
directory (`brew --repo unagents/tap`) and `git pull --ff-only` in it, so that only that tap is
refreshed (not `brew update`, which refreshes every tap); then `brew upgrade --cask binone` with
`HOMEBREW_NO_AUTO_UPDATE=1`; then `brew list --cask --versions binone`. The update counts as done
only when the installed version is the latest one the check found: when it is not (brew ended with
code 0 and did nothing) the screen says it was not updated, with both versions, and when the
installed version cannot be read it says that this could not be confirmed. A tap that cannot be
refreshed does not stop the update; the screen says so after the result. The whole update, the
judgment above included, has 10 minutes in all. When it runs out of time everything it started is
stopped with it, child processes too (the whole process group: TERM first, then KILL after a short
grace), and reaped; the screen says the update ran out of time and shows the end of the output so
far, and "更新" can be pressed again. A failure shows the exit code and the end of the output; a
button starts the app again after a success (it never restarts by itself). Without Homebrew, or for
an app that the cask did not install (a development build, or a cask in neither Homebrew), "更新"
is not shown and nothing is run: the screen offers, as soon as the newer version is known, the
release page in the default browser and the command `brew upgrade --cask binone`. The update does
not touch the app's data directory, so the saved address and key stay.

With `NUXT_PUBLIC_SAMPLE_DATA=1` the states can be drawn with `?sample-update=latest`,
`?sample-update=available` (update by Homebrew), `?sample-update=manual` (newer, by hand) and
`?sample-update=unknown`.

## Release

A release has two parts. The app is built and signed with Tauri's build on the release host; then
one script packs it into a dmg, registers the dmg as an asset of the release `v<version>`,
checks that it downloads without credentials, and writes the cask to the tap.

1. Bump the version in `client/package.json` and `client/src-tauri/Cargo.toml` (both must
   match; `tauri.conf.json` reads `package.json`), commit and push.
2. Check the code, in `client/`:

   ```sh
   bun install --frozen-lockfile
   bun run typecheck && bun run lint && bun run test
   (cd src-tauri && cargo test)
   ```

3. Build `binone.app`: run Tauri's build in `client/` with the bundle type `app` only, and
   take the path of the `binone.app` it made to pass with `--app`. For example, with the
   Tauri CLI:

   ```sh
   bun run tauri build --bundles app
   # -> client/src-tauri/target/release/bundle/macos/binone.app
   ```

   The build signs the app (ad-hoc, as set in `tauri.conf.json`). The release script does not
   build, and the dmg is not made by Tauri (its dmg bundler drives Finder through AppleScript,
   which times out on a host without a desktop session, error -1712).

   The build must not leave the build machine's paths in the binary. `src-tauri/.cargo/config.toml`
   sets a rustc wrapper (`.cargo/remap-paths.sh`) that replaces the home directory and cargo's
   directories with fixed names. A `RUSTC_WRAPPER` in the environment takes precedence over it
   and switches this off, so check the result; this must print 0:

   ```sh
   strings src-tauri/target/release/bundle/macos/binone.app/Contents/MacOS/binone | grep -c "/Users/"
   ```
4. Rehearse with that app. Nothing is sent anywhere:

   ```sh
   client/scripts/release.sh --dry-run --app <path of binone.app>
   ```

   This makes `client/src-tauri/target/release-work/binone_<version>_aarch64.dmg` and prints
   its sha256 and the cask.
5. Release:

   ```sh
   BINONE_DIST_BASE_URL=<web URL of the repository that holds the dmg> \
   BINONE_TAP_URL=<git URL of the tap repository> \
     client/scripts/release.sh --app <path of binone.app>
   ```

### The dmg

The script copies the app into a folder next to a link to `/Applications` and turns the folder
into a compressed image with `hdiutil create` (format UDZO, volume name `binone`). Before that
it checks the app: its name is `binone.app`, `codesign --verify --deep --strict` passes, its
version is the one in `package.json`, and it is built for arm64. No window layout is set, so
neither Finder nor AppleScript is used.

### Settings

Where the dmg and the cask go is passed in at run time. No host name, address or token is
stored in this repository or in the cask source, and nothing is taken from the clone's
`origin`.

| Variable | Meaning | Default |
| --- | --- | --- |
| `BINONE_DIST_BASE_URL` | web URL of the repository whose release holds the dmg, ending in `<owner>/<repo>` | required (except with `--dry-run`) |
| `BINONE_TAP_URL` | git URL of the tap repository | required (except with `--dry-run`) |
| `BINONE_TAP_BRANCH` | branch of the tap to update | `main` |
| `BINONE_DIST_FORGE` | API of that host: `gitea` or `github` | `github` for github.com, otherwise `gitea` |

The repository that holds the dmg must be readable without credentials, because brew downloads
the dmg anonymously. The script checks this before it touches the tap.

`BINONE_DIST_BASE_URL` is written into the cask, which is public. The script therefore stops
with exit code 2, before anything is sent, when that URL is not `https` or when its host is an
IP address. Pass `--allow-insecure-dist` to release to such a URL anyway; over `http` the
token for the release API is sent in clear text as well, and the script warns about it. The
URL may only have letters, digits and `._~:/[]-`, and `BINONE_DIST_FORGE=github` is accepted
only for a URL on `github.com`; otherwise the script stops with exit code 2. With `--dry-run`
and no `BINONE_DIST_BASE_URL`, the cask is rendered with a placeholder URL and the script says so.

URLs are printed without the credentials they may carry.

### Options

- `--app <path>`: the signed `binone.app` to pack into the dmg and release.
- `--dmg <path>`: use an already made `binone_<version>_aarch64.dmg` instead of `--app` (for
  example the one a rehearsal or an earlier, interrupted run made).
- `--dry-run`: make the dmg (or take `--dmg`), compute the sha256, render the cask to
  `client/src-tauri/target/release-work/binone.rb` and print what would be done.
- `--skip-upload`: the release with the dmg was already registered and published by another
  tool (for example the host's web page or its command-line client); only verify the download
  and update the tap.
- `--allow-insecure-dist`: accept a `BINONE_DIST_BASE_URL` that is not `https` or whose host
  is an IP address.

Exit codes: 0 done; 1 a step failed; 2 wrong usage or settings, with nothing sent.

### What the script does on the release host

1. Before anything is sent, checks that the tap and its branch exist (`git ls-remote`), that
   the repository that holds the dmg has the commit that is checked out, and that neither the
   tag `v<version>` nor a release of that name exists there yet. It stops when one of these
   does not hold.
2. Creates the release `v<version>` as a draft, at the commit that is checked out. A draft is
   not visible to others.
3. Attaches the dmg to the draft and compares what the host stored (name, size, and the digest
   when the host reports one) with the file that was sent.
4. Publishes the release.
5. Downloads the dmg from its public URL without credentials and compares the sha256. A
   redirect is followed to `https` only. A draft cannot be fetched without credentials, so
   this check can only come after publishing; when it fails, the script puts the release back
   to a draft and stops.
6. Clones the tap, writes `Casks/binone.rb`, commits that file only and pushes. The commit's
   author is set in the clone (`claude.cli`), not taken from the settings of whoever runs the
   script.

The work directory is `client/src-tauri/target/release-work` (not tracked). The dmg and the
rendered cask stay there; the clone of the tap and the downloaded copy of the dmg are removed
when the script ends, also when it fails.

### Requirements on the release host

- Apple Silicon Mac with the Xcode Command Line Tools and Node.js 20.19 or later. `ruby`,
  `curl`, `shasum`, `hdiutil`, `codesign`, `ditto` and `lipo` come with macOS or the Command
  Line Tools. Building the app (step 3) also needs Rust (cargo) and bun.
- A git credential for the host of `BINONE_DIST_BASE_URL` that may create releases, served by
  a git credential helper. The script reads the token with `git credential fill` when it
  runs; it does not write it to a file or print it.
- Push access to the tap through git (the same credential helper mechanism).

### The cask

`client/Casks/binone.rb` is the source of the cask. It carries placeholders (`@VERSION@`,
`@SHA256@`, `@DIST_BASE_URL@`) that the release script fills in; the tap's
`Casks/binone.rb` is always generated from it. Change the cask here, never in the tap.

### If a release stops halfway

The script says which of these happened.

- Stopped with exit code 2, or failed before "create release": nothing was sent. Fix the
  cause and run again.
- Failed while attaching the dmg, or the attached file did not match: a draft release exists
  and is not public. Delete the draft on the host, then run again.
- The host answered to creating the release, but not with the JSON of its API: the script
  says so. Check `BINONE_DIST_BASE_URL` and `BINONE_DIST_FORGE`; if a draft release now exists
  on the host, delete it, then run again.
- Failed while publishing: the draft has the dmg and is not public. Either delete the draft
  and run again, or publish it on the host and run again with
  `--dmg <the same dmg> --skip-upload`.
- Failed at the download check: the script put the release back to a draft (the tag
  `v<version>` may remain on the host). Fix the cause (usually: the repository is not
  readable without credentials), delete the draft and the tag, and run again. If the script
  reports that it could not put the release back, the release is public: withdraw it on the
  host.
- Failed at the tap update: the release is published and correct. Fix the cause and run
  again with `--dmg <the same dmg> --skip-upload`.
