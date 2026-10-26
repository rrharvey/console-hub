---
name: release
description: Build, package and release Console Hub for macOS, Windows and Linux. Use this whenever someone wants to cut a release, publish a new version, bump the version number, make installers (.dmg, .exe, .msi, .deb, .rpm, AppImage), do a test build of the installers, check why the Release or CI workflow failed, or install their local build. Releases are built by GitHub Actions; this skill covers the version bump, the tag, watching the build and the draft release.
---

# Release Console Hub

Installers are built by `.github/workflows/release.yml` on GitHub's macOS, Windows and
Linux runners, since you can't build a Windows installer on a Mac or the other way round.
`ci.yml` runs the tests on all three for every push.

## Rules, and why

- **Ask before pushing a tag or publishing a release.** A tag starts the release build and
  a published release is visible to everyone with repo access. Neither is easy to take
  back cleanly.
- **Releases are drafts.** The workflow creates a draft. Publishing is a separate step
  for the person, after they've checked the assets.
- **The tag must match `package.json`'s version** (`v` + version). The workflow refuses
  a mismatch. The app reads its version from `package.json` (`tauri.conf.json` says
  `"version": "../package.json"`). Keep `src-tauri/Cargo.toml`'s version the same, so
  nothing reports a different number.
- **Look for account data before a release.** Anyone the release reaches can browse the
  repo. Search the tree and recent commits for real IDs, emails and balances (see the
  write-provider skill).

## Test build (nothing published)

```
gh workflow run release.yml --ref main
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run download <run id> --dir ./release-artifacts
```

Run this the first time after changing the workflow, the bundle settings or the
dependencies. It builds every installer and attaches them to the run, so a teammate on
each OS can try one. macOS runner minutes cost about 10× Linux on private repos, and
Windows about 2×, so don't loop on it.

## Cut a release

1. **Main is green**: `gh run list --workflow ci.yml --branch main --limit 1`.
2. **Pick the version** (semver): patch for fixes, minor for features, major for changes
   that break providers (a new `apiVersion`).
3. **Bump it** in `package.json` and `src-tauri/Cargo.toml`. Then run
   `cargo update -p console-hub --manifest-path src-tauri/Cargo.toml`, which updates
   `Cargo.lock`, and `npm install`, which updates `package-lock.json`.
4. **Commit**: `Release vX.Y.Z`, listing notable changes in the body.
5. **Ask**, then tag and push:
   `git tag vX.Y.Z && git push origin main vX.Y.Z`.
6. **Watch**: `gh run watch` on the Release run. On failure:
   `gh run view <id> --log-failed`.
7. **Check the draft**: `gh release view vX.Y.Z` should list a `.dmg` for aarch64 and for
   x64, a Windows `-setup.exe` and `.msi`, and a Linux `.deb`, `.rpm` and `.AppImage`.
   Write the release notes.
8. **Hand over**: tell the person the draft is ready. Publish only if they ask:
   `gh release edit vX.Y.Z --draft=false`.

If the build failed after the tag was pushed, fix it on main, then move the tag only with
the person's go-ahead (`git tag -f vX.Y.Z && git push -f origin vX.Y.Z`). Nobody has the
release yet while it's a draft, but force-pushing a tag still rewrites shared state.

## Local builds

`npm run build` builds for the machine you're on, into
`src-tauri/target/release/bundle/`. On macOS it also installs the app into
`/Applications` (`scripts/postbuild.mjs`). Elsewhere it lists the installers. To build a
macOS installer for the other architecture, run
`rustup target add x86_64-apple-darwin` (or `aarch64-apple-darwin`), then
`npx tauri build --target <target>`.

## Unsigned builds

The installers aren't code-signed, so macOS (Gatekeeper) and Windows (SmartScreen) warn on
first launch. INSTALL.md tells people how to get past that. Signing needs an Apple
Developer ID and a Windows code-signing certificate, which tauri-action can use through
repository secrets. It isn't set up.
