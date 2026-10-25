use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn register_thread(
    codex: &Path,
    codex_home: &Path,
    rollout_path: &Path,
    thread_id: &str,
    cwd: &Path,
    expected_name: &str,
    archived: bool,
) -> Result<bool> {
    let mut child = Command::new(codex)
        .args(["app-server", "--listen", "stdio://"])
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start Codex App Server using {}", codex.display()))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("missing stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("missing stdout"))?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let _ = sender.send(value);
            }
        }
    });

    let result = restore_protocol(
        &mut stdin,
        &receiver,
        rollout_path,
        thread_id,
        cwd,
        expected_name,
        archived,
    );
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn restore_protocol(
    stdin: &mut impl Write,
    receiver: &mpsc::Receiver<Value>,
    rollout_path: &Path,
    thread_id: &str,
    cwd: &Path,
    expected_name: &str,
    archived: bool,
) -> Result<bool> {
    send(
        stdin,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {
                    "name": "codex_migrate",
                    "title": "Codex Migrate",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {"experimentalApi": true}
            }
        }),
    )?;
    wait_for_result(receiver, 1, Duration::from_secs(10))?;
    send(stdin, json!({"method": "initialized", "params": {}}))?;
    send(
        stdin,
        json!({"method":"thread/read", "id":2,
            "params":{"threadId":thread_id,"includeTurns":false}}),
    )?;
    let before = wait_for_result(receiver, 2, Duration::from_secs(10))?;
    check_thread(&before, thread_id)?;
    if before
        .pointer("/thread/status/type")
        .and_then(Value::as_str)
        == Some("active")
    {
        return Err(anyhow!(
            "thread is active; retry restoration when it is idle"
        ));
    }
    // Archived sessions remain archived until the user explicitly reopens them.
    if archived {
        return Ok(false);
    }
    send(
        stdin,
        json!({
            "method": "thread/resume",
            "id": 3,
            "params": {
                "threadId": thread_id,
                "path": rollout_path.to_string_lossy(),
                "cwd": cwd.to_string_lossy(),
                "excludeTurns": true
            }
        }),
    )?;
    let resumed = wait_for_result(receiver, 3, Duration::from_secs(30))?;
    check_thread(&resumed, thread_id)?;
    let existing_name = before
        .pointer("/thread/name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty());
    let desired_name = existing_name.unwrap_or(expected_name);
    if existing_name.is_none() && !desired_name.trim().is_empty() {
        send(
            stdin,
            json!({"method":"thread/name/set", "id":4,
                "params":{"threadId":thread_id,"name":desired_name}}),
        )?;
        wait_for_result(receiver, 4, Duration::from_secs(10))?;
    }
    send(
        stdin,
        json!({"method":"thread/turns/list", "id":5,
            "params":{"threadId":thread_id,"limit":1,"itemsView":"notLoaded"}}),
    )?;
    let page = wait_for_result(receiver, 5, Duration::from_secs(30))?;
    let history = page
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("history response has no data array"))?;
    send(
        stdin,
        json!({
            "method": "thread/read",
            "id": 6,
            "params": {"threadId": thread_id, "includeTurns": false}
        }),
    )?;
    let after = wait_for_result(receiver, 6, Duration::from_secs(10))?;
    check_thread(&after, thread_id)?;
    if !desired_name.trim().is_empty()
        && after.pointer("/thread/name").and_then(Value::as_str) != Some(desired_name)
    {
        return Err(anyhow!("thread name readback does not match"));
    }
    let actual_cwd = after
        .pointer("/thread/cwd")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("thread readback has no working directory"))?;
    if Path::new(actual_cwd) != cwd {
        return Err(anyhow!("working directory readback does not match"));
    }
    Ok(!history.is_empty())
}

fn check_thread(value: &Value, thread_id: &str) -> Result<()> {
    if value.pointer("/thread/id").and_then(Value::as_str) != Some(thread_id) {
        return Err(anyhow!("Codex returned a different thread UUID"));
    }
    Ok(())
}

fn send(stdin: &mut impl Write, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *stdin, &value)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn wait_for_result(receiver: &mpsc::Receiver<Value>, id: i64, timeout: Duration) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!("App Server request {id} timed out"));
        }
        let value = receiver
            .recv_timeout(remaining)
            .map_err(|_| anyhow!("App Server request {id} timed out"))?;
        if value.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(anyhow!("App Server request {id} failed: {error}"));
        }
        return value
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("App Server request {id} returned no result"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(
        name: Option<&str>,
        final_name: &str,
        history: Value,
        archived: bool,
    ) -> (Result<bool>, Vec<Value>) {
        let (sender, receiver) = mpsc::channel();
        let thread = |name: Option<&str>| json!({"thread":{"id":"test-id","name":name,"cwd":"/mapped","status":{"type":"idle"}}});
        for (id, result) in [
            (1, json!({})),
            (2, thread(name)),
            (3, thread(name)),
            (4, json!({})),
            (5, json!({"data":history})),
            (6, thread(Some(final_name))),
        ] {
            sender.send(json!({"id":id,"result":result})).unwrap();
        }
        let mut bytes = Vec::new();
        let result = restore_protocol(
            &mut bytes,
            &receiver,
            Path::new("/rollout.jsonl"),
            "test-id",
            Path::new("/mapped"),
            "原标题",
            archived,
        );
        let requests = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (result, requests)
    }

    #[test]
    fn restores_title_and_verifies_history_without_starting_a_turn() {
        let (result, requests) = fixture(None, "原标题", json!([{"id":"old-turn"}]), false);
        assert!(result.unwrap());
        assert!(requests
            .iter()
            .any(|request| request["method"] == "thread/name/set"
                && request["params"]["name"] == "原标题"));
        assert!(!requests
            .iter()
            .any(|request| request["method"] == "turn/start"));
    }

    #[test]
    fn preserves_local_title_and_distinguishes_empty_history() {
        let (result, requests) = fixture(Some("本机标题"), "本机标题", json!([]), false);
        assert!(!result.unwrap());
        assert!(!requests
            .iter()
            .any(|request| request["method"] == "thread/name/set"));
    }

    #[test]
    fn mismatched_name_is_not_success() {
        assert!(fixture(None, "wrong", json!([{}]), false).0.is_err());
    }

    #[test]
    fn archives_are_never_resumed_or_unarchived() {
        let (result, requests) = fixture(None, "", json!([]), true);
        assert!(!result.unwrap());
        assert_eq!(requests.len(), 3);
        assert!(!requests
            .iter()
            .any(|request| request["method"] == "thread/resume"
                || request["method"] == "thread/unarchive"));
    }
}
