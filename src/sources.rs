//! Чтение следов контуров. Прибор только читает: ни одной записи в чужие системы.
//!
//! Каждый источник возвращает [`InputState`] — и если файла нет, это видно в отчёте,
//! а не превращается в ноль. Молчаливый ноль — главный способ соврать в мониторинге.

use crate::{now, Facts, InputState, PactRow};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn input(name: &str, source: &str, loaded: bool, note: impl Into<String>) -> InputState {
    InputState {
        name: name.to_string(),
        source: source.to_string(),
        loaded,
        note: note.into(),
    }
}

fn unreadable(path: &Path, facts: &mut Facts, name: &str) -> InputState {
    let state = input(
        name,
        &path.display().to_string(),
        false,
        "файл не прочитан: нет или не JSON",
    );
    facts.inputs.push(state.clone());
    state
}

/// Канбан: карточки по статусам. Форма — список или {"tasks": [...]}.
pub fn load_kanban(path: &Path, facts: &mut Facts) -> InputState {
    let Ok(raw) = fs::read_to_string(path) else {
        return unreadable(path, facts, "kanban");
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return unreadable(path, facts, "kanban");
    };
    let rows = value
        .get("tasks")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(rows) = rows else {
        return unreadable(path, facts, "kanban");
    };
    let mut assignees: Vec<String> = Vec::new();
    for row in rows {
        match row.get("status").and_then(Value::as_str).unwrap_or("") {
            "running" => {
                facts.running += 1;
                if let Some(name) = row.get("assignee").and_then(Value::as_str) {
                    if !name.is_empty() && !assignees.iter().any(|seen| seen == name) {
                        assignees.push(name.to_string());
                    }
                }
            }
            "ready" => facts.ready += 1,
            "blocked" => facts.blocked += 1,
            "review" => facts.review += 1,
            _ => {}
        }
    }
    facts.assignees_working = assignees.len();
    let state = input(
        "kanban",
        &path.display().to_string(),
        true,
        format!(
            "{} карточек: running {}, ready {}, blocked {}, review {}",
            rows.len(),
            facts.running,
            facts.ready,
            facts.blocked,
            facts.review
        ),
    );
    facts.inputs.push(state.clone());
    state
}

/// Диагностика канбана: залипшие карточки и повторные падения.
pub fn load_diagnostics(path: &Path, facts: &mut Facts) -> InputState {
    let Ok(raw) = fs::read_to_string(path) else {
        return unreadable(path, facts, "diagnostics");
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return unreadable(path, facts, "diagnostics");
    };
    let rows = value
        .get("diagnostics")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(rows) = rows else {
        return unreadable(path, facts, "diagnostics");
    };
    for row in rows {
        let Some(items) = row.get("diagnostics").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
            let count = item.get("count").and_then(Value::as_u64).unwrap_or(1) as usize;
            match kind {
                "stranded_in_ready" => facts.stranded += count,
                "repeated_crashes" => {
                    let crashes = item
                        .get("data")
                        .and_then(|data| data.get("consecutive_crashes"))
                        .and_then(Value::as_u64)
                        .unwrap_or(count as u64) as usize;
                    facts.crashed_runs = facts.crashed_runs.max(crashes);
                }
                _ => {}
            }
        }
    }
    let state = input(
        "diagnostics",
        &path.display().to_string(),
        true,
        format!(
            "{} карточек с замечаниями; залипших {}, падений подряд {}",
            rows.len(),
            facts.stranded,
            facts.crashed_runs
        ),
    );
    facts.inputs.push(state.clone());
    state
}

fn lines_since(path: &Path, cutoff: u64) -> Option<usize> {
    let raw = fs::read_to_string(path).ok()?;
    Some(
        raw.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|row| row.get("at").and_then(Value::as_u64).unwrap_or(0) >= cutoff)
            .count(),
    )
}

