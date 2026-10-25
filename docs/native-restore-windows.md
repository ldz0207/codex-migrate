# Native restoration on Windows

This fork adds an optional Windows PowerShell 5.1+ adapter for records that have
already been copied into the selected Codex home. It does not replace the Rust
migration engine, run an import, or copy account/configuration files.

```powershell
# Restore a selected existing record and a missing user-facing title.
.\scripts\restore-native.ps1 -ThreadId '<UUID>' -ExpectedName 'My conversation'

# Only when the user has selected a new, existing project directory:
.\scripts\restore-native.ps1 -ThreadId '<UUID>' -Cwd 'D:\projects\example'

# Inspect a separate, isolated target home using its own configuration:
.\scripts\restore-native.ps1 -ThreadId '<UUID>' -CodexHome 'D:\migration-test\.codex'
```

The adapter uses Codex app-server to resume the same UUID, restores a title only
when a local user-facing title is missing, reads a history page, and verifies
the UUID/name again. No `turn/start` or `thread/start` request is sent. A working
directory override is sent only when explicitly supplied; a missing directory
is rejected rather than created. An active thread is left alone.

Archived records stay archived by default. Their history can be checked, but
they are reported as `archived`, not as ready for the normal sidebar. Supply
`-AllowUnarchive` only when the user explicitly wants to reopen that record.
For archived records without a native title, `pendingName` retains the expected
name for that explicit reopening; the helper does not unarchive just to set it.
Some Codex versions return no turns until an archived thread is reopened. In
that case, `historyVerification` is `deferredUntilExplicitReopen`, rather than
claiming the history is empty or has been verified.

Results distinguish `ready`, `archived`, and `empty`; RPC/time-out or readback
errors cause a failure. `desktopDisplayVerified` stays false: protocol readback
cannot prove that a user has seen the conversation in the desktop UI.

## Compressed records and current limitations

Codex itself reads its `.jsonl.zst` records. This adapter delegates history
loading to the installed Codex version, so it does not decompress or rewrite
historical messages. The upstream Rust scanner still only selects `.jsonl`:
this change does **not** claim compressed-input support for that scanner.
Dependency/parent records must already have been copied by the importer.

The adapter intentionally does not replace the target SQLite database or patch
desktop preference files. The Rust engine's existing import/rollback behavior
is unchanged. Use the upstream GUI for its documented offline migration flow;
this optional helper covers native recognition after a separate bounded copy.

## Verification

Run the isolated protocol integration suite on Windows:

```powershell
python -m unittest discover -s tests -p test_native_restore.py -v
```

The fixture exercises missing/existing titles, history/empty history, archive
preservation and explicit unarchive, active threads, wrong IDs, readback
mismatches, working-directory validation, notifications, and time-outs. It does
not read real conversations or use a model/API key. Test dependencies are Python
3.9+ standard library and Windows PowerShell 5.1; end-user restoration only
requires PowerShell and a compatible installed Codex executable.

Protocol reference: [Codex app-server](https://learn.chatgpt.com/docs/app-server).

Module callers may explicitly supply `-DiagnosticPath` to save backend stderr to
a private local file during troubleshooting. No diagnostic log is written by
default; do not publish logs from a real Codex home.
