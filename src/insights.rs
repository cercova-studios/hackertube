//! Deterministic chapters, claims, mentions, and quotes from a timestamped transcript.
//!
//! An optional LLM pass may only keep claims that quote the transcript at a real timestamp.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static TS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(\d{1,2}):)?(\d{1,2}):(\d{2})$").unwrap());
static TS_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[((?:\d{1,2}:)?\d{1,2}:\d{2})\]\((https://www\.youtube\.com/watch\?v=[^)\s]+)\)")
        .unwrap()
});
static ARXIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)https?://arxiv\.org/(?:abs|pdf)/(\d{4}\.\d{4,5})(?:v\d+)?").unwrap()
});
static GITHUB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)").unwrap()
});
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").unwrap());
static STAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[(?:music|applause|laughter|upbeat|cheering)[^\]]*\]").unwrap()
});
static CLAIM_CUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(should|must|means|because|need to|is the|percent|%|instead)\b").unwrap()
});
static SOURCE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^Source:\s*https://www\.youtube\.com/watch\?v=([A-Za-z0-9_-]{11})\s*$")
        .unwrap()
});
static PUBLISHED_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^Published:\s*(\d{4}-\d{2}-\d{2})\s*$").unwrap());
static WATCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https://www\.youtube\.com/watch\?v=([A-Za-z0-9_-]{11})").unwrap()
});
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*]\s+").unwrap());

const DENY: &[&str] = &[
    "tl;dr",
    "tldr",
    "summary",
    "key ideas",
    "key takeaways",
    "slides",
    "full transcript",
    "quotes",
    "tags",
    "references",
    "transcript",
    "anthropic slide decks",
];
const TOOLS: &[(&str, &str)] = &[
    ("Model Context Protocol", "MCP"),
    ("GitHub Copilot", "GitHub Copilot"),
    ("Claude Code", "Claude Code"),
    ("Claude Agent SDK", "Claude Agent SDK"),
    ("SWE-bench", "SWE-bench"),
    ("LlamaIndex", "LlamaIndex"),
    ("LangChain", "LangChain"),
    ("Stagehand", "Stagehand"),
    ("Browserbase", "Browserbase"),
    ("pgvector", "pgvector"),
    ("FastAPI", "FastAPI"),
    ("PyTorch", "PyTorch"),
    ("vLLM", "vLLM"),
    ("Ollama", "Ollama"),
    ("ChatGPT", "ChatGPT"),
    ("SKILL.md", "SKILL.md"),
    ("MCP", "MCP"),
    ("RAG", "RAG"),
];

#[derive(Clone, Debug)]
pub struct Segment {
    pub start_s: i64,
    pub url: String,
    pub text: String,
    pub heading: String,
    pub section: String,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub at: String,
    pub text: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Mention {
    pub kind: String,
    pub name: String,
    pub at: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Insights {
    pub chapters: Vec<Item>,
    pub claims: Vec<Item>,
    pub mentions: Vec<Mention>,
    pub quotes: Vec<Item>,
    pub chapters_total: usize,
    pub claims_total: usize,
    pub mentions_total: usize,
    pub quotes_total: usize,
    pub method: String,
}

pub fn youtube_url(vid: &str, start_s: Option<i64>) -> String {
    match start_s {
        Some(seconds) => format!("https://www.youtube.com/watch?v={vid}&t={seconds}s"),
        None => format!("https://www.youtube.com/watch?v={vid}"),
    }
}

pub fn format_at(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub fn parse_timestamp(value: &str) -> Option<i64> {
    let caps = TS.captures(value.trim())?;
    let hours: i64 = caps
        .get(1)
        .map(|m| m.as_str().parse().unwrap_or(0))
        .unwrap_or(0);
    let minutes: i64 = caps.get(2)?.as_str().parse().ok()?;
    let seconds: i64 = caps.get(3)?.as_str().parse().ok()?;
    Some(hours * 3600 + minutes * 60 + seconds)
}

pub fn identity(markdown: &str) -> Option<(String, String)> {
    let published = PUBLISHED_LINE
        .captures(markdown)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    if let Some(caps) = SOURCE_LINE.captures(markdown) {
        return Some((caps.get(1)?.as_str().to_string(), published));
    }
    WATCH
        .captures(markdown)
        .and_then(|caps| caps.get(1))
        .map(|vid| (vid.as_str().to_string(), published))
}

pub fn parse_markdown(text: &str, vid: &str) -> Vec<Segment> {
    let lines: Vec<String> = text.lines().map(|line| line.trim().to_string()).collect();
    let mut h2 = String::new();
    let mut h3 = String::new();
    let mut segments = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].clone();
        index += 1;
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = HEADING.captures(&line) {
            let marks = caps.get(1).map(|m| m.as_str().len()).unwrap_or(1);
            let title = caps
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            if marks <= 2 {
                h2 = title;
                h3.clear();
            } else {
                h3 = title;
            }
            continue;
        }
        let links: Vec<(String, String)> = TS_LINK
            .captures_iter(&line)
            .filter_map(|caps| {
                Some((
                    caps.get(1)?.as_str().to_string(),
                    caps.get(2)?.as_str().to_string(),
                ))
            })
            .collect();
        if links.is_empty() {
            continue;
        }
        let mut body = clean_body(&TS_LINK.replace_all(&line, ""));
        if body.is_empty() && index < lines.len() {
            let next = &lines[index];
            if !next.is_empty() && HEADING.captures(next).is_none() && !TS_LINK.is_match(next) {
                body = clean_body(next);
                index += 1;
            }
        }
        let Some(start) = parse_timestamp(&links[0].0) else {
            continue;
        };
        let mut url = links[0].1.clone();
        if !url.contains(vid) {
            url = youtube_url(vid, Some(start));
        }
        let heading = if h3.is_empty() {
            h2.clone()
        } else {
            h3.clone()
        };
        segments.push(Segment {
            start_s: start,
            url,
            text: body,
            heading,
            section: h2.clone(),
        });
    }
    segments
}

fn clean_body(line: &str) -> String {
    let without_images = IMAGE.replace_all(line, "");
    let without_links = LINK.replace_all(&without_images, "$1");
    let without_stage = STAGE.replace_all(&without_links, "");
    let without_bullet = BULLET.replace_all(without_stage.trim(), "");
    without_bullet
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch| ch == ' ' || ch == '-')
        .to_string()
}

