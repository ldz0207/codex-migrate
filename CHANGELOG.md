# Changelog

All notable changes to this project will be documented here.

The project uses semantic versioning where practical.

## [Unreleased]


- Make GUI stroke widths explicit f32 literals for newer Rust compilers.

- Add an optional PowerShell 5.1+ native-restoration adapter for already-copied
  Windows sessions, with UTF-8 RPC, missing-title restoration, history-page
  verification, archive preservation, explicit working-directory changes, and
  isolated protocol tests.

- Export backups as complete `.codex` directory copies instead of minimal
  session-only folders, preserving databases, settings, Skills, logs, caches,
  and other local contents while excluding root-level login credential files.
- Name exported folders `Codex_backup` so they remain visible on macOS and Linux.

## [1.0.7] - 2026-06-21

- Use a zero-pixel optical text offset on Windows while retaining the existing macOS/Linux alignment.
- Embed the project icon and version metadata into Windows executables.
- Hide the console window when launching the Windows release GUI.

## [1.0.6] - 2026-06-21

- Added multi-select deletion for rollback snapshots.
- Added a default-selected tri-state “Select all” control for rollback records.
- Embedded user images and tool screenshots directly in exported single-file HTML.
- Added completion dialogs for import, backup export, HTML export, and path repair.
- Added Chinese and English interfaces with system-language detection.
- Added path repair, parent-directory mapping, and HTML conversation export.
- Added selective project/session import, conflict preview, transactional rollback, and Codex Desktop project registration.
## Fork: selective desktop restoration

- Require explicit session selection in the GUI and preview related records.
- Read compressed rollouts and completed NAS snapshot pointers.
- Restore missing native names, verify historical pages and report incomplete
  restoration separately from file/index import.
- Preserve destination names/archive state and record per-session verification
  in the rollback transaction's `import-result.json`.

