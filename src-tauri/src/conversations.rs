//! Claude Desktop's Code conversations as its sidebar lists them, for the
//! phone's 对话 menu: read from its records (`local_<id>.json`, see
//! `Paths::desktop_sessions`) and from Claude Code's transcripts
//! (`<projects>/<folder as a name>/<id>.jsonl`). Nothing here writes.
//!
//! Claude Desktop keeps a conversation it has shown running in the
//! background, with what was said so far in its memory, until it runs too
//! many. One it holds like that must not be continued from here: it would
//! carry on from what it remembers and leave the phone's turns behind. So
//! `Held` tells, from the engines it runs, which ones it may hold; those
//! continue in a copy.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::claude_config::Paths;

/// Conversations listed for the phone, at most.
const LIST_MAX: usize = 80;
/// How much of a transcript's end is read for its history, and of its
/// start for a title.
const TAIL_BYTES: u64 = 4 << 20;
const HEAD_BYTES: u64 = 64 << 10;
/// The history sent to the phone, at most: the relay takes 64 KB an event.
const HISTORY_BYTES: usize = 48 << 10;
const HISTORY_TEXT: usize = 6_000;
/// A record made this long before an engine without a conversation of its
/// own started may still be the one it runs.
const FRESH_SLACK_MS: i64 = 60_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation {
    /// Claude Code's session id: the transcript's name.
    pub id: String,
    pub title: String,
    pub folder: String,
    /// Last activity (ms since the epoch): Claude Desktop's, or the
    /// transcript's last change when later (the phone may have gone on).
    pub at: i64,
    /// What it runs with in Claude Desktop.
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub transcript: PathBuf,
}

/// Claude Code's conversations folder: `CLAUDE_CONFIG_DIR` when the user set
/// one (not in a dev build, which keeps to its sandbox), else ~/.claude.
pub fn projects_dir(paths: &Paths) -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|_| !cfg!(debug_assertions))
        .map(|d| PathBuf::from(d).join("projects"))
        .or_else(|| paths.cli_projects())
}

/// Every transcript by its session id, in one pass over the projects.
fn transcripts(paths: &Paths) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    let Some(projects) = projects_dir(paths) else {
        return found;
    };
    for dir in std::fs::read_dir(projects).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    if crate::remote::is_uuid(id) {
                        found.insert(id.to_string(), path);
                    }
                }
            }
        }
    }
    found
}

/// Claude Desktop's records, `<account>/<org>/local_<id>.json`: the
/// current account's (its device id in gateway mode) when it has some.
fn records(paths: &Paths) -> Vec<Value> {
    let Some(root) = paths.desktop_sessions() else {
        return Vec::new();
    };
    let mut accounts: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if let Some(device) = crate::claude_config::device_id(paths) {
        let mine = root.join(device);
        if mine.is_dir() {
            accounts = vec![mine];
        }
    }
    let mut out = Vec::new();
    for account in accounts {
        for org in std::fs::read_dir(account).into_iter().flatten().flatten() {
            for file in std::fs::read_dir(org.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = file.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with("local_") && name.ends_with(".json")) {
                    continue;
                }
                if let Some(v) = std::fs::read(file.path())
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                {
                    out.push(v);
                }
            }
        }
    }
    out
}

fn modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64)
}

/// A phone conversation not (yet) in Claude Desktop: its id, title, folder.
pub struct Own {
    pub id: String,
    pub title: String,
    pub folder: String,
}

