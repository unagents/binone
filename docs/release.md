# Installing and releasing the client

The client app (`client/`) is distributed as a Homebrew cask for Apple Silicon Macs
(macOS 15 Sequoia or later). The app is ad-hoc signed; the cask removes the quarantine
attribute when it installs the app. The app has no in-app updater: updates come through brew.

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

## Release

One script does the whole release: it builds the app, registers the dmg as an asset of the
release `v<version>`, checks that the dmg downloads without credentials, and writes the cask
to the tap.

1. Bump the version in `client/package.json` and `client/src-tauri/Cargo.toml` (both must
   match; `tauri.conf.json` reads `package.json`), commit and push.
2. Rehearse. Nothing is sent anywhere:

   ```sh
   client/scripts/release.sh --dry-run
   ```

3. Release:

   ```sh
   BINONE_TAP_URL=<git URL of the tap repository> client/scripts/release.sh
   ```

### Settings

Where the dmg and the cask go is passed in at run time. No host name, address or token is
stored in this repository or in the cask source.

| Variable | Meaning | Default |
| --- | --- | --- |
| `BINONE_TAP_URL` | git URL of the tap repository | required (except with `--dry-run`) |
| `BINONE_TAP_BRANCH` | branch of the tap to update | `main` |
| `BINONE_DIST_BASE_URL` | web URL of the repository whose release holds the dmg | this clone's `origin` (binone's own repository) |
| `BINONE_DIST_FORGE` | API of that host: `gitea` or `github` | `github` for github.com, otherwise `gitea` |

The repository that holds the dmg must be readable without credentials, because brew downloads
the dmg anonymously. The script checks this before it touches the tap.

### Options

- `--dry-run`: build, compute the sha256, render the cask to `client/dist/release/binone.rb`
  and print what would be done.
- `--dmg <path>`: use an already built `binone_<version>_aarch64.dmg` instead of building in
  the script (for hosts where builds go through a build service).
- `--skip-upload`: the release with the dmg was already registered by another tool; only
  verify the download and update the tap.

### Requirements on the release host

- Apple Silicon Mac with the Xcode Command Line Tools, Rust (cargo), Node.js 20.19 or later,
  and bun. `ruby`, `curl` and `shasum` come with macOS.
- A git credential for the host of `BINONE_DIST_BASE_URL` that may create releases, served by
  a git credential helper. The script reads the token with `git credential fill` when it
  runs; it does not write it to a file or print it.
- Push access to the tap through git (the same credential helper mechanism).

### The cask

`client/Casks/binone.rb` is the source of the cask. It carries placeholders (`@VERSION@`,
`@SHA256@`, `@DIST_BASE_URL@`) that the release script fills in; the tap's
`Casks/binone.rb` is always generated from it. Change the cask here, never in the tap.

### If a release stops halfway

- Failed before "create release": nothing was published. Fix the cause and run again.
- Failed while attaching the dmg: the release `v<version>` exists without the asset. Delete
  that release (and its tag) on the host, then run again.
- Failed at the download check or the tap update: the release is in place. Fix the cause and
  run again with `--dmg <the same dmg> --skip-upload`.
