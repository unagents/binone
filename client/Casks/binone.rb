# Source of the binone cask. The tap's copy is generated from this file by scripts/release.sh,
# which fills in the three @...@ placeholders (version, sha256 of the dmg, and the web URL of
# the repository whose release holds the dmg). Edit this file, not the tap's copy.
cask "binone" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "@DIST_BASE_URL@/releases/download/v#{version}/binone_#{version}_aarch64.dmg"
  name "binone"
  desc "Client app for binone, a task-driven supporter on top of bantou"
  homepage "@DIST_BASE_URL@"

  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "binone.app"

  preflight_steps do
    # The app is ad-hoc signed, so only the quarantine attribute is removed
    # (re-signing would break the hardened runtime flags).
    run "xattr", args: ["-dr", "com.apple.quarantine", "{{staged_path}}/binone.app"]
  end

  zap trash: [
    "~/Library/Application Support/jp.grrow.binone",
    "~/Library/Caches/jp.grrow.binone",
    "~/Library/WebKit/jp.grrow.binone",
  ]
end