/// Claude Desktop's conversations, the archived ones left out, and the
/// phone's own ones it doesn't have yet, newest first. Only those whose
/// transcript and folder are on this computer.
pub fn list(paths: &Paths, own: &[Own]) -> Vec<Conversation> {
    let files = transcripts(paths);
    // Ids Claude Desktop has, archived ones too: the phone's own copy of one
    // isn't listed again.
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in records(paths) {
        let Some(id) = r["cliSessionId"]
            .as_str()
            .filter(|s| crate::remote::is_uuid(s))
        else {
            continue;
        };
        if !seen.insert(id.to_string()) || r["isArchived"] == true {
            continue;
        }
        let (Some(file), Some(folder)) = (files.get(id), r["cwd"].as_str()) else {
            continue;
        };
        // Another machine's (SSH, WSL) folder isn't one here.
        if folder.is_empty() || !Path::new(folder).is_dir() {
            continue;
        }
        let text = |k: &str| r[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        let title = text("title")
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| head_title(file))
            .unwrap_or_else(|| "未命名对话".into());
        out.push(Conversation {
            id: id.to_string(),
            title: crate::remote::clip(&title, 80),
            folder: folder.to_string(),
            at: r["lastActivityAt"]
                .as_i64()
                .unwrap_or(0)
                .max(modified_ms(file)),
            mode: text("permissionMode"),
            model: text("model"),
            effort: text("effort"),
            transcript: file.clone(),
        });
    }
    for o in own {
        if seen.contains(&o.id) || !Path::new(&o.folder).is_dir() {
            continue;
        }
        let Some(file) = files.get(&o.id) else {
            continue;
        };
        seen.insert(o.id.clone());
        let title = Some(o.title.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| head_title(file))
            .unwrap_or_else(|| "未命名对话".into());
        out.push(Conversation {
            id: o.id.clone(),
            title: crate::remote::clip(&title, 80),
            folder: o.folder.clone(),
            at: modified_ms(file),
            transcript: file.clone(),
            ..Default::default()
        });
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.at));
    out.truncate(LIST_MAX);
    out
}

/// Reads up to `max` bytes of a file from `start`, as whole lines: the
/// first is dropped when it starts mid-way.
fn read_lines(path: &Path, start: u64, max: u64) -> Vec<Value> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(max).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    if start > 0 {
        lines.next();
    }
    lines
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// A title from the transcript's start: one it was given, else the first
/// thing the user said.
fn head_title(file: &Path) -> Option<String> {
    let lines = read_lines(file, 0, HEAD_BYTES);
    lines
        .iter()
        .find_map(|v| {
            (v["type"] == "custom-title")
                .then(|| v["customTitle"].as_str())
                .flatten()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            lines.iter().find_map(|v| {
                (v["type"] == "user" && v["isSidechain"] != true)
                    .then(|| prompt_text(v))
                    .flatten()
                    .map(|t| t.lines().next().unwrap_or("").trim().to_string())
                    .filter(|t| !t.is_empty())
            })
        })
}

/// What the user typed in a transcript line, if it is that: not Claude
/// Code's own notes (meta, compaction, slash commands, task notices).
fn prompt_text(v: &Value) -> Option<String> {
    if v["isMeta"] == true || v["isCompactSummary"] == true || v["promptSource"] == "system" {
        return None;
    }
    let content = &v["message"]["content"];
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            let mut parts: Vec<String> = Vec::new();
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = b["text"].as_str() {
                            parts.push(t.to_string());
                        }
                    }
                    Some("image") => parts.push("[图片]".into()),
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<')).then(|| text.to_string())
}

pub struct History {
    /// Events as the phone shows them live: user, text, tool, tool_done, note.
    pub items: Vec<Value>,
    /// Earlier parts were left out.
    pub more: bool,
}

