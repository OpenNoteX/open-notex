# Releasing

A release is a tag. CI builds the arm64 bundle from it, proves the artifact
runs, and publishes a **draft** release. A person installs that draft before it
goes public, because a published release is hard to take back.

## Cut one

```sh
# 1. The version, in the one place it lives.
$EDITOR Cargo.toml          # [workspace.package] version = "0.1.0"

# 2. The notes, under the same version.
$EDITOR CHANGELOG.md        # ## [0.1.0] - 2026-11-18

# 3. Commit both, then tag the commit the artifact was built from.
git commit -am "release: 0.1.0"
git tag v0.1.0
git push origin main v0.1.0
```

`release.yml` then:

- refuses the tag unless it matches `cargo xtask version` (a run of
  `cargo pkgid`-free truth: the workspace version), and refuses a runner that is
  not arm64, so a name can never disagree with its bytes;
- builds with `cargo xtask bundle --dmg --zip --check`, which also launches the
  bundle from inside the `.app` with a home of its own and waits for Rune's ready
  receipt;
- mounts the disk image, copies the app the way a drag does, launches it again
  (`scripts/release-smoke.sh`), and re-checks the checksums;
- uploads the artifacts, then creates a **draft** release with the `.dmg`, the
  `.zip`, `SHA256SUMS.txt` and `scripts/install.sh`, with the CHANGELOG section
  as its notes.

## Verify the draft

On a machine that has never had openNoteX:

```sh
# From the draft's assets, or from the install command in its notes.
curl -fsSLO https://raw.githubusercontent.com/OpenNoteX/open-notex/main/scripts/install.sh
sh install.sh 0.1.0
```

Check the list opens, a note opens, and `openNoteX → About`-level facts hold:
the version in `--version` matches the tag, and the app reads the vault you
point it at. Then publish the draft:

```sh
gh release edit v0.1.0 --draft=false
```

To withdraw one, `gh release delete v0.1.0 --cleanup-tag` (the tag goes too, so
the next attempt is not a forced re-tag) and cut a patch release.

## What an install actually does

`scripts/install.sh` downloads the zip, compares its SHA-256 with the release's
`SHA256SUMS.txt`, unpacks it, stages the bundle beside its destination, swaps it
in with two renames on one volume, clears the quarantine flag and opens it. If a
copy is running it asks the app to quit first, and a failure at the swap puts the
old bundle back rather than leaving a hole.

## No Apple Developer account (yet)

Every build is **ad-hoc signed** (`codesign -s -`), which is the minimum Apple
Silicon accepts. Consequences, stated plainly:

- A `.dmg` downloaded through a browser carries a quarantine flag, and macOS
  refuses to open an ad-hoc signed app from one. The old right-click → Open is
  gone on current macOS. Either run `scripts/install.sh`, which clears the flag
  for you, or go to System Settings → Privacy & Security → **Open Anyway**.
- Each release has a different ad-hoc identity, so macOS treats an update as a
  new app for anything tied to code signing (permission prompts). Nothing today
  depends on that; the first thing that will is notifications (M4).
- A release cannot be notarized. That needs a Developer ID certificate and
  `notarytool`, which needs the paid account (M6b).

`cargo xtask bundle --sign "Developer ID Application: …"` already signs with the
hardened runtime and a timestamp, and `docs/PLAN.md` tracks M6b as blocked rather
than dropped.

## Versions

`0.x` until 1.0. `CFBundleShortVersionString` is the workspace version;
`CFBundleVersion` is the commit count, which only has to increase. `--version`
prints the version and the commit, so a bug report identifies its binary:

```sh
$ /Applications/openNoteX.app/Contents/MacOS/open-notex --version
openNoteX 0.1.0 (28d4503)
```

## Building one locally

```sh
cargo xtask bundle --dmg --zip --check --out artifacts/dist
open artifacts/dist/openNoteX-0.1.0-arm64.dmg
```

`--check` opens a window for a moment; the artifacts land in `artifacts/dist`
(ignored by git), and `--sign` takes an identity when one exists.
