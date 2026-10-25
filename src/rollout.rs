use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub fn is_rollout(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if path.extension().and_then(|value| value.to_str()) == Some("zst") {
        zstd::stream::decode_all(bytes.as_slice())
            .with_context(|| format!("decompress {}", path.display()))
    } else {
        Ok(bytes)
    }
}

pub fn encode_for_path(path: &Path, content: &[u8]) -> Result<Vec<u8>> {
    if path.extension().and_then(|value| value.to_str()) == Some("zst") {
        Ok(zstd::stream::encode_all(content, 0)?)
    } else {
        Ok(content.to_vec())
    }
}

pub fn rewrite_cwd_bytes(content: &[u8], cwd: &Path) -> Result<(Vec<u8>, usize)> {
    let cwd = cwd.to_string_lossy();
    let mut output = Vec::with_capacity(content.len());
    let mut changed = 0;
    for (line_number, line) in content.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut value: Value = serde_json::from_slice(line)
            .with_context(|| format!("parse rollout line {}", line_number + 1))?;
        if matches!(
            value.get("type").and_then(Value::as_str),
            Some("session_meta" | "turn_context")
        ) {
            if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
                if payload.get("cwd").and_then(Value::as_str) != Some(cwd.as_ref()) {
                    payload.insert("cwd".to_owned(), Value::String(cwd.to_string()));
                    changed += 1;
                }
            }
        }
        output.extend_from_slice(&serde_json::to_vec(&value)?);
        output.push(b'\n');
    }
    Ok((output, changed))
}

pub fn rewrite_cwd_file(path: &Path, cwd: &Path) -> Result<usize> {
    let original = read(path)?;
    let (rewritten, changed) = rewrite_cwd_bytes(&original, cwd)?;
    if changed > 0 {
        let temporary = path.with_extension("jsonl.tmp");
        fs::write(&temporary, encode_for_path(path, &rewritten)?)?;
        fs::rename(&temporary, path)?;
    }
    Ok(changed)
}

pub fn validate_cwd_file(path: &Path, cwd: &Path) -> Result<()> {
    let content = read(path)?;
    let expected = cwd.to_string_lossy();
    let mut session_meta = 0;
    for (line_number, line) in content.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_slice(line)
            .with_context(|| format!("parse rollout line {}", line_number + 1))?;
        if !matches!(
            value.get("type").and_then(Value::as_str),
            Some("session_meta" | "turn_context")
        ) {
            continue;
        }
        let Some(actual) = value
            .get("payload")
            .and_then(|payload| payload.get("cwd"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if actual != expected {
            anyhow::bail!(
                "{} contains cwd {actual}, expected {}",
                path.display(),
                cwd.display()
            );
        }
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            session_meta += 1;
        }
    }
    if session_meta == 0 {
        anyhow::bail!("{} has no mapped session_meta cwd", path.display());
    }
    Ok(())
}

pub fn canonicalize_cwd(content: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(content.len());
    for line in content.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut value: Value = serde_json::from_slice(line).ok()?;
        if matches!(
            value.get("type").and_then(Value::as_str),
            Some("session_meta" | "turn_context")
        ) {
            if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
                if payload.contains_key("cwd") {
                    payload.insert("cwd".to_owned(), Value::String("<mapped-cwd>".to_owned()));
                }
            }
        }
        output.extend_from_slice(&serde_json::to_vec(&value).ok()?);
        output.push(b'\n');
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_structured_cwd_fields() {
        let input = br#"{"type":"session_meta","payload":{"cwd":"/old","note":"/old"}}
{"type":"event_msg","payload":{"message":"/old"}}
"#;
        let (output, changed) = rewrite_cwd_bytes(input, Path::new("/new")).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(changed, 1);
        assert!(text.contains("\"cwd\":\"/new\""));
        assert!(text.contains("\"note\":\"/old\""));
        assert!(text.contains("\"message\":\"/old\""));
    }

    #[test]
    fn canonical_form_ignores_mapped_cwd() {
        let old = br#"{"type":"session_meta","payload":{"id":"1","cwd":"/old"}}"#;
        let new = br#"{"type":"session_meta","payload":{"id":"1","cwd":"C:\\new"}}"#;
        assert_eq!(canonicalize_cwd(old), canonicalize_cwd(new));
    }
}