pub fn extract(segments: &[Segment], plain: &str, vid: &str, limit_scale: usize) -> Insights {
    let mut chapters = chapters_of(segments);
    chapters.sort_by_key(by_time);
    let mut claims = claims_of(segments);
    if claims.is_empty() {
        claims = fallback_claims(segments);
    }
    claims.sort_by_key(by_time);
    let mut mentions = mentions_of(segments);
    if !plain.is_empty() && !vid.is_empty() {
        mentions = mentions_in_plain(plain, vid, mentions);
    }
    let mut quotes = quotes_of(segments);
    quotes.sort_by_key(by_time);
    let scale = limit_scale.max(1);
    Insights {
        chapters_total: chapters.len(),
        claims_total: claims.len(),
        mentions_total: mentions.len(),
        quotes_total: quotes.len(),
        chapters: take(chapters, 8 * scale),
        claims: take(claims, 6 * scale),
        mentions: take(mentions, 8 * scale),
        quotes: take(quotes, 3 * scale),
        method: "deterministic".to_string(),
    }
}

fn take<T>(mut items: Vec<T>, cap: usize) -> Vec<T> {
    items.truncate(cap);
    items
}

fn by_time(item: &Item) -> (i64, String) {
    (parse_timestamp(&item.at).unwrap_or(0), item.text.clone())
}

fn denied(title: &str) -> bool {
    let normalized = title.trim().trim_end_matches(':').to_lowercase();
    DENY.contains(&normalized.as_str())
}

fn chapters_of(segments: &[Segment]) -> Vec<Item> {
    let mut seen = Vec::new();
    let mut items = Vec::new();
    for segment in segments {
        let title = segment.heading.trim();
        if title.is_empty() || denied(title) || seen.iter().any(|item: &String| item == title) {
            continue;
        }
        let section = segment.section.trim().trim_end_matches(':').to_lowercase();
        if DENY.contains(&section.as_str()) && title == segment.section.trim() {
            continue;
        }
        seen.push(title.to_string());
        items.push(Item {
            at: format_at(segment.start_s),
            text: title.to_string(),
            url: segment.url.clone(),
        });
    }
    items
}

fn claims_of(segments: &[Segment]) -> Vec<Item> {
    let under = |name: &str| {
        segments
            .iter()
            .filter(|segment| {
                segment
                    .section
                    .trim()
                    .trim_end_matches(':')
                    .eq_ignore_ascii_case(name)
                    && segment.text.trim().chars().count() >= 40
            })
            .collect::<Vec<_>>()
    };
    let mut chosen = under("key ideas");
    if chosen.is_empty() {
        chosen = under("key takeaways");
    }
    let mut items = Vec::new();
    let mut seen = Vec::new();
    for segment in chosen {
        let text = segment.text.trim().to_string();
        let key = text.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        items.push(Item {
            at: format_at(segment.start_s),
            text,
            url: segment.url.clone(),
        });
    }
    items
}

