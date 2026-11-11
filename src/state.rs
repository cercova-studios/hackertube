use crate::error::AppError;
use crate::paths::{data_home, now_iso, today};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Mark {
    pub status: String,
    pub at: String,
    pub until: String,
}

pub fn path_for(home: &Path) -> PathBuf {
    home.join("state.json")
}

pub fn load(home: &Path) -> Result<BTreeMap<String, Mark>, AppError> {
    let path = path_for(home);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(&path)?;
    let payload: Value = serde_json::from_str(&text)
        .map_err(|err| AppError::new("state_invalid", err.to_string()))?;
    let mut marks = BTreeMap::new();
    let Some(items) = payload.get("items").and_then(Value::as_object) else {
        return Ok(marks);
    };
    for (vid, raw) in items {
        let Some(obj) = raw.as_object() else {
            continue;
        };
        let status = obj.get("status").and_then(Value::as_str).unwrap_or("");
        if !matches!(status, "seen" | "liked" | "disliked" | "snoozed") {
            continue;
        }
        let until = match obj.get("until") {
            Some(Value::String(value)) => value.clone(),
            _ => String::new(),
        };
        marks.insert(
            vid.clone(),
            Mark {
                status: status.to_string(),
                at: obj
                    .get("at")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                until,
            },
        );
    }
    Ok(marks)
}

pub fn save(home: &Path, marks: &BTreeMap<String, Mark>) -> Result<(), AppError> {
    fs::create_dir_all(home)?;
    let items = marks
        .iter()
        .map(|(vid, mark)| {
            (
                vid.clone(),
                json!({
                    "status": mark.status,
                    "at": mark.at,
                    "until": if mark.until.is_empty() { Value::Null } else { Value::String(mark.until.clone()) },
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let document = json!({"version": 1, "items": items});
    fs::write(
        path_for(home),
        serde_json::to_string_pretty(&document).expect("state json") + "\n",
    )?;
    Ok(())
}

pub fn set_mark(home: &Path, vid: &str, status: &str, until: &str) -> Result<bool, AppError> {
    let mut marks = load(home)?;
    if let Some(current) = marks.get(vid) {
        if current.status == status && current.until == until {
            return Ok(false);
        }
    }
    marks.insert(
        vid.to_string(),
        Mark {
            status: status.to_string(),
            at: now_iso(),
            until: until.to_string(),
        },
    );
    save(home, &marks)?;
    Ok(true)
}

pub fn forget(home: &Path, vid: &str) -> Result<bool, AppError> {
    let mut marks = load(home)?;
    if marks.remove(vid).is_none() {
        return Ok(false);
    }
    save(home, &marks)?;
    Ok(true)
}

pub fn blocks(mark: &Mark, day: NaiveDate) -> bool {
    match mark.status.as_str() {
        "seen" | "disliked" => true,
        "snoozed" => {
            if mark.until.is_empty() {
                true
            } else {
                crate::paths::parse_day(&mark.until).is_some_and(|until| until >= day)
            }
        }
        _ => false,
    }
}

pub fn counts(marks: &BTreeMap<String, Mark>, day: NaiveDate) -> [(&'static str, i64); 4] {
    let mut seen = 0;
    let mut liked = 0;
    let mut disliked = 0;
    let mut snoozed = 0;
    for mark in marks.values() {
        if mark.status == "snoozed" && !blocks(mark, day) {
            continue;
        }
        match mark.status.as_str() {
            "seen" => seen += 1,
            "liked" => liked += 1,
            "disliked" => disliked += 1,
            "snoozed" => snoozed += 1,
            _ => {}
        }
    }
    [
        ("seen", seen),
        ("liked", liked),
        ("disliked", disliked),
        ("snoozed", snoozed),
    ]
}

pub fn state_line(marks: &BTreeMap<String, Mark>) -> String {
    let tallies = counts(marks, today());
    format!(
        "seen {}, liked {}, disliked {}, snoozed {}",
        tallies[0].1, tallies[1].1, tallies[2].1, tallies[3].1
    )
}

pub fn blocked_vids(marks: &BTreeMap<String, Mark>, day: NaiveDate) -> Vec<String> {
    marks
        .iter()
        .filter(|(_, mark)| blocks(mark, day))
        .map(|(vid, _)| vid.clone())
        .collect()
}

pub fn default_path() -> PathBuf {
    path_for(&data_home())
}
