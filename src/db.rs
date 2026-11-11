use crate::error::AppError;
use crate::vecstore::{self, SqliteVec, VectorIndex};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "2";

#[derive(Clone, Debug)]
pub struct Video {
    pub vid: String,
    pub source: String,
    pub url: String,
    pub page_url: String,
    pub title: String,
    pub channel: String,
    pub summary: String,
    pub why: String,
    pub content_type: String,
    pub level: String,
    pub shelf_life: String,
    pub primary_topic: String,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub topic_slugs: Vec<String>,
    pub quote: String,
    pub hn_recommenders: i64,
    pub score: f64,
    pub published: String,
    pub event: String,
    pub transcript_path: String,
    pub embed_row: Option<i64>,
    pub area_agentic: i32,
    pub area_engineering: i32,
    pub area_research: i32,
    pub updated_at: String,
}

pub struct Catalog {
    pub conn: Connection,
    pub path: PathBuf,
}

impl Catalog {
    pub fn open(home: &Path) -> Result<Self, AppError> {
        vecstore::install()?;
        std::fs::create_dir_all(home)?;
        let path = home.join("catalog.sqlite");
        if path.exists() {
            let conn = Connection::open(&path)?;
            let version = meta_get(&conn, "schema_version")?;
            drop(conn);
            if version.as_deref() != Some(SCHEMA_VERSION) && version.is_some() {
                let backup = home.join("catalog.sqlite.v1");
                let _ = std::fs::rename(&path, backup);
            }
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        init(&conn)?;
        Ok(Self { conn, path })
    }

    pub fn exists(home: &Path) -> bool {
        home.join("catalog.sqlite").exists()
    }
}

fn init(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS videos (
           vid TEXT PRIMARY KEY,
           source TEXT NOT NULL,
           url TEXT NOT NULL,
           page_url TEXT NOT NULL DEFAULT '',
           title TEXT NOT NULL,
           channel TEXT NOT NULL DEFAULT '',
           summary TEXT NOT NULL DEFAULT '',
           why TEXT NOT NULL DEFAULT '',
           content_type TEXT NOT NULL DEFAULT '',
           level TEXT NOT NULL DEFAULT '',
           shelf_life TEXT NOT NULL DEFAULT '',
           primary_topic TEXT NOT NULL DEFAULT '',
           tags_json TEXT NOT NULL DEFAULT '[]',
           categories_json TEXT NOT NULL DEFAULT '[]',
           topics_json TEXT NOT NULL DEFAULT '[]',
           quote TEXT NOT NULL DEFAULT '',
           hn_recommenders INTEGER NOT NULL DEFAULT 0,
           score REAL NOT NULL DEFAULT 0,
           published TEXT NOT NULL DEFAULT '',
           event TEXT NOT NULL DEFAULT '',
           transcript_path TEXT NOT NULL DEFAULT '',
           embed_row INTEGER,
           area_agentic INTEGER NOT NULL DEFAULT 0,
           area_engineering INTEGER NOT NULL DEFAULT 0,
           area_research INTEGER NOT NULL DEFAULT 0,
           blocked INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL DEFAULT ''
         );
         CREATE TABLE IF NOT EXISTS mentions (
           hn_comment_id INTEGER NOT NULL,
           vid TEXT NOT NULL,
           hn_url TEXT NOT NULL DEFAULT '',
           stance TEXT NOT NULL DEFAULT '',
           why TEXT NOT NULL DEFAULT '',
           quote TEXT NOT NULL DEFAULT '',
           month TEXT NOT NULL DEFAULT '',
           PRIMARY KEY (hn_comment_id, vid)
         );
         CREATE TABLE IF NOT EXISTS related (
           vid TEXT NOT NULL,
           other_vid TEXT NOT NULL,
           shared INTEGER NOT NULL,
           PRIMARY KEY (vid, other_vid)
         );
         CREATE TABLE IF NOT EXISTS chunks (
           id INTEGER PRIMARY KEY,
           vid TEXT NOT NULL,
           idx INTEGER NOT NULL,
           start_s INTEGER NOT NULL,
           text TEXT NOT NULL,
           url TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS emb_videos (
           vid TEXT PRIMARY KEY,
           emb BLOB NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS videos_fts USING fts5(vid, title, channel, summary, why, topic, tags);
         CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(vid, text, start_s UNINDEXED, idx UNINDEXED);",
    )?;
    SqliteVec { conn }.ensure_schema()?;
    meta_set(conn, "schema_version", SCHEMA_VERSION)?;
    Ok(())
}

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let value = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn next_id(conn: &Connection, key: &str) -> Result<i64, AppError> {
    let current: i64 = meta_get(conn, key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let next = current + 1;
    meta_set(conn, key, &next.to_string())?;
    Ok(next)
}

pub fn upsert_video(conn: &Connection, video: &Video) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO videos(
            vid, source, url, page_url, title, channel, summary, why, content_type, level,
            shelf_life, primary_topic, tags_json, categories_json, topics_json, quote,
            hn_recommenders, score, published, event, transcript_path, embed_row,
            area_agentic, area_engineering, area_research, blocked, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,0,?26)
         ON CONFLICT(vid) DO UPDATE SET
            source=excluded.source, url=excluded.url, page_url=excluded.page_url, title=excluded.title,
            channel=excluded.channel, summary=excluded.summary, why=excluded.why,
            content_type=excluded.content_type, level=excluded.level, shelf_life=excluded.shelf_life,
            primary_topic=excluded.primary_topic, tags_json=excluded.tags_json,
            categories_json=excluded.categories_json, topics_json=excluded.topics_json,
            quote=excluded.quote, hn_recommenders=excluded.hn_recommenders, score=excluded.score,
            published=excluded.published, event=excluded.event, transcript_path=excluded.transcript_path,
            embed_row=excluded.embed_row, area_agentic=excluded.area_agentic,
            area_engineering=excluded.area_engineering, area_research=excluded.area_research,
            updated_at=excluded.updated_at",
        params![
            video.vid, video.source, video.url, video.page_url, video.title, video.channel,
            video.summary, video.why, video.content_type, video.level, video.shelf_life,
            video.primary_topic, serde_json::to_string(&video.tags).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&video.categories).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&video.topic_slugs).unwrap_or_else(|_| "[]".into()),
            video.quote, video.hn_recommenders, video.score, video.published, video.event,
            video.transcript_path, video.embed_row, video.area_agentic, video.area_engineering,
            video.area_research, video.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_video(conn: &Connection, vid: &str) -> Result<Option<Video>, AppError> {
    let mut stmt = conn.prepare("SELECT * FROM videos WHERE vid = ?1")?;
    let mut rows = stmt.query(params![vid])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(video_from_row(row)?));
    }
    Ok(None)
}

