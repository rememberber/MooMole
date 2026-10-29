# MooMole

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja-JP.md)

An unofficial macOS graphical client for Mole CLI, with more approachable workflows built on its capabilities. Keep it simple: see the result, understand the impact, then choose the next action. Built with React + Vite + TypeScript + Tauri + SQLite.

> This independent, unofficial project is for learning and research only. Understand the effects of each operation and back up important data before use. You assume the risks and any resulting issues.
>
> Some features build on [Mole CLI](https://github.com/tw93/Mole). Thanks to its author and contributors. Please respect and support their work by choosing the [official Mole product](https://mole.fit/) for everyday use.

## Languages

The interface supports Simplified Chinese, English and Japanese. Switch at any time in **Settings → General → Interface language**, without restarting. First launch follows the system’s primary preferred language; other system languages fall back to English. Manual choices are saved locally across restarts, and you can return to **Follow system**. Paths, original logs and diagnostic evidence retain their original text. Detailed development documents are currently mainly in Chinese.

## Current status

This is a development version. **Release acceptance for the first publicly usable version is not complete.** Cleanup, uninstall and maintenance have real execution paths with positive tests in the actual app. Remaining permission/fault, distribution and licensing gates are tracked in [first-release execution acceptance](docs/first-release-execution.md). Repository builds are not validated production releases.

| Feature                        | Available now                                                                                                                                                                                         | Current limits                                                                                                                                                                                                                          |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Quick cleanup / Cleanup        | Summarizes caches, logs, uninstall remnants, large files and apps. Checked help/icon caches are selected by default; eligible user caches and ordinary logs can be moved to Trash after confirmation. | Only clearly sourced user-cache ordinary files/strict folders and ordinary logs. Remnants and personal large files cannot be processed yet. Unknown, changed or expired items are retained. See [quick cleanup](docs/quick-cleanup.md). |
| AI deep scan                   | Detects local agents and uses a built-in skill, Mole candidates, selected-folder metadata and model knowledge for item suggestions, then enters existing cleanup review.                              | Initially Codex CLI / Claude Code (Claude not yet tested locally). Model data use and quota require confirmation. Extended findings are for verification only. See [AI scan](docs/ai-deep-scan.md).                                     |
| Applications                   | Search apps, inspect running/associated clues, and confirm moving one supported ordinary app bundle to Trash.                                                                                         | Settings, caches and personal data outside the bundle remain. Running, protected, managed or uncertain apps are retained; no promise to remove all remnants.                                                                            |
| Maintenance                    | Browse 19 pinned Mole task descriptions; explicitly choose and execute two-step Quick Look thumbnail-cache and generator-list refresh.                                                                | Other tasks are read only. No original-cache backup or one-click undo; no repair or speed guarantee.                                                                                                                                    |
| Space analyzer                 | Choose folders, view maps/lists, drill down, navigate clickable paths, filter large files and reveal in Finder.                                                                                       | No deletion. Measured sizes are not reclaimable space; incomplete results are labelled.                                                                                                                                                 |
| System status                  | Snapshots, pausable continuous collection, CPU/memory/disk/network trends and available metrics.                                                                                                      | Missing or stale metrics are identified. Leaving stops continuous collection.                                                                                                                                                           |
| Project artifacts / Installers | Explicit scope selection, discovery, measurement, filtering and read-only checks.                                                                                                                     | No cleanup yet. No detected usage is not proof of eligibility.                                                                                                                                                                          |
| Activity history               | Item results, receipts, interrupted/unverified states, complete archiving, export and read-only opening. Verification preserves original facts.                                                       | No re-execution from history or automatic in-app recovery. Archives still consume disk space.                                                                                                                                           |

Start with [first use](docs/first-use.md). See [product direction](docs/product-direction.md) and [release roadmap](docs/release-roadmap.md) for the full capability map and remaining scope. The home page shows the actual bundled Mole CLI version and engine state. Browser previews use sample data only.

Moving to Trash does not immediately free space. Cancellation stops only work not yet started; existing outcomes remain. Unverified results are not retried automatically. To retrieve an item, verify the real object and original folder in Finder and avoid overwriting existing files. Client protection and Mole allowlists remain in force; unknown or restricted conditions are never relaxed to obtain success.

## Installation and updates

Distribution uses architecture-matched DMGs, free ad-hoc signing and GitHub Releases. No fully accepted production release is available yet. A paid Apple developer account is not required, and we do not claim Developer ID signing or Apple notarization.

Once released, choose `arm64` for Apple Silicon or `x64` for Intel. Check the chip/processor in [About This Mac](https://support.apple.com/guide/mac-help/mchlea7173f3/mac). Verify file size and SHA-512 on the release page, open the DMG, copy the app to Applications, then launch from that location.

On first open, macOS may warn that the developer or malicious-software status cannot be verified. Confirm the source and integrity, then decide using [Apple’s security guidance](https://support.apple.com/102445). The app never disables Gatekeeper or strips system security attributes. Updates follow **Settings → About → Check for updates → Download and verify size/SHA-512 → Open DMG → Quit normally and replace manually**. A running app is never silently replaced. Checking does not generate packages when no new release exists.

Candidate builds for both architectures and local installation/replacement have validation records. Gatekeeper after public download, cross-SemVer upgrades, native Intel use and full supported-environment acceptance remain pending. See [distribution](docs/distribution.md) and [candidate builds](docs/candidate-build.md).

## Local development

Build target: macOS 14+. Actual native interaction acceptance has mainly used an Apple Silicon development Mac; the target does not establish minimum-OS or Intel acceptance.

Requires Node.js 24+, npm, Rust, Go and Xcode Command Line Tools. Pinned versions are in [build validation](docs/build-validation.md). Scripts prefer toolchains in project `.cache/` without changing shell configuration. A fresh clone still needs tool setup.

```sh
npm ci
npm run engine:prepare
npm run desktop

# Browser samples only; no local data access
npm run dev

# Build a local optimized App, ZIP and verification report; no upload/publication
npm run build:local-app

# Also create, mount, verify and eject an architecture-matched ad-hoc DMG
npm run build:local-dmg
```

The engine is built from a pinned Git tree, checked for resources/architecture and published atomically. Failure retains the old engine. With caches present, append `-- --offline` to preparation/local-artifact commands. Running the app needs no development toolchain and never downloads or separately updates its bundled engine. See [engine preparation](docs/engine-integrity.md) and [local builds](docs/local-app-build.md).

## Checks

Edit the app version only in `package.json`, then run `npm run version:sync` to synchronize native and lock files. Common checks:

```sh
npm run version:check
npm run format:check
npm test
npm run build
npm run notices:prepare # Prepare licensing materials before native builds; Tauri wrapper does this automatically
npm run check:rust
npm run test:engine
npm run test:adapter
npx playwright install chromium
npm run test:e2e
```

Browser regression uses a separate port 1431 without reusing an existing server; override with `MOOMOLE_E2E_PORT=1432` if occupied. Development app uses 1420. Other version/artifact/source/licensing/candidate checks are in [build validation](docs/build-validation.md). Remote checks have run. After checks, the candidate pipeline builds optimized artifacts for both architectures; it does not publish Releases or update manifests automatically.

## Local data and recovery

Settings and records live in `~/Library/Application Support/io.github.rememberber.moomole/`, with database `moomole.sqlite`. Records may contain paths, check results, operation intent and Trash receipts, never file contents or administrator passwords. AI deep scans separately ask permission to send bounded file metadata to the chosen agent’s model service; other records are not uploaded automatically.

Task, scan and review history defaults to 30 days, with 7/90-day options. Execution evidence and unverified interruption evidence are not pruned with ordinary history. All three execution modules share admission and active-record limits. When full, new batches stop. In **Activity history → Archive…**, save complete records that have ended and received required review to free active capacity. Archives preserve original plans, results, receipts, events and verification marks, support export/read-only opening, and never restore execution permission. See [retention](docs/local-data-retention.md) and [execution archives](docs/execution-archive.md).

Current database schema: v7. Upgrades from v1–v6 back up automatically; older apps should not open the new database directly. Interrupted changes do not resume automatically. Verify actual objects before recording verification of uncertain results; original unknown outcomes are not rewritten. No one-click database rollback or universal undo. See [upgrade and recovery](docs/native-upgrade-recovery.md).

The frontend exposes no general shell, arbitrary SQL or direct filesystem interface. Changes use separate log/uninstall/Quick Look confirmation and per-run validation. Updates access fixed sources without sending folders, history or diagnostics; see [update privacy and protocol](docs/update-check.md). Read-only checks may write local caches/check records; they do not imply zero local writes.

## Sources and license

Bundled Mole CLI baseline: **1.65.0** / [`308bf9911179ddfa40e5df9fdc42095a18ba5cfc`](https://github.com/tw93/Mole/commit/308bf9911179ddfa40e5df9fdc42095a18ba5cfc). Upstream sources are unchanged. Client adapters and UI sources are in this repository. Builds/candidates provide matching client, Mole, distributed dependencies and npm source inputs bound to the same app. Complete corresponding-source and release-license review remain pending; see [source bundles](docs/source-bundle.md) and [dependency materials](docs/dependency-materials.md).

Licensed under [GPL-3.0-only](LICENSE). See [Mole-LICENSE](third-party/Mole-LICENSE) and [third-party notices](third-party/NOTICE.md) for upstream licensing, provenance and build details. Mole names and brand assets belong to the original project. This project is neither official nor endorsed; the public name still requires confirmation under upstream branding policy.
