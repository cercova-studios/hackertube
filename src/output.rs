use crate::error::UsageError;
use serde_json::{Map, Number, Value as Json};
use std::io::{self, Write};

const TEXT_LIMIT: usize = 180;
const CELL_LIMIT: usize = 110;
const PROTECTED: &[&str] = &[
    "vid",
    "url",
    "page",
    "source_url",
    "bin",
    "home",
    "catalog",
    "state_file",
    "flag",
    "cache",
];

#[derive(Clone, Debug)]
pub enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Val>),
    Table {
        fields: Vec<String>,
        rows: Vec<Vec<Val>>,
    },
}

impl From<bool> for Val {
    fn from(value: bool) -> Self {
        Val::Bool(value)
    }
}

impl From<i64> for Val {
    fn from(value: i64) -> Self {
        Val::Num(value as f64)
    }
}

impl From<f64> for Val {
    fn from(value: f64) -> Self {
        Val::Num(value)
    }
}

impl From<String> for Val {
    fn from(value: String) -> Self {
        Val::Str(value)
    }
}

impl From<&str> for Val {
    fn from(value: &str) -> Self {
        Val::Str(value.to_string())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub entries: Vec<(String, Val)>,
}

impl Doc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, key: impl Into<String>, value: impl Into<Val>) {
        self.entries.push((key.into(), value.into()));
    }

    pub fn table(&mut self, key: &str, fields: &[&str], rows: Vec<Vec<Val>>) {
        self.entries.push((
            key.to_string(),
            Val::Table {
                fields: fields.iter().map(|field| (*field).to_string()).collect(),
                rows,
            },
        ));
    }

    pub fn help(&mut self, lines: &[&str]) {
        self.add(
            "help",
            Val::List(
                lines
                    .iter()
                    .map(|line| Val::Str((*line).to_string()))
                    .collect(),
            ),
        );
    }
}

pub fn project_fields(
    rows: &[Vec<(String, Val)>],
    default: &[&str],
    requested: Option<&[String]>,
    allowed: &[&str],
) -> Result<(Vec<String>, Vec<Vec<Val>>), UsageError> {
    let fields: Vec<String> = if let Some(asked) = requested {
        if asked.is_empty() {
            return Err(UsageError::new("--fields needs at least one name").flag("--fields"));
        }
        if let Some(unknown) = asked
            .iter()
            .find(|field| !allowed.contains(&field.as_str()))
        {
            return Err(UsageError::new(format!("unknown field {unknown}")).flag("--fields"));
        }
        asked.to_vec()
    } else {
        default.iter().map(|field| (*field).to_string()).collect()
    };
    let projected = rows
        .iter()
        .map(|row| {
            fields
                .iter()
                .map(|field| {
                    row.iter()
                        .find(|(key, _)| key == field)
                        .map(|(_, value)| value.clone())
                        .unwrap_or(Val::Null)
                })
                .collect()
        })
        .collect();
    Ok((fields, projected))
}

pub fn render(doc: &Doc, as_json: bool, full: bool) -> String {
    if as_json {
        serde_json::to_string_pretty(&doc_json(doc, full)).expect("json") + "\n"
    } else {
        to_toon(doc, full) + "\n"
    }
}

pub fn emit(doc: &Doc, as_json: bool, full: bool) {
    let mut out = io::stdout().lock();
    let _ = out.write_all(render(doc, as_json, full).as_bytes());
}

pub fn error_doc(
    code: &str,
    detail: &str,
    exit_code: i64,
    help: &[&str],
    extra: &[(String, String)],
) -> Doc {
    let mut doc = Doc::new();
    doc.add("ok", false);
    doc.add("error", code);
    doc.add("detail", detail);
    doc.add("exit", exit_code);
    for (key, value) in extra {
        doc.add(key, value.clone());
    }
    let lines = if help.is_empty() {
        vec!["hackertube --help"]
    } else {
        help.to_vec()
    };
    doc.help(&lines);
    doc
}

fn doc_json(doc: &Doc, full: bool) -> Json {
    let mut map = Map::new();
    for (key, value) in &doc.entries {
        map.insert(key.clone(), val_json(key, value, full));
    }
    Json::Object(map)
}

fn val_json(key: &str, value: &Val, full: bool) -> Json {
    match value {
        Val::Null => Json::Null,
        Val::Bool(value) => Json::Bool(*value),
        Val::Num(value) => number_json(*value),
        Val::Str(value) => Json::String(maybe_truncate(key, value, full)),
        Val::List(values) => Json::Array(
            values
                .iter()
                .map(|value| val_json("", value, full))
                .collect(),
        ),
        Val::Table { fields, rows } => Json::Array(
            rows.iter()
                .map(|row| {
                    let mut object = Map::new();
                    for (field, cell) in fields.iter().zip(row.iter()) {
                        object.insert(field.clone(), val_json(field, cell, full));
                    }
                    Json::Object(object)
                })
                .collect(),
        ),
    }
}

fn number_json(value: f64) -> Json {
    if !value.is_finite() {
        return Json::Null;
    }
    if (value - value.round()).abs() < 1e-9 && value.abs() < 1e15 {
        return Json::Number(Number::from(value.round() as i64));
    }
    serde_json::Number::from_f64(value)
        .map(Json::Number)
        .unwrap_or(Json::Null)
}

