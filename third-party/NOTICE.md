# Mole CLI

- Upstream: https://github.com/tw93/Mole
- Revision: `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`
- Source snapshot: https://github.com/tw93/Mole/tree/ba3a0f57f78e4c22818fda701219cc3dd4d6953f
- Copyright: Mole authors and contributors; retain the notices in the upstream source.
- License: GPL-3.0; the unmodified license text is included in `Mole-LICENSE`.
- Trademark policy: https://github.com/tw93/Mole/blob/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/TRADEMARK.md
- Official product: https://mole.fit/zh/

`npm run engine:prepare` fetches the exact Git revision, checks its identity and tracked-file cleanliness, and exports that tree into an isolated build directory with `git archive`. It preserves the upstream `mole`, `mo`, `bin/`, `lib/`, license and trademark files, and compiles `cmd/analyze` and `cmd/status` with `go build -mod=readonly -buildvcs=false -trimpath`. The generated manifest records the actual Go toolchain, target architecture, resource sizes and SHA-256 digests; a verified staging directory is atomically published. The complete upstream source and Go dependency manifest remain available in `.cache/mole-source/`. No upstream source patches are applied. See `docs/engine-integrity.md` for offline preparation and signing boundaries.

MooMole's `scripts/mole-clean-preview.sh` is copied beside the unmodified upstream files. It sources the pinned `bin/clean.sh`, runs only the dry-run lifecycle and captures `emit_deduplicated_dry_run_ledger` before upstream exit cleanup. It redirects the human preview into a private task directory rather than overwriting the user's Mole preview. This wrapper is MooMole adaptation code, not an upstream feature or patch. Protocol details and limitations are documented in `docs/clean-preview-protocol.md`.

`scripts/mole-review-rules.sh` is a separate read-only adapter that loads the same pinned source and invokes its path-level protection, whitelist, active-cache and SQLite guards. It does not invoke cleanup or grant execution eligibility; results are request-bound and kept in memory for review only.

The generated engine is application-local and is not installed into `/usr/local/bin` or Homebrew. Runtime commands are limited to analysis, status snapshots, application listing and cleanup preview. Before distributing compiled packages publicly, provide the corresponding source and build information under the applicable licenses and review notices for transitive dependencies. Local development packages are not an assertion that distribution compliance or Apple notarization has been completed.