fn video_from_row(row: &rusqlite::Row<'_>) -> Result<Video, AppError> {
    let tags = json_list(
        row.get::<_, String>("tags_json")
            .unwrap_or_else(|_| "[]".into()),
    );
    let categories = json_list(
        row.get::<_, String>("categories_json")
            .unwrap_or_else(|_| "[]".into()),
    );
    let topic_slugs = json_list(
        row.get::<_, String>("topics_json")
            .unwrap_or_else(|_| "[]".into()),
    );
    Ok(Video {
        vid: row.get("vid")?,
        source: row.get("source")?,
        url: row.get("url")?,
        page_url: row.get("page_url")?,
        title: row.get("title")?,
        channel: row.get("channel")?,
        summary: row.get("summary")?,
        why: row.get("why")?,
        content_type: row.get("content_type")?,
        level: row.get("level")?,
        shelf_life: row.get("shelf_life")?,
        primary_topic: row.get("primary_topic")?,
        tags,
        categories,
        topic_slugs,
        quote: row.get("quote")?,
        hn_recommenders: row.get("hn_recommenders")?,
        score: row.get("score")?,
        published: row.get("published")?,
        event: row.get("event")?,
        transcript_path: row.get("transcript_path")?,
        embed_row: row.get("embed_row")?,
        area_agentic: row.get("area_agentic")?,
        area_engineering: row.get("area_engineering")?,
        area_research: row.get("area_research")?,
        updated_at: row.get("updated_at")?,
    })
}

fn json_list(text: String) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(&text)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

pub fn source_counts(conn: &Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt =
        conn.prepare("SELECT source, COUNT(*) FROM videos GROUP BY source ORDER BY source")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn video_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row("SELECT COUNT(*) FROM videos", [], |row| row.get(0))?)
}

pub fn sync_blocked(conn: &Connection, vids: &[String]) -> Result<(), AppError> {
    conn.execute("UPDATE videos SET blocked = 0", [])?;
    SqliteVec { conn }.set_blocked(vids)?;
    for vid in vids {
        conn.execute("UPDATE videos SET blocked = 1 WHERE vid = ?1", params![vid])?;
    }
    Ok(())
}

pub fn store_embedding(conn: &Connection, vid: &str, embedding: &[f32]) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO emb_videos(vid, emb) VALUES(?1, ?2)
         ON CONFLICT(vid) DO UPDATE SET emb = excluded.emb",
        params![vid, vecstore::f32_bytes(embedding)],
    )?;
    Ok(())
}

