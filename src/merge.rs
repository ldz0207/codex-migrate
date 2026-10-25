use crate::discovery::Environment;
use crate::model::{
    ImportOptions, ImportPlan, MergeAction, PlannedThread, ScannedThread, SourceCatalog,
};
use crate::path_mapper::{history_only_path, map_explicit, normalize};
use crate::rollout;
use crate::scanner;
use anyhow::{anyhow, Result};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub fn build_plan(
    catalog: &SourceCatalog,
    source_threads: &[ScannedThread],
    environment: &Environment,
    options: &ImportOptions,
) -> Result<ImportPlan> {
    let existing =
        scanner::scan_codex_home(&environment.codex_home, environment.state_db.as_deref())?;
    let existing = existing
        .into_iter()
        .map(|thread| (thread.record.id.clone(), thread))
        .collect::<HashMap<_, _>>();
    let source = source_threads
        .iter()
        .map(|thread| (thread.record.id.clone(), thread))
        .collect::<HashMap<_, _>>();
    let mut threads = Vec::new();

    let selected = expand_selection(source_threads, &options.selected_thread_ids)?;
    for thread_id in &selected {
        let source_thread = source
            .get(thread_id)
            .ok_or_else(|| anyhow!("selected thread does not exist in source: {thread_id}"))?;
        let original_cwd = normalize(&source_thread.record.cwd);
        let history_only = options.history_only_projects.contains(&original_cwd);
        let mapped_cwd = if history_only {
            history_only_path(
                &environment.codex_home.join("migration_history"),
                &original_cwd,
            )
        } else {
            map_explicit(&original_cwd, &options.mappings, &environment.platform)
                .or_else(|| {
                    Path::new(&original_cwd)
                        .is_dir()
                        .then(|| PathBuf::from(&original_cwd))
                })
                .ok_or_else(|| anyhow!("selected project is not mapped: {original_cwd}"))?
        };
        let (action, mut reason) = match existing.get(thread_id) {
            None => (MergeAction::Import, "thread UUID is not present".to_owned()),
            Some(target) if target.record.sha256 == source_thread.record.sha256 => (
                MergeAction::SkipIdentical,
                "source and target hashes match".to_owned(),
            ),
            Some(target) => compare_contents(&source_thread.content, &target.content),
        };
        let target_path = match (&action, existing.get(thread_id)) {
            (MergeAction::SkipIdentical | MergeAction::KeepTargetLonger, Some(existing_thread)) => {
                existing_thread.source_path.clone()
            }
            _ => environment
                .codex_home
                .join(&source_thread.record.archive_path),
        };
        if !options.selected_thread_ids.contains(thread_id) {
            reason = format!("Required related session; {reason}");
        }
        let mut record = source_thread.record.clone();
        if let Some(target) = existing.get(thread_id) {
            // Preserve choices already made on the destination device.
            record.archived = target.record.archived;
            if !target.record.title.trim().is_empty() {
                record.title = target.record.title.clone();
            }
        }
        threads.push(PlannedThread {
            thread: record,
            source_path: source_thread.source_path.to_string_lossy().into_owned(),
            mapped_cwd: mapped_cwd.to_string_lossy().into_owned(),
            history_only,
            target_path: target_path.to_string_lossy().into_owned(),
            action,
            reason,
        });
    }
    threads.sort_by(|left, right| {
        left.thread
            .cwd
            .cmp(&right.thread.cwd)
            .then_with(|| right.thread.updated_at.cmp(&left.thread.updated_at))
    });
    let conflicts = threads
        .iter()
        .filter(|thread| thread.action == MergeAction::Conflict)
        .count();
    Ok(ImportPlan {
        source_codex_home: catalog.source_codex_home.clone(),
        codex_home: environment.codex_home.to_string_lossy().into_owned(),
        threads,
        conflicts,
    })
}

fn related_ids(thread: &ScannedThread) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in thread.content.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if value["type"] != "session_meta" {
            continue;
        }
        let payload = &value["payload"];
        for key in [
            "id",
            "forked_from_id",
            "forked_from_thread_id",
            "parent_thread_id",
        ] {
            if let Some(id) = payload[key].as_str() {
                if id != thread.record.id {
                    ids.insert(id.to_owned());
                }
            }
        }
        for key in ["source", "thread_source"] {
            let source = payload[key]
                .as_str()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                .unwrap_or_else(|| payload[key].clone());
            if let Some(id) = source
                .pointer("/subagent/thread_spawn/parent_thread_id")
                .and_then(serde_json::Value::as_str)
            {
                ids.insert(id.to_owned());
            }
        }
        break;
    }
    ids
}

fn expand_selection(
    threads: &[ScannedThread],
    selected: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let known = threads
        .iter()
        .map(|thread| thread.record.id.clone())
        .collect::<BTreeSet<_>>();
    let mut graph: HashMap<String, BTreeSet<String>> = HashMap::new();
    for thread in threads {
        for related in related_ids(thread) {
            graph
                .entry(thread.record.id.clone())
                .or_default()
                .insert(related.clone());
            graph
                .entry(related)
                .or_default()
                .insert(thread.record.id.clone());
        }
    }
    let mut result = selected.clone();
    let mut pending = selected.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !known.contains(&id) {
            return Err(anyhow!(
                "required related session is missing from source: {id}"
            ));
        }
        for related in graph.get(&id).into_iter().flatten() {
            if result.insert(related.clone()) {
                pending.push(related.clone());
            }
        }
    }
    Ok(result)
}

fn compare_contents(source: &[u8], target: &[u8]) -> (MergeAction, String) {
    if let (Some(source), Some(target)) = (
        rollout::canonicalize_cwd(source),
        rollout::canonicalize_cwd(target),
    ) {
        if source == target {
            return (
                MergeAction::SkipIdentical,
                "source and target differ only by mapped cwd metadata".to_owned(),
            );
        }
        if source.starts_with(&target) {
            return (
                MergeAction::ReplaceWithLonger,
                "target rollout is a normalized prefix of source".to_owned(),
            );
        }
        if target.starts_with(&source) {
            return (
                MergeAction::KeepTargetLonger,
                "source rollout is a normalized prefix of target".to_owned(),
            );
        }
    }
    if source.starts_with(target) {
        (
            MergeAction::ReplaceWithLonger,
            "target rollout is a byte-for-byte prefix of source".to_owned(),
        )
    } else if target.starts_with(source) {
        (
            MergeAction::KeepTargetLonger,
            "source rollout is a byte-for-byte prefix of target".to_owned(),
        )
    } else {
        (
            MergeAction::Conflict,
            "same UUID has divergent rollout content".to_owned(),
        )
    }
}

pub fn selected_source_bytes<'a>(
    source_threads: &'a [ScannedThread],
    source_path: &Path,
) -> Option<&'a [u8]> {
    source_threads
        .iter()
        .find(|thread| thread.source_path == source_path)
        .map(|thread| thread.content.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_prefix_and_conflict() {
        assert_eq!(
            compare_contents(b"abc", b"ab").0,
            MergeAction::ReplaceWithLonger
        );
        assert_eq!(
            compare_contents(b"ab", b"abc").0,
            MergeAction::KeepTargetLonger
        );
        assert_eq!(compare_contents(b"abc", b"abd").0, MergeAction::Conflict);
        assert_eq!(
            compare_contents(
                br#"{"type":"session_meta","payload":{"cwd":"/old"}}"#,
                br#"{"type":"session_meta","payload":{"cwd":"/new"}}"#
            )
            .0,
            MergeAction::SkipIdentical
        );
    }
}