fn fallback_claims(segments: &[Segment]) -> Vec<Item> {
    let mut items = Vec::new();
    for segment in segments {
        let section = segment.section.trim().trim_end_matches(':').to_lowercase();
        let in_transcript = matches!(section.as_str(), "full transcript" | "transcript" | "")
            || section.contains("transcript")
            || segment.heading == "Full transcript";
        if !in_transcript {
            continue;
        }
        let text = segment.text.trim();
        let len = text.chars().count();
        if !(40..=320).contains(&len) || !CLAIM_CUE.is_match(text) {
            continue;
        }
        items.push(Item {
            at: format_at(segment.start_s),
            text: text.to_string(),
            url: segment.url.clone(),
        });
    }
    items
}

fn mentions_of(segments: &[Segment]) -> Vec<Mention> {
    let mut found = Vec::new();
    let mut seen = Vec::new();
    for segment in segments {
        let at = format_at(segment.start_s);
        for caps in GITHUB.captures_iter(&segment.text) {
            let owner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let repo = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if matches!(owner, "topics" | "orgs" | "features")
                || matches!(repo, "blob" | "tree" | "issues")
            {
                continue;
            }
            add_mention(
                &mut found,
                &mut seen,
                "repo",
                &format!("github.com/{owner}/{repo}"),
                &at,
                &segment.url,
            );
        }
        for caps in ARXIV.captures_iter(&segment.text) {
            if let Some(id) = caps.get(1) {
                add_mention(
                    &mut found,
                    &mut seen,
                    "paper",
                    &format!("arxiv:{}", id.as_str()),
                    &at,
                    &segment.url,
                );
            }
        }
        let mut occupied: Vec<(usize, usize)> = Vec::new();
        for (pattern, label) in TOOLS {
            let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(pattern))) else {
                continue;
            };
            for found_at in re.find_iter(&segment.text) {
                let span = (found_at.start(), found_at.end());
                if occupied
                    .iter()
                    .any(|(start, end)| span.0 < *end && span.1 > *start)
                {
                    continue;
                }
                occupied.push(span);
                add_mention(&mut found, &mut seen, "tool", label, &at, &segment.url);
            }
        }
    }
    found.sort_by(|a, b| {
        (&a.kind, a.name.to_lowercase(), &a.at).cmp(&(&b.kind, b.name.to_lowercase(), &b.at))
    });
    found
}

fn mentions_in_plain(plain: &str, vid: &str, mut found: Vec<Mention>) -> Vec<Mention> {
    let fake = [Segment {
        start_s: 0,
        url: youtube_url(vid, None),
        text: plain.to_string(),
        heading: String::new(),
        section: String::new(),
    }];
    for item in mentions_of(&fake) {
        if found
            .iter()
            .any(|have| have.kind == item.kind && have.name.eq_ignore_ascii_case(&item.name))
        {
            continue;
        }
        found.push(Mention {
            kind: item.kind,
            name: item.name,
            at: String::new(),
            url: youtube_url(vid, None),
        });
    }
    found.sort_by(|a, b| {
        (&a.kind, a.name.to_lowercase(), &a.at).cmp(&(&b.kind, b.name.to_lowercase(), &b.at))
    });
    found
}

fn add_mention(
    found: &mut Vec<Mention>,
    seen: &mut Vec<(String, String)>,
    kind: &str,
    name: &str,
    at: &str,
    url: &str,
) {
    let key = (kind.to_string(), name.to_lowercase());
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    found.push(Mention {
        kind: kind.to_string(),
        name: name.to_string(),
        at: at.to_string(),
        url: url.to_string(),
    });
}

fn quotes_of(segments: &[Segment]) -> Vec<Item> {
    let transcript: Vec<&Segment> = segments
        .iter()
        .filter(|segment| {
            segment.section.to_lowercase().contains("transcript")
                || segment.heading.eq_ignore_ascii_case("full transcript")
        })
        .collect();
    let pool: Vec<&Segment> = if transcript.is_empty() {
        segments.iter().collect()
    } else {
        transcript
    };
    let mut picked = Vec::new();
    let mut seen = Vec::new();
    for (minimum, maximum) in [(8usize, 28usize), (6, 40)] {
        for segment in &pool {
            let words = segment.text.split_whitespace().count();
            if !(minimum..=maximum).contains(&words) {
                continue;
            }
            let key = segment.text.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            picked.push(Item {
                at: format_at(segment.start_s),
                text: segment.text.clone(),
                url: segment.url.clone(),
            });
            if picked.len() >= 3 && minimum == 8 {
                return picked;
            }
        }
        if !picked.is_empty() {
            return picked;
        }
    }
    picked
}