pub fn load_embedding(conn: &Connection, vid: &str) -> Result<Option<Vec<f32>>, AppError> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT emb FROM emb_videos WHERE vid = ?1",
            params![vid],
            |row| row.get(0),
        )
        .optional()?;
    Ok(blob.map(bytes_to_f32))
}

pub fn bytes_to_f32(bytes: Vec<u8>) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub fn replace_chunks(
    conn: &Connection,
    vid: &str,
    chunks: &[(i64, i64, String, String)],
) -> Result<Vec<i64>, AppError> {
    conn.execute("DELETE FROM chunks WHERE vid = ?1", params![vid])?;
    conn.execute("DELETE FROM chunks_fts WHERE vid = ?1", params![vid])?;
    let _ = conn.execute("DELETE FROM vec_chunks WHERE vid = ?1", params![vid]);
    let mut ids = Vec::with_capacity(chunks.len());
    for (idx, start_s, text, url) in chunks {
        let id = next_id(conn, "chunk_id")?;
        conn.execute(
            "INSERT INTO chunks(id, vid, idx, start_s, text, url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, vid, idx, start_s, text, url],
        )?;
        conn.execute(
            "INSERT INTO chunks_fts(vid, text, start_s, idx) VALUES (?1, ?2, ?3, ?4)",
            params![vid, text, start_s, idx],
        )?;
        ids.push(id);
    }
    Ok(ids)
}

pub fn rebuild_video_fts(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM videos_fts", [])?;
    let mut stmt = conn.prepare(
        "SELECT vid, title, channel, summary, why, primary_topic, tags_json FROM videos",
    )?;
    let rows = stmt.query_map([], |row| {
        let tags: String = row.get(6)?;
        let joined = json_list(tags).join(" ");
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            joined,
        ))
    })?;
    let mut collected = Vec::new();
    for row in rows {
        collected.push(row?);
    }
    for (vid, title, channel, summary, why, topic, tags) in collected {
        conn.execute(
            "INSERT INTO videos_fts(vid, title, channel, summary, why, topic, tags) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![vid, title, channel, summary, why, topic, tags],
        )?;
    }
    Ok(())
}

pub fn keyword_videos(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<(String, f64)>, AppError> {
    let Some(match_query) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT vid, bm25(videos_fts) FROM videos_fts WHERE videos_fts MATCH ?1 ORDER BY bm25(videos_fts) LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![match_query, limit], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    let mut raw = Vec::new();
    for row in rows {
        raw.push(row?);
    }
    Ok(normalize_bm25(raw))
}

pub fn keyword_chunks(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<(String, i64, i64, f64)>, AppError> {
    let Some(match_query) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT vid, start_s, idx, bm25(chunks_fts) FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts) LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![match_query, limit], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    let mut raw = Vec::new();
    for row in rows {
        let (vid, start_s, idx, rank): (String, i64, i64, f64) = row?;
        raw.push((format!("{vid}:{idx}"), rank, vid, start_s, idx));
    }
    let weights = normalize_bm25(
        raw.iter()
            .map(|(key, rank, _, _, _)| (key.clone(), *rank))
            .collect(),
    );
    let mut out = Vec::new();
    for (key, _, vid, start_s, idx) in raw {
        let score = weights
            .iter()
            .find(|(id, _)| id == &key)
            .map(|(_, score)| *score)
            .unwrap_or(0.0);
        out.push((vid, start_s, idx, score));
    }
    Ok(out)
}

fn normalize_bm25(rows: Vec<(String, f64)>) -> Vec<(String, f64)> {
    if rows.is_empty() {
        return rows;
    }
    let strengths: Vec<(String, f64)> = rows.into_iter().map(|(key, rank)| (key, -rank)).collect();
    let peak = strengths
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max);
    if peak <= 0.0 {
        return strengths.into_iter().map(|(key, _)| (key, 0.0)).collect();
    }
    strengths
        .into_iter()
        .map(|(key, value)| (key, value / peak))
        .collect()
}

pub fn fts_query(text: &str) -> Option<String> {
    let tokens: Vec<String> = text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .take(12)
        .map(|token| format!("\"{}\"", token.to_lowercase()))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" OR "))
    }
}

pub fn chunk_text(
    conn: &Connection,
    vid: &str,
    idx: i64,
) -> Result<Option<(String, String)>, AppError> {
    let row = conn
        .query_row(
            "SELECT text, url FROM chunks WHERE vid = ?1 AND idx = ?2",
            params![vid, idx],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row)
}

pub fn related_to(
    conn: &Connection,
    vid: &str,
    limit: i64,
) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT other_vid, shared FROM related WHERE vid = ?1 ORDER BY shared DESC, other_vid ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![vid, limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn area_column(area: &str) -> Option<&'static str> {
    crate::areas::Affinity::column(area)
}
