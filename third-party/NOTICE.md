# Mole CLI

- Upstream: https://github.com/tw93/Mole
- Revision: `ba3a0f57f78e4c22818fda701219cc3dd4d6953f`
- Source snapshot: https://github.com/tw93/Mole/tree/ba3a0f57f78e4c22818fda701219cc3dd4d6953f
- Copyright: Mole authors and contributors; retain the notices in the upstream source.
- License: GPL-3.0; the unmodified license text is included in `Mole-LICENSE`.
- Trademark policy: https://github.com/tw93/Mole/blob/ba3a0f57f78e4c22818fda701219cc3dd4d6953f/TRADEMARK.md
- Official product: https://mole.fit/zh/

`npm run engine:prepare` fetches the exact Git revision, checks its identity and tracked-file cleanliness, preserves the upstream `mole`, `mo`, `bin/`, `lib/`, license and trademark files, and compiles `cmd/analyze` and `cmd/status` with `go build -trimpath`. The complete upstream source and Go dependency manifest are retained in `.cache/mole-source/`. No upstream source patches are applied.

MooMole's `scripts/mole-clean-preview.sh` is copied beside the unmodified upstream files. It sources the pinned `bin/clean.sh`, runs only the dry-run lifecycle and captures `emit_deduplicated_dry_run_ledger` before upstream exit cleanup. It redirects the human preview into a private task directory rather than overwriting the user's Mole preview. This wrapper is MooMole adaptation code, not an upstream feature or patch. Protocol details and limitations are documented in `docs/clean-preview-protocol.md`.

The generated engine is application-local and is not installed into `/usr/local/bin` or Homebrew. Runtime commands are limited to analysis, status snapshots, application listing and cleanup preview. Before distributing compiled packages publicly, provide the corresponding source and build information under the applicable licenses and review notices for transitive dependencies. Local development packages are not an assertion that distribution compliance or Apple notarization has been completed.
