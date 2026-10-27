# Environment and verification scope

The upstream Rust application is version 1.0.8. Its dependencies and resolved
versions remain defined by `Cargo.toml` and `Cargo.lock`; see the main README for
the Rust/GUI build requirements. GUI stroke widths use explicit f32 literals to
avoid newer compiler fallback errors. This fork's engine now supports compressed
rollouts through the direct `zstd` 0.13.3 dependency, related-session selection,
and native title/history verification through the installed Codex executable.
Resolved dependencies are pinned by `Cargo.lock`; unrelated versions are retained.

Selection fix on 2026-10-27: parent dependencies are traversed in one direction.
Choosing a main chat no longer expands to all of its forks or background reviews.
The old packaged CLI reproduced two selected chats expanding to 103 records in a
synthetic fixture. Regression coverage checks that only the two chosen records are
planned, that required parents are still included without siblings, and that a
missing parent remains an error. The GUI hides auxiliary records by default and
counts explicit choices separately from dependencies. These changes require a new
Windows build; prior delivered binaries retain the old behavior.

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
- The desktop/CLI application is built in GitHub Actions. The verified compiler
  is Rust 1.99.1 (58cf8f9c3, 2026-10-13); Windows packages target
  x86_64-pc-windows-msvc. Format, Clippy and tests run on Windows, Linux and macOS.
  Packaged executables are used for local isolated checks. End users do not need
  a Rust toolchain, Python, or PowerShell for the desktop application's own flow.
- Desktop visibility is a separate user acceptance step, never inferred from
  a successful RPC response alone. No new model turn was started by the helper.
- The compiled Windows CLI passed a fresh isolated import of one ordinary chat
  and four related records, original-name/history readback, repeat import with
  no new copies, and compressed archive preservation. Input hashes were unchanged.
  The GUI's live click/visual acceptance remains pending.