/// Командный слой tupleboard: закрытия с вердиктами, эскалации, алгедоника, политика.
pub fn load_team(dir: &Path, window: u64, facts: &mut Facts) -> InputState {
    let cutoff = now().saturating_sub(window);
    let closed_path = dir.join("closed.jsonl");
    let Ok(raw) = fs::read_to_string(&closed_path) else {
        return unreadable(&closed_path, facts, "team");
    };
    for line in raw.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if row.get("at").and_then(Value::as_u64).unwrap_or(0) < cutoff {
            continue;
        }
        facts.closed += 1;
        match row.get("verdict").and_then(Value::as_str).unwrap_or("") {
            "confirmed" => facts.confirmed += 1,
            "mismatch" => facts.mismatched += 1,
            "unverifiable" => facts.unverifiable += 1,
            _ => {}
        }
    }
    facts.escalations = lines_since(&dir.join("escalations.jsonl"), cutoff).unwrap_or(0);
    facts.algedonic = lines_since(&dir.join("algedonic.jsonl"), cutoff).unwrap_or(0);
    facts.algedonic_limit = fs::read_to_string(dir.join("policy.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.get("algedonic_per_hour").and_then(Value::as_u64))
        .unwrap_or(3) as usize;
    let state = input(
        "team",
        &dir.display().to_string(),
        true,
        format!(
            "закрытий {}, подтверждено {}, расхождений {}, эскалаций {}, алгедоники {} (лимит {})",
            facts.closed,
            facts.confirmed,
            facts.mismatched,
            facts.escalations,
            facts.algedonic,
            facts.algedonic_limit
        ),
    );
    facts.inputs.push(state.clone());
    state
}

/// Пространство кортежей: что взято в работу и сколько раз отказали по ёмкости.
pub fn load_tupleboard(path: &Path, facts: &mut Facts) -> InputState {
    let Ok(raw) = fs::read_to_string(path) else {
        return unreadable(path, facts, "tupleboard");
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return unreadable(path, facts, "tupleboard");
    };
    facts.in_flight = value.get("in_flight").and_then(Value::as_u64).unwrap_or(0) as usize;
    facts.in_space = value.get("in_space").and_then(Value::as_u64).unwrap_or(0) as usize;
    facts.refusals = value
        .get("counters")
        .and_then(|counters| counters.get("refused"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let state = input(
        "tupleboard",
        &path.display().to_string(),
        true,
        format!(
            "в пространстве {}, в работе {}, отказов по ёмкости {}",
            facts.in_space, facts.in_flight, facts.refusals
        ),
    );
    facts.inputs.push(state.clone());
    state
}

fn pact_row(fields: &Value, owner: &str, lease_in: u64) -> Option<PactRow> {
    let row = fields.as_array()?;
    if row.len() != 5 || row[0].as_str()? != "pact" {
        return None;
    }
    let contract = row[4].clone();
    let accepts = format!(
        "{}: {}",
        contract["accepts"]["kind"].as_str().unwrap_or("?"),
        contract["accepts"]["value"].as_str().unwrap_or("?")
    );
    Some(PactRow {
        id: row[3].as_str().unwrap_or("?").to_string(),
        capability: row[1].as_str().unwrap_or("?").to_string(),
        priority: row[2].as_i64().unwrap_or(0),
        accepts,
        owner: owner.to_string(),
        lease_in,
    })
}

/// Доска договоров: `tupleboard` `GET /tuples`, снятый в файл.
pub fn load_pacts(path: &Path, facts: &mut Facts) -> InputState {
    let Ok(raw) = fs::read_to_string(path) else {
        return unreadable(path, facts, "pacts");
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return unreadable(path, facts, "pacts");
    };
    if let Some(rows) = value.get("in_space").and_then(Value::as_array) {
        for row in rows {
            if let Some(pact) = pact_row(&row["fields"], "", 0) {
                facts.pacts_waiting.push(pact);
            }
        }
    }
    if let Some(rows) = value.get("in_flight").and_then(Value::as_array) {
        for row in rows {
            let owner = row["owner"].as_str().unwrap_or("anonymous");
            let lease = row["deadline_in"].as_u64().unwrap_or(0);
            if let Some(pact) = pact_row(&row["fields"], owner, lease) {
                facts.pacts_in_work.push(pact);
            }
        }
    }
    let state = input(
        "pacts",
        &path.display().to_string(),
        true,
        format!(
            "ждут {}, в работе {}",
            facts.pacts_waiting.len(),
            facts.pacts_in_work.len()
        ),
    );
    facts.inputs.push(state.clone());
    state
}

/// Среда: изменения снаружи (например, события сканера архива).
pub fn load_outlook(path: &Path, window: u64, facts: &mut Facts) -> InputState {
    let cutoff = now().saturating_sub(window);
    let Ok(raw) = fs::read_to_string(path) else {
        return unreadable(path, facts, "outlook");
    };
    facts.environmental = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row.get("at").and_then(Value::as_u64).unwrap_or(0) >= cutoff)
        .count();
    let state = input(
        "outlook",
        &path.display().to_string(),
        true,
        format!("внешних изменений за окно: {}", facts.environmental),
    );
    facts.inputs.push(state.clone());
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("homeostat-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn kanban_counts_statuses_and_workers() {
        let dir = temp_dir("kanban");
        let path = dir.join("kanban.json");
        fs::write(
            &path,
            serde_json::json!([
                {"status":"running","assignee":"forgessa"},
                {"status":"running","assignee":"forgessa"},
                {"status":"ready","assignee":"archivessa"},
                {"status":"blocked"},
                {"status":"review"}
            ])
            .to_string(),
        )
        .unwrap();
        let mut facts = Facts::default();
        let state = load_kanban(&path, &mut facts);
        assert!(state.loaded);
        assert_eq!(
            (facts.running, facts.ready, facts.blocked, facts.review),
            (2, 1, 1, 1)
        );
        assert_eq!(
            facts.assignees_working, 1,
            "два running у одного агента — один работник"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnostics_gives_stranded_and_crashes() {
        let dir = temp_dir("diag");
        let path = dir.join("diag.json");
        fs::write(
            &path,
            serde_json::json!([{
                "task_id": "t_1",
                "diagnostics": [
                    {"kind":"stranded_in_ready","count":2},
                    {"kind":"repeated_crashes","count":1,"data":{"consecutive_crashes":3}}
                ]
            }])
            .to_string(),
        )
        .unwrap();
        let mut facts = Facts::default();
        load_diagnostics(&path, &mut facts);
        assert_eq!(facts.stranded, 2);
        assert_eq!(facts.crashed_runs, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn team_counts_only_within_window() {
        let dir = temp_dir("team");
        let stamp = now();
        fs::write(
            dir.join("closed.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                serde_json::json!({"at": stamp, "verdict": "confirmed"}),
                serde_json::json!({"at": stamp, "verdict": "mismatch"}),
                serde_json::json!({"at": stamp.saturating_sub(7200), "verdict": "confirmed"})
            ),
        )
        .unwrap();
        fs::write(
            dir.join("algedonic.jsonl"),
            format!("{}\n", serde_json::json!({"at": stamp, "alert": "диск"})),
        )
        .unwrap();
        fs::write(dir.join("policy.json"), r#"{"algedonic_per_hour":2}"#).unwrap();
        let mut facts = Facts::default();
        load_team(&dir, 3600, &mut facts);
        assert_eq!((facts.closed, facts.confirmed, facts.mismatched), (2, 1, 1));
        assert_eq!(facts.algedonic, 1);
        assert_eq!(
            facts.algedonic_limit, 2,
            "лимит берётся из политики, а не из головы"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_input_is_not_a_zero() {
        let mut facts = Facts::default();
        let state = load_team(
            Path::new("/tmp/homeostat-нет-такого-каталога"),
            3600,
            &mut facts,
        );
        assert!(!state.loaded);
        assert!(state.note.contains("не прочитан"));
        assert_eq!(facts.closed, 0);
        assert_eq!(facts.inputs.len(), 1, "незнание зафиксировано");
    }

    #[test]
    fn tupleboard_stats_are_read() {
        let dir = temp_dir("tb");
        let path = dir.join("tb.json");
        fs::write(
            &path,
            r#"{"in_space":3,"in_flight":2,"counters":{"refused":4}}"#,
        )
        .unwrap();
        let mut facts = Facts::default();
        load_tupleboard(&path, &mut facts);
        assert_eq!((facts.in_space, facts.in_flight, facts.refusals), (3, 2, 4));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pact_board_is_read_from_tuples_dump() {
        let dir = temp_dir("pacts");
        let path = dir.join("tuples.json");
        fs::write(
            &path,
            serde_json::json!({
                "in_space": [
                    {"fields": ["pact", "rust", 5, "p1", {"id":"p1","capability":"rust","accepts":{"kind":"file","value":"/tmp/a"}}]},
                    {"fields": ["что-то", "другое"]}
                ],
                "in_flight": [
                    {"owner": "cell-1", "deadline_in": 42,
                     "fields": ["pact", "python", 1, "p2", {"id":"p2","capability":"python","accepts":{"kind":"url","value":"https://x"}}]}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let mut facts = Facts::default();
        load_pacts(&path, &mut facts);
        assert_eq!(
            facts.pacts_waiting.len(),
            1,
            "только договоры, не любые кортежи"
        );
        assert_eq!(facts.pacts_waiting[0].id, "p1");
        assert_eq!(facts.pacts_waiting[0].accepts, "file: /tmp/a");
        assert_eq!(facts.pacts_in_work[0].owner, "cell-1");
        assert_eq!(facts.pacts_in_work[0].lease_in, 42);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn outlook_counts_recent_events_only() {
        let dir = temp_dir("outlook");
        let path = dir.join("outlook.jsonl");
        let stamp = now();
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::json!({"at": stamp, "what": "RFC 868 понижен"}),
                serde_json::json!({"at": stamp.saturating_sub(99999), "what": "старое"})
            ),
        )
        .unwrap();
        let mut facts = Facts::default();
        load_outlook(&path, 3600, &mut facts);
        assert_eq!(facts.environmental, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
