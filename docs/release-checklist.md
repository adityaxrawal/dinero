# DMG Packaging & Direct Distribution Release Checklist

**Purpose (Document 30 TASK-DESK-007):** since distribution is direct
(outside the Mac App Store, per the `com.apple.security.cs.allow-jit`
entitlement the in-process Candle LLM runtime requires — Document 16
§9.1), the DMG itself is the entire first-run experience for a new user.
`scripts/build_dmg.sh` automates what's structurally verifiable (the `.app`
bundle and `/Applications` symlink are both present, at the positions
`bundle.macOS.dmg`'s schema defaults already place them — confirmed against
`@tauri-apps/cli`'s `config.schema.json`, no explicit override needed).
What it cannot automate is listed here.

## 1. `dmg_first_launch_clean_machine_verification`

**Status: Manual QA only, not automated — per Doc 30's own acceptance
criterion for this task.**

Before every tagged release:

- [ ] Download the notarized, stapled DMG from the GitHub Release asset
      (not a locally-built copy — this must be the artifact a real user
      would actually download).
- [ ] Open it on a clean macOS VM/account with **no prior Dinero developer
      trust** (no Xcode command-line tools override, no prior "Open
      Anyway" click for this Team ID).
- [ ] Confirm Gatekeeper's first-launch dialog reads as a normal, trusted
      notarized-developer prompt (`"Apple could not verify..."` should
      **not** appear — that indicates a stapling or notarization failure
      that `scripts/notarize.sh --verify-only`'s CI check should already
      have caught, so seeing it here means that CI check itself needs
      investigation, not just this one release).
- [ ] Confirm the app actually launches and reaches onboarding after the
      Gatekeeper prompt is accepted.

This is deliberately not automated: exercising real Gatekeeper
first-launch trust establishment requires a machine that has never seen
this Team ID's signature before, which a CI runner (that ran the exact
same signing job) cannot represent.

## 2. Custom DMG background image — blocked, not forgotten

**Status: Open, blocked on Document 14 §11.**

Document 14 (Design System & Brand Guidelines) §11 lists brand assets
(logo, wordmark) as still-open. `bundle.macOS.dmg.background` (Tauri's DMG
background-image config field) is deliberately left unset rather than
pointed at a fabricated placeholder — an invented "brand" image would be
worse than the current plain background, and referencing a non-existent
file path would break every DMG build outright. Once Document 14 §11
resolves, add the real asset path to `tauri.conf.json`'s
`bundle.macOS.dmg.background` field (JSON, so this can only be recorded
here, not as an inline comment in that file).

## 3. Versioning and GitHub Release asset naming

**Status: Verified — already correct, no changes needed.**

- Tauri's bundler names the DMG using `tauri.conf.json`'s own `version`
  field automatically — there is no separate version string to keep in
  sync by hand.
- `release.yml` (TASK-DESK-006) tags the GitHub Release with the pushed
  `v*` tag (`tagName: ${{ github.ref_name }}`), and the auto-updater's
  `latest.json` manifest (Document 16 §9.1, TASK-DESK-005) is generated
  from the same build, so both always describe the same version.

## 4. Direct-download landing page

**Status: Minimal static page built — see `public/download.html`.**

Serves first-time installs that arrive outside the auto-update flow
(someone finding Dinero for the first time, not an existing install
checking for updates). Deliberately minimal (no final branding, per the
same Document 14 §11 blocker as item 2) — a heading, a direct link to the
latest GitHub Release, and system requirements. Where this gets hosted
(the production domain, `dinero.app`, per Document 49 §7) is a deployment
decision outside this checklist's scope — the file itself is ready to
deploy as a static asset wherever that ends up being.
