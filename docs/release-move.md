# Move releases from bleep-releases to the main repo

**Status:** planned, starts with release 0.5.1.

## Why

- **The old reason is gone.** `AviroopPaul/bleep-releases` exists because the source repo was private, and nobody can download a private repo's release files without a login (the updater, `install.sh`, Homebrew). `AviroopPaul/bleep` has been public since 18 Nov 2026.
- **Fewer places per release.** A release now goes to two repos, the tap and the site. After the move it goes to the main repo, the tap and the site.
- **SEO/GEO.** Downloads, stars and releases count on one repo. `docs/seo-audit.md` notes that `bleep-releases` says "Source is private", which hurts listings.

## The constraint

Every installed prod app (0.5.0 and older) has this feed compiled in:
`https://github.com/AviroopPaul/bleep-releases/releases/latest/download/latest.json`.
If `bleep-releases` stops getting releases, those apps never update again. So 0.5.1 is a **bridge release**: it goes to both repos, and it reads the new feed.

## Steps

### 1. Code changes (one PR, before the 0.5.1 release PR)

- **Prod feed:** `app/src-tauri/tauri.conf.json` `plugins.updater.endpoints` changes to `https://github.com/AviroopPaul/bleep/releases/latest/download/latest.json`.
- **Nightly feed:** `app/src-tauri/tauri.nightly.conf.json` changes to `https://github.com/AviroopPaul/bleep/releases/download/nightly/latest-nightly.json`.
- **Build script:** `app/scripts/build-release.sh` writes tarball URLs on the main repo (`.../bleep/releases/download/v$VER/...` and `.../bleep/releases/download/nightly/...`).
- **Nightly workflow:** `.github/workflows/nightly.yml` publishes all four files (dmg, sha256, tarball, `latest-nightly.json`) to the `nightly` prerelease in this repo only. Remove the `bleep-releases` step and the `RELEASES_TOKEN` secret.
- **Code comment:** `app/src-tauri/src/updater.rs` line 1 names the new feed.

### 2. Site and docs (same PR)

- **Install script:** `landing/install.sh` `BASE` changes to `https://github.com/AviroopPaul/bleep/releases/latest/download`.
- **Landing page:** `landing/index.html` JSON-LD `downloadUrl` and the two "Releases" links.
- **Generated pages:** `scripts/seo/pages.py` footer link, then rerun it. This updates changelog, privacy, alternatives, Claude Code and the three `vs/` pages.
- **Privacy page:** no change. Its update-check sentence says "GitHub", not a repo.
- **Other files:** `landing/llms.txt`, `README.md` (Download link), `.github/ISSUE_TEMPLATE/config.yml`, `CLAUDE.md` (Nightly line).

### 3. The 0.5.1 release (bridge)

- **Main repo:** release `v0.5.1` with `bleep-mac.dmg`, `.sha256`, `bleep-mac.app.tar.gz` and `latest.json`. The feed on the main repo is now live.
- **bleep-releases, one last time:** release `v0.5.1` with the same four files. Old apps find it, update to 0.5.1 and read the main repo from then on.
- **Homebrew tap:** `Casks/bleep.rb` `url` changes to `https://github.com/AviroopPaul/bleep/releases/download/v#{version}/bleep-mac.dmg`, plus the new `version` and `sha256`.
- **Site:** version bump as usual (`pages.py`, JSON-LD), then deploy.

### 4. Check after the release

- `curl -sI` both `latest.json` URLs: each returns 302, then 200.
- An installed 0.5.0 app finds 0.5.1 in Settings > Updates and installs it.
- After the update, its next check goes to the main repo (the app log shows the URL).
- `brew upgrade --cask bleep` installs 0.5.1 from the main repo.
- `curl -fsSL <site>/install.sh | sh` installs 0.5.1.

### 5. After the bridge

- **Stop publishing to bleep-releases** from 0.5.2 on.
- **Keep it for people who skip updates.** Someone on 0.5.0 who updates late still finds 0.5.1 there. Keep its last release for about three months.
- **Then archive it.** Set the description to "Moved to AviroopPaul/bleep" first.
- **Delete the `RELEASES_TOKEN` secret** and revoke the token on github.com.

## Risks

- **A release that misses bleep-releases strands old apps.** Only 0.5.1 must go there. Check it in step 4 before stopping.
- **A prerelease must never be "latest".** The `nightly` release in the main repo must stay a prerelease, or prod apps would read `latest.json` from it. It has no `latest.json` (only `latest-nightly.json`), so an update check would fail and not install nightly. Still, keep it a prerelease.
- **Branch rules:** all of this lands through PRs. Merge your own PRs with `--admin` after CI passes.