/// The conversation so far, as the phone shows it: the branch that was
/// written last (a rewind leaves others), from the transcript's end.
pub fn history(file: &Path, folder: &str) -> History {
    let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    let start = size.saturating_sub(TAIL_BYTES);
    let lines = read_lines(file, start, TAIL_BYTES);
    let mut by_uuid: HashMap<&str, usize> = HashMap::new();
    let mut leaf = None;
    for (i, v) in lines.iter().enumerate() {
        let Some(uuid) = v["uuid"].as_str() else {
            continue;
        };
        if v["isSidechain"] == true {
            continue;
        }
        by_uuid.insert(uuid, i);
        if matches!(
            v["type"].as_str(),
            Some("user" | "assistant" | "system" | "attachment")
        ) {
            leaf = Some(i);
        }
    }
    // Back from the leaf, across compactions (they start a new chain that
    // names the old one), to the start of what was read.
    let mut chain = Vec::new();
    let mut cut = start > 0;
    let mut at = leaf;
    let mut steps = 0;
    while let Some(i) = at {
        steps += 1;
        if steps > lines.len() {
            break; // a loop in a damaged file
        }
        chain.push(i);
        let v = &lines[i];
        let parent = v["parentUuid"]
            .as_str()
            .or_else(|| v["logicalParentUuid"].as_str());
        at = match parent {
            Some(p) => match by_uuid.get(p) {
                Some(&j) => Some(j),
                None => {
                    cut = true;
                    None
                }
            },
            None => None,
        };
    }
    chain.reverse();

    let mut items = Vec::new();
    for i in chain {
        let v = &lines[i];
        match v["type"].as_str() {
            Some("user") => {
                if let Some(text) = prompt_text(v) {
                    items.push(
                        json!({ "kind": "user", "text": crate::remote::clip(&text, HISTORY_TEXT) }),
                    );
                }
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        items.push(json!({ "kind": "tool_done", "id": b["tool_use_id"], "ok": b["is_error"] != true }));
                    }
                }
            }
            Some("assistant") => {
                let uuid = v["uuid"].as_str().unwrap_or("");
                for (n, b) in v["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    match b["type"].as_str() {
                        Some("text") => {
                            let text = b["text"].as_str().unwrap_or("").trim();
                            if !text.is_empty() {
                                items.push(json!({ "kind": "text", "id": format!("h-{uuid}:{n}"), "text": crate::remote::clip(text, HISTORY_TEXT) }));
                            }
                        }
                        Some("tool_use") => items.push(json!({
                            "kind": "tool",
                            "id": b["id"],
                            "title": crate::remote::tool_title(b["name"].as_str().unwrap_or(""), &b["input"], folder),
                        })),
                        _ => {}
                    }
                }
            }
            Some("system") if v["subtype"] == "compact_boundary" => {
                items.push(json!({ "kind": "note", "text": "之前的对话已压缩" }));
            }
            _ => {}
        }
    }
    // The newest that fit.
    let mut total = 0;
    let mut keep = items.len();
    for (i, item) in items.iter().enumerate().rev() {
        let size = item.to_string().len() + 1;
        if total + size > HISTORY_BYTES {
            break;
        }
        total += size;
        keep = i;
    }
    if keep > 0 {
        cut = true;
    }
    History {
        items: items.split_off(keep),
        more: cut,
    }
}

/// One Claude Code engine Claude Desktop runs: when it started (ms since
/// the epoch) and the conversation it resumed, if its command line says.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub started: i64,
    pub resumed: Option<String>,
}

/// Which conversations Claude Desktop may hold open (see the module's notes).
#[derive(Debug, Default)]
pub struct Held {
    ids: HashSet<String>,
    /// Its engines couldn't be read: any may be held.
    unknown: bool,
}

impl Held {
    /// `records`: Claude Desktop's conversations, each with when it made its
    /// record (ms).
    pub fn of(engines: Option<&[Engine]>, records: &[(String, i64)]) -> Held {
        let Some(engines) = engines else {
            return Held {
                unknown: true,
                ..Default::default()
            };
        };
        let mut ids: HashSet<String> = engines.iter().filter_map(|e| e.resumed.clone()).collect();
        // An engine begun without a conversation runs one new in this run of
        // Claude Desktop, whose record it made as the engine started (within
        // a second, seen on 4.4212).
        for e in engines.iter().filter(|e| e.resumed.is_none()) {
            let found = records
                .iter()
                .filter(|(id, made)| *made >= e.started - FRESH_SLACK_MS && !ids.contains(id))
                .min_by_key(|(_, made)| (made - e.started).abs())
                .map(|(id, _)| id.clone());
            if let Some(id) = found {
                ids.insert(id);
            }
        }
        Held {
            ids,
            unknown: false,
        }
    }

    pub fn holds(&self, id: &str) -> bool {
        self.unknown || self.ids.contains(id)
    }
}

/// What Claude Desktop holds now; nothing when it isn't running.
pub fn held(paths: &Paths) -> Held {
    let engines = crate::desktop::engines();
    let made: Vec<(String, i64)> = records(paths)
        .iter()
        .filter_map(|r| {
            Some((
                r["cliSessionId"].as_str()?.to_string(),
                r["createdAt"].as_i64()?,
            ))
        })
        .collect();
    Held::of(engines.as_deref(), &made)
}