pub fn known_timestamps(segments: &[Segment]) -> Vec<String> {
    segments
        .iter()
        .map(|segment| format_at(segment.start_s))
        .collect()
}

pub fn merge_llm_claims(
    insights: Insights,
    payload: &Value,
    allowed: &[String],
    source: &str,
) -> Insights {
    let Some(claims) = payload.get("claims").and_then(Value::as_array) else {
        return insights;
    };
    let mut grounded = Vec::new();
    for claim in claims {
        let at = claim.get("at").and_then(Value::as_str).unwrap_or("");
        let text = claim
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !allowed.iter().any(|stamp| stamp == at) || text.chars().count() < 20 {
            continue;
        }
        if !source.to_lowercase().contains(&text.to_lowercase()) {
            continue;
        }
        let url = claim
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_string)
            .or_else(|| {
                insights
                    .claims
                    .iter()
                    .find(|item| item.at == at)
                    .map(|item| item.url.clone())
            })
            .unwrap_or_default();
        grounded.push(Item {
            at: at.to_string(),
            text,
            url,
        });
    }
    if grounded.is_empty() {
        return insights;
    }
    let total = grounded.len();
    Insights {
        claims: grounded,
        claims_total: total,
        method: "llm".to_string(),
        ..insights
    }
}

pub fn chunks_from_segments(segments: &[Segment]) -> Vec<Segment> {
    let mut chunks = Vec::new();
    let mut buf = String::new();
    let mut start = 0;
    let mut url = String::new();
    let flush = |chunks: &mut Vec<Segment>, buf: &mut String, start: i64, url: &str| {
        let text = buf.trim().to_string();
        if text.is_empty() {
            return;
        }
        chunks.push(Segment {
            start_s: start,
            url: url.to_string(),
            text,
            heading: String::new(),
            section: "chunk".to_string(),
        });
        buf.clear();
    };
    for segment in segments {
        if segment.text.trim().is_empty() {
            continue;
        }
        if buf.is_empty() {
            start = segment.start_s;
            url = segment.url.clone();
        }
        if !buf.is_empty() && buf.chars().count() + segment.text.chars().count() > 800 {
            flush(&mut chunks, &mut buf, start, &url);
            start = segment.start_s;
            url = segment.url.clone();
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(segment.text.trim());
    }
    flush(&mut chunks, &mut buf, start, &url);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    const TALK: &str = include_str!("../tests/fixtures/talk.md");

    #[test]
    fn extracts_fixture_talk() {
        let (vid, published) = identity(TALK).unwrap();
        assert_eq!(vid, "abcdefghijk");
        assert_eq!(published, "2026-01-15");
        let segments = parse_markdown(TALK, "abcdefghijk");
        let insights = extract(&segments, TALK, "abcdefghijk", 1);
        assert!(insights
            .chapters
            .iter()
            .any(|item| item.text.starts_with("Agents need a harness")));
        assert!(insights
            .claims
            .iter()
            .any(|item| item.text.contains("graded on completed work")));
        assert!(insights
            .claims
            .iter()
            .all(|item| item.url.contains("abcdefghijk") && !item.at.is_empty()));
        let names: Vec<&str> = insights
            .mentions
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert!(names.contains(&"github.com/browserbase/stagehand"));
        assert!(names.contains(&"arxiv:2406.01234"));
        assert!(names.contains(&"Claude Code"));
        assert!(names.contains(&"MCP"));
        assert!(insights
            .quotes
            .iter()
            .any(|item| item.text.contains("completed work")));
        let repo = insights
            .mentions
            .iter()
            .find(|item| item.kind == "repo")
            .unwrap();
        assert_eq!(repo.at, "2:10");
        assert!(repo.url.contains("t=130"));
    }

    #[test]
    fn llm_merge_keeps_verbatim_only() {
        let segments = parse_markdown(TALK, "abcdefghijk");
        let insights = extract(&segments, TALK, "abcdefghijk", 1);
        let verbatim = "A coding agent should be graded on completed work, not a single answer.";
        let payload = serde_json::json!({
            "claims": [
                {"at": "1:05", "text": verbatim},
                {"at": "9:99", "text": verbatim},
                {"at": "1:05", "text": "This paraphrase is not in the transcript."}
            ]
        });
        let allowed = vec!["1:05".to_string(), "2:10".to_string(), "0:00".to_string()];
        let merged = merge_llm_claims(insights, &payload, &allowed, TALK);
        assert_eq!(merged.method, "llm");
        assert_eq!(merged.claims.len(), 1);
        assert_eq!(merged.claims[0].text, verbatim);
    }
}
