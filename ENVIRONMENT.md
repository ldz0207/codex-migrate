# Environment and verification scope

The upstream Rust application is version 1.0.8. Its dependencies and resolved
versions remain defined by `Cargo.toml` and `Cargo.lock`; see the main README for
the Rust/GUI build requirements. GUI stroke widths use explicit f32 literals to
avoid newer compiler fallback errors. This fork's engine now supports compressed
rollouts through the direct `zstd` 0.13 dependency, related-session selection,
and native title/history verification through the installed Codex executable.
Resolved dependencies are pinned by `Cargo.lock`; unrelated versions are retained.

The optional Windows native-restoration adapter has no third-party PowerShell
or Python package dependencies. End users need Windows PowerShell 5.1+ and an
installed Codex executable with `thread/resume`, `thread/read`,
`thread/name/set`, and `thread/turns/list` app-server methods.

Local verification on 2026-10-25:

- Windows PowerShell: 5.1.26100.9711.
- Python test runner: 3.11.9, standard library only.
- Installed Codex used for isolated native restoration: 0.164.0-alpha.11.1.
- Sixteen subprocess protocol tests passed on Windows.
- A private isolated integration run verified native title/history restoration,
  explicit working-directory mapping, and reopening a compressed archived record.
  Real session fixtures and output are excluded from this repository.
- No Rust toolchain was available locally; the Rust application was
  not rebuilt in this run. Existing Rust CI and a new Windows protocol test job
  provide separate checks.
- Desktop visibility is a separate user acceptance step, never inferred from
  a successful RPC response alone. No new model turn was started by the helper.