/// The conversation `--resume` names in an engine's command line.
pub fn resumed_in(command_line: &str) -> Option<String> {
    let mut words = command_line.split_whitespace().map(|w| w.trim_matches('"'));
    while let Some(w) = words.next() {
        let id = if let Some(v) = w.strip_prefix("--resume=") {
            v.to_string()
        } else if w == "--resume" {
            words.next().unwrap_or("").to_string()
        } else {
            continue;
        };
        return crate::remote::is_uuid(&id).then_some(id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";
    const C: &str = "33333333-3333-4333-8333-333333333333";
    const D: &str = "44444444-4444-4444-8444-444444444444";
    const E: &str = "55555555-5555-4555-8555-555555555555";

    #[test]
    fn the_list_is_claude_desktops_and_the_phones_own_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("local");
        let home = dir.path().join("home");
        let paths = Paths::under(&local, &home, dir.path().join("backups"));
        let work = dir.path().join("work");
        let gone = dir.path().join("gone");
        std::fs::create_dir_all(&work).unwrap();
        let projects = home.join(".claude").join("projects").join("C--work");
        let sessions = local
            .join("Claude-3p")
            .join("claude-code-sessions")
            .join("acct")
            .join("org");
        let record = |id: &str, title: &str, cwd: &Path, at: i64, archived: bool| {
            write(
                &sessions.join(format!("local_{id}.json")),
                &json!({
                    "sessionId": format!("local_{id}"), "cliSessionId": id, "cwd": cwd,
                    "title": title, "lastActivityAt": at, "createdAt": at - 10,
                    "isArchived": archived, "model": "claude-opus-5-5", "effort": "max",
                    "permissionMode": "acceptEdits",
                })
                .to_string(),
            );
        };
        for id in [A, B, C, D, E] {
            write(
                &projects.join(format!("{id}.jsonl")),
                &format!(
                    "{}\n",
                    json!({ "type": "user", "uuid": "u1", "parentUuid": null, "message": { "role": "user", "content": format!("第一句 {id}\n第二行") } })
                ),
            );
        }
        // Titled; untitled (its first prompt); archived; folder gone.
        record(A, "修好登录", &work, 2_000_000_000_000, false);
        record(B, "", &work, 2_000_000_000_001, false);
        record(C, "旧的", &work, 2_000_000_000_002, true);
        record(D, "别处", &gone, 2_000_000_000_003, false);
        let own = |id: &str, title: &str| Own {
            id: id.into(),
            title: title.into(),
            folder: work.to_string_lossy().into(),
        };
        // A and C are Claude Desktop's already (C archived there); E is new.
        let list = list(&paths, &[own(A, "手机上说的"), own(C, ""), own(E, "")]);
        let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, [B, A, E]);
        assert_eq!(list[0].title, format!("第一句 {B}"));
        assert_eq!(list[1].title, "修好登录", "Claude Desktop's record wins");
        assert_eq!(list[1].mode.as_deref(), Some("acceptEdits"));
        assert_eq!(list[2].title, format!("第一句 {E}"));
        assert_eq!(list[2].mode, None, "not Claude Desktop's");
    }

    #[test]
    fn history_follows_the_branch_written_last_and_reads_as_the_phone_shows_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        let lines = [
            json!({ "type": "queue-operation", "operation": "enqueue" }),
            json!({ "type": "user", "uuid": "u1", "parentUuid": null, "promptSource": "sdk", "message": { "role": "user", "content": "看看 README" } }),
            json!({ "type": "attachment", "uuid": "x1", "parentUuid": "u1" }),
            json!({ "type": "assistant", "uuid": "a1", "parentUuid": "x1", "message": { "id": "m1", "content": [{ "type": "thinking", "thinking": "…" }] } }),
            json!({ "type": "assistant", "uuid": "a2", "parentUuid": "a1", "message": { "id": "m1", "content": [{ "type": "tool_use", "id": "t1", "name": "Read", "input": { "file_path": "README.md" } }] } }),
            json!({ "type": "user", "uuid": "r1", "parentUuid": "a2", "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "is_error": false }] } }),
            json!({ "type": "assistant", "uuid": "a3", "parentUuid": "r1", "message": { "id": "m2", "content": [{ "type": "text", "text": "  写得很清楚。 " }] } }),
            // An abandoned branch, then a side task, then the compaction.
            json!({ "type": "user", "uuid": "old", "parentUuid": "a3", "message": { "content": "不要了" } }),
            json!({ "type": "user", "uuid": "s1", "parentUuid": "a3", "isSidechain": true, "message": { "content": "子任务" } }),
            json!({ "type": "system", "subtype": "compact_boundary", "uuid": "c1", "parentUuid": null, "logicalParentUuid": "a3" }),
            json!({ "type": "user", "uuid": "cs", "parentUuid": "c1", "isCompactSummary": true, "message": { "content": "This session is being continued…" } }),
            json!({ "type": "user", "uuid": "m", "parentUuid": "cs", "isMeta": true, "message": { "content": "<local-command-caveat>" } }),
            json!({ "type": "user", "uuid": "u2", "parentUuid": "m", "message": { "content": [{ "type": "text", "text": "再跑一下测试" }, { "type": "image" }] } }),
            json!({ "type": "user", "uuid": "n1", "parentUuid": "u2", "promptSource": "system", "message": { "content": "<task-notification>" } }),
            json!({ "type": "last-prompt", "leafUuid": "u2" }),
        ];
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        write(&file, &text);
        let h = history(&file, "");
        assert!(!h.more);
        let kinds: Vec<(&str, &str)> = h
            .items
            .iter()
            .map(|e| {
                (
                    e["kind"].as_str().unwrap(),
                    e["text"].as_str().or(e["title"].as_str()).unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                ("user", "看看 README"),
                ("tool", "读取 README.md"),
                ("tool_done", ""),
                ("text", "写得很清楚。"),
                ("note", "之前的对话已压缩"),
                ("user", "再跑一下测试\n[图片]"),
            ]
        );
        assert_eq!(h.items[2]["ok"], true);
        assert_eq!(head_title(&file).as_deref(), Some("看看 README"));
    }

    #[test]
    fn a_long_history_keeps_its_newest_part() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        let mut text = String::new();
        let mut parent = Value::Null;
        for i in 0..200 {
            let uuid = format!("u{i}");
            text.push_str(&format!(
                "{}\n",
                json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "message": { "content": format!("{i} {}", "长".repeat(400)) } })
            ));
            parent = json!(uuid);
        }
        write(&file, &text);
        let h = history(&file, "");
        assert!(h.more);
        let total: usize = h.items.iter().map(|e| e.to_string().len() + 1).sum();
        assert!(total <= HISTORY_BYTES);
        assert!(h.items.last().unwrap()["text"]
            .as_str()
            .unwrap()
            .starts_with("199 "));
    }

    #[test]
    fn claude_desktop_holds_what_its_engines_resumed_and_its_new_ones() {
        let engines = [
            Engine {
                started: 1_000_000,
                resumed: Some(A.into()),
            },
            // A new conversation's: its record made as it started.
            Engine {
                started: 5_000_000,
                resumed: None,
            },
        ];
        let made = [
            (A.to_string(), 900_000),
            (B.to_string(), 100),
            (C.to_string(), 5_000_036),
            (D.to_string(), 9_000_000),
        ];
        let held = Held::of(Some(&engines), &made);
        assert!(held.holds(A));
        assert!(held.holds(C), "made as that engine started");
        assert!(!held.holds(B), "made long before");
        assert!(!held.holds(D), "made long after: another conversation");
        assert!(!held.holds(E), "the phone's own, not in Claude Desktop");
        // Nothing runs: nothing held. Unreadable: all of them.
        assert!(!Held::of(Some(&[]), &made).holds(A));
        assert!(Held::of(None, &made).holds(E));
    }

    #[test]
    fn engines_name_the_conversation_they_resumed() {
        let line = format!("\"C:\\x\\claude.exe\" --output-format stream-json --resume={A} --model=claude-opus-5-5");
        assert_eq!(resumed_in(&line).as_deref(), Some(A));
        assert_eq!(
            resumed_in(&format!("claude --resume {B} -p")).as_deref(),
            Some(B)
        );
        assert_eq!(resumed_in("claude --output-format stream-json"), None);
        assert_eq!(resumed_in("claude --resume=../../etc"), None);
    }

    /// Reads this PC's real Claude Desktop conversations; run by hand. Only
    /// reads, and prints no conversation text.
    #[test]
    #[ignore]
    fn reads_this_pcs_conversations() {
        let paths = Paths::system(std::env::temp_dir()).unwrap();
        let started = std::time::Instant::now();
        let list = list(&paths, &[]);
        let held = held(&paths);
        println!("{} conversations in {:?}", list.len(), started.elapsed());
        for c in list.iter().take(8) {
            println!(
                "{} at={} open={} title={} chars folder={}",
                &c.id[..8],
                c.at,
                held.holds(&c.id),
                c.title.chars().count(),
                Path::new(&c.folder)
                    .file_name()
                    .map(|n| n.to_string_lossy().len())
                    .unwrap_or(0)
            );
        }
        if let Some(c) = list.first() {
            let started = std::time::Instant::now();
            let h = history(&c.transcript, &c.folder);
            let bytes: usize = h.items.iter().map(|e| e.to_string().len()).sum();
            println!(
                "history of the newest: {} items, {bytes} bytes, more={} in {:?}",
                h.items.len(),
                h.more,
                started.elapsed()
            );
        }
    }
}