fn to_toon(doc: &Doc, full: bool) -> String {
    let mut lines = Vec::new();
    for (key, value) in &doc.entries {
        encode(&mut lines, key, value, 0, full);
    }
    lines.join("\n")
}

fn encode(lines: &mut Vec<String>, key: &str, value: &Val, depth: usize, full: bool) {
    let indent = "  ".repeat(depth);
    let name = quote_key(key);
    match value {
        Val::Table { fields, rows } => {
            lines.push(format!(
                "{indent}{name}[{}]{{{}}}:",
                rows.len(),
                fields.join(",")
            ));
            for row in rows {
                let cells: Vec<String> = fields
                    .iter()
                    .zip(row.iter())
                    .map(|(field, cell)| quote_cell(&display_cell(field, cell, full), ","))
                    .collect();
                lines.push(format!("{indent}  {}", cells.join(",")));
            }
        }
        Val::List(values) => {
            let cells: Vec<String> = values
                .iter()
                .map(|value| quote_cell(&display_cell("", value, full), ","))
                .collect();
            let joined = cells.join(",");
            lines.push(
                format!("{indent}{name}[{}]: {joined}", values.len())
                    .trim_end()
                    .to_string(),
            );
        }
        other => {
            lines.push(format!("{indent}{name}: {}", scalar(key, other, full)));
        }
    }
}

fn display_cell(field: &str, value: &Val, full: bool) -> String {
    match value {
        Val::Null => "null".to_string(),
        Val::Bool(value) => {
            if *value {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Val::Num(value) => format_number(*value),
        Val::Str(value) => maybe_truncate(field, value, full),
        Val::List(_) | Val::Table { .. } => String::new(),
    }
}

fn scalar(key: &str, value: &Val, full: bool) -> String {
    match value {
        Val::Str(text) => {
            let shown = maybe_truncate(key, text, full);
            if shown.is_empty()
                || shown.contains('\n')
                || shown != shown.trim()
                || shown.starts_with('{')
                || shown.starts_with('[')
            {
                quote_cell(&shown, ",")
            } else {
                shown
            }
        }
        other => display_cell(key, other, full),
    }
}

fn maybe_truncate(field: &str, value: &str, full: bool) -> String {
    if full || PROTECTED.contains(&field) {
        return value.to_string();
    }
    let limit = if matches!(field, "text" | "summary" | "why" | "reason") {
        TEXT_LIMIT
    } else if field.is_empty() {
        return value.to_string();
    } else {
        CELL_LIMIT
    };
    truncate(value, limit)
}

pub fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let cut_chars = limit.saturating_sub(14).max(1);
    let cut: String = value.chars().take(cut_chars).collect();
    let cut = cut.trim_end().to_string();
    let omitted = value.chars().count() - cut.chars().count();
    format!("{cut}…[+{omitted}c]")
}

fn quote_key(key: &str) -> String {
    if is_bare_key(key) {
        key.to_string()
    } else {
        quote_cell(key, ",")
    }
}

fn is_bare_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn quote_cell(text: &str, delimiter: &str) -> String {
    if text == "null" || text == "true" || text == "false" {
        return text.to_string();
    }
    let numeric = is_numeric(text);
    let needs = text.is_empty()
        || text != text.trim()
        || text.contains(delimiter)
        || text.contains('"')
        || text.contains('\\')
        || text.contains('\n')
        || text.contains('\r')
        || text.contains('\t')
        || matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "false" | "null"
        )
        || numeric;
    if !needs {
        return text.to_string();
    }
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn is_numeric(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    if matches!(chars.peek(), Some('-')) {
        chars.next();
    }
    let body: String = chars.collect();
    if body.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    for ch in body.chars() {
        if ch == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    if (value - value.round()).abs() < 1e-9 && value.abs() < 1e15 {
        return format!("{}", value.round() as i64);
    }
    let text = format!("{value:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_quotes_commas_and_empty_keeps_fields() {
        let mut doc = Doc::new();
        doc.table(
            "videos",
            &["vid", "title"],
            vec![vec![Val::from("abc"), Val::from("Hello, world")]],
        );
        doc.help(&["hackertube show abc"]);
        let text = render(&doc, false, false);
        assert!(text.contains("videos[1]{vid,title}:"));
        assert!(text.contains("\"Hello, world\""));
        assert!(text.contains("help[1]: hackertube show abc"));
        let mut empty = Doc::new();
        empty.table("videos", &["vid", "title", "score", "reason"], vec![]);
        let text = render(&empty, false, false);
        assert!(text.contains("videos[0]{vid,title,score,reason}:"));
    }

    #[test]
    fn json_whole_numbers_and_truncate() {
        let mut doc = Doc::new();
        doc.table(
            "videos",
            &["vid", "score"],
            vec![vec![Val::from("abc"), Val::from(10.0)]],
        );
        let json: serde_json::Value = serde_json::from_str(&render(&doc, true, false)).unwrap();
        assert_eq!(json["videos"][0]["score"], 10);
        assert!(truncate("abcdefghij", 8).contains("…[+"));
    }
}
