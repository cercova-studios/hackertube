//! sqlite-vec backed vector index.
//!
//! [`VectorIndex`] is the swap point. A fork of sqlite-vec (for example
//! github.com/cercova-studios/sqlite-vec) can replace [`install`] and
//! [`SqliteVec`] without changing search or ingest, as long as `vec0` KNN
//! with metadata filters still works.

use crate::error::AppError;
use rusqlite::{params, Connection};
use std::sync::Once;

const DIM: usize = 384;

pub fn install() -> Result<(), AppError> {
    static ONCE: Once = Once::new();
    let mut error = None;
    ONCE.call_once(|| {
        let result = unsafe {
            rusqlite::auto_extension::register_auto_extension(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut std::os::raw::c_char,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> std::os::raw::c_int,
            >(
                sqlite_vec::sqlite3_vec_init as *const (),
            ))
        };
        if let Err(err) = result {
            error = Some(err.to_string());
        }
    });
    if let Some(err) = error {
        return Err(AppError::new("sqlite_vec", err));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct VideoVec {
    pub rowid: i64,
    pub vid: String,
    pub source: String,
    pub shelf_life: String,
    pub area_agentic: i32,
    pub area_engineering: i32,
    pub area_research: i32,
    pub blocked: i32,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct ChunkVec {
    pub rowid: i64,
    pub vid: String,
    pub start_s: i64,
    pub source: String,
    pub area_agentic: i32,
    pub area_engineering: i32,
    pub area_research: i32,
    pub blocked: i32,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct KnnFilter {
    pub source: Option<String>,
    pub area: Option<String>,
    pub hide_news: bool,
    pub hide_blocked: bool,
    pub exclude_vid: Option<String>,
}

#[derive(Clone, Debug)]
pub struct VideoHit {
    pub vid: String,
    pub distance: f64,
}

#[derive(Clone, Debug)]
pub struct ChunkHit {
    pub rowid: i64,
    pub vid: String,
    pub start_s: i64,
    pub distance: f64,
}

pub trait VectorIndex {
    fn ensure_schema(&self) -> Result<(), AppError>;
    fn upsert_video(&self, row: &VideoVec) -> Result<(), AppError>;
    fn upsert_chunk(&self, row: &ChunkVec) -> Result<(), AppError>;
    fn knn_videos(
        &self,
        query: &[f32],
        k: usize,
        filter: &KnnFilter,
    ) -> Result<Vec<VideoHit>, AppError>;
    fn knn_chunks(
        &self,
        query: &[f32],
        k: usize,
        filter: &KnnFilter,
    ) -> Result<Vec<ChunkHit>, AppError>;
    fn set_blocked(&self, vids: &[String]) -> Result<(), AppError>;
    fn set_areas(
        &self,
        vid: &str,
        agentic: i32,
        engineering: i32,
        research: i32,
    ) -> Result<(), AppError>;
}

pub struct SqliteVec<'a> {
    pub conn: &'a Connection,
}

impl VectorIndex for SqliteVec<'_> {
    fn ensure_schema(&self) -> Result<(), AppError> {
        self.conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_videos USING vec0(
                embedding float[384] distance_metric=cosine,
                vid text,
                source text,
                shelf_life text,
                area_agentic integer,
                area_engineering integer,
                area_research integer,
                blocked integer
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                embedding float[384] distance_metric=cosine,
                vid text,
                start_s integer,
                source text,
                area_agentic integer,
                area_engineering integer,
                area_research integer,
                blocked integer
            );",
        )?;
        Ok(())
    }

    fn upsert_video(&self, row: &VideoVec) -> Result<(), AppError> {
        let bytes = f32_bytes(&row.embedding);
        self.conn.execute(
            "DELETE FROM vec_videos WHERE rowid = ?1",
            params![row.rowid],
        )?;
        // A shared YouTube id can have been inserted under a different rowid.
        let _ = self
            .conn
            .execute("DELETE FROM vec_videos WHERE vid = ?1", params![row.vid]);
        self.conn.execute(
            "INSERT INTO vec_videos(rowid, embedding, vid, source, shelf_life, area_agentic, area_engineering, area_research, blocked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.rowid,
                bytes,
                row.vid,
                row.source,
                row.shelf_life,
                row.area_agentic,
                row.area_engineering,
                row.area_research,
                row.blocked
            ],
        )?;
        Ok(())
    }

    fn upsert_chunk(&self, row: &ChunkVec) -> Result<(), AppError> {
        let bytes = f32_bytes(&row.embedding);
        self.conn.execute(
            "DELETE FROM vec_chunks WHERE rowid = ?1",
            params![row.rowid],
        )?;
        self.conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding, vid, start_s, source, area_agentic, area_engineering, area_research, blocked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.rowid,
                bytes,
                row.vid,
                row.start_s,
                row.source,
                row.area_agentic,
                row.area_engineering,
                row.area_research,
                row.blocked
            ],
        )?;
        Ok(())
    }

    fn knn_videos(
        &self,
        query: &[f32],
        k: usize,
        filter: &KnnFilter,
    ) -> Result<Vec<VideoHit>, AppError> {
        if query.len() != DIM || k == 0 {
            return Ok(Vec::new());
        }
        let bytes = f32_bytes(query);
        let (sql, extra) = video_sql(k, filter);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            std::iter::once(rusqlite::types::Value::Blob(bytes))
                .chain(extra.into_iter().map(rusqlite::types::Value::Text)),
        ))?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            hits.push(VideoHit {
                vid: row.get(0)?,
                distance: row.get(1)?,
            });
        }
        Ok(hits)
    }

    fn knn_chunks(
        &self,
        query: &[f32],
        k: usize,
        filter: &KnnFilter,
    ) -> Result<Vec<ChunkHit>, AppError> {
        if query.len() != DIM || k == 0 {
            return Ok(Vec::new());
        }
        let bytes = f32_bytes(query);
        let (sql, extra) = chunk_sql(k, filter);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            std::iter::once(rusqlite::types::Value::Blob(bytes))
                .chain(extra.into_iter().map(rusqlite::types::Value::Text)),
        ))?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            hits.push(ChunkHit {
                rowid: row.get(0)?,
                vid: row.get(1)?,
                start_s: row.get(2)?,
                distance: row.get(3)?,
            });
        }
        Ok(hits)
    }

    fn set_blocked(&self, vids: &[String]) -> Result<(), AppError> {
        self.conn.execute("UPDATE vec_videos SET blocked = 0", [])?;
        self.conn.execute("UPDATE vec_chunks SET blocked = 0", [])?;
        for vid in vids {
            self.conn.execute(
                "UPDATE vec_videos SET blocked = 1 WHERE vid = ?1",
                params![vid],
            )?;
            self.conn.execute(
                "UPDATE vec_chunks SET blocked = 1 WHERE vid = ?1",
                params![vid],
            )?;
        }
        Ok(())
    }

    fn set_areas(
        &self,
        vid: &str,
        agentic: i32,
        engineering: i32,
        research: i32,
    ) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE vec_videos SET area_agentic = ?1, area_engineering = ?2, area_research = ?3 WHERE vid = ?4",
            params![agentic, engineering, research, vid],
        )?;
        self.conn.execute(
            "UPDATE vec_chunks SET area_agentic = ?1, area_engineering = ?2, area_research = ?3 WHERE vid = ?4",
            params![agentic, engineering, research, vid],
        )?;
        Ok(())
    }
}

fn video_sql(k: usize, filter: &KnnFilter) -> (String, Vec<String>) {
    let mut sql =
        format!("SELECT vid, distance FROM vec_videos WHERE embedding MATCH ?1 AND k = {k}");
    let mut extra = Vec::new();
    push_filters(&mut sql, &mut extra, filter);
    (sql, extra)
}

fn chunk_sql(k: usize, filter: &KnnFilter) -> (String, Vec<String>) {
    let mut sql = format!(
        "SELECT rowid, vid, start_s, distance FROM vec_chunks WHERE embedding MATCH ?1 AND k = {k}"
    );
    let mut extra = Vec::new();
    push_filters(&mut sql, &mut extra, filter);
    (sql, extra)
}

fn push_filters(sql: &mut String, extra: &mut Vec<String>, filter: &KnnFilter) {
    if filter.hide_blocked {
        sql.push_str(" AND blocked = 0");
    }
    if filter.hide_news {
        sql.push_str(" AND shelf_life != 'news'");
    }
    if let Some(source) = &filter.source {
        extra.push(source.clone());
        sql.push_str(&format!(" AND source = ?{}", extra.len() + 1));
    }
    if let Some(area) = &filter.area {
        let column = match area.as_str() {
            "agentic-engineering" => "area_agentic",
            "ai-engineering" => "area_engineering",
            "ai-research" => "area_research",
            _ => "",
        };
        if !column.is_empty() {
            sql.push_str(&format!(" AND {column} >= 2"));
        }
    }
    if let Some(vid) = &filter.exclude_vid {
        extra.push(vid.clone());
        sql.push_str(&format!(" AND vid != ?{}", extra.len() + 1));
    }
}

pub fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

pub fn normalize(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for value in values.iter_mut() {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(index: usize) -> Vec<f32> {
        let mut values = vec![0.0; DIM];
        values[index] = 1.0;
        values
    }

    #[test]
    fn knn_filters_blocked_and_area_in_sql() {
        install().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let index = SqliteVec { conn: &conn };
        index.ensure_schema().unwrap();
        let on_topic = unit(0);
        let off_topic = unit(1);
        let mut blocked = unit(0);
        blocked[2] = 0.2;
        normalize(&mut blocked);
        index
            .upsert_video(&VideoVec {
                rowid: 1,
                vid: "on-topic-vid".into(),
                source: "aietalks".into(),
                shelf_life: "evergreen".into(),
                area_agentic: 3,
                area_engineering: 0,
                area_research: 0,
                blocked: 0,
                embedding: on_topic,
            })
            .unwrap();
        index
            .upsert_video(&VideoVec {
                rowid: 2,
                vid: "other-area-vid".into(),
                source: "hackertube".into(),
                shelf_life: "evergreen".into(),
                area_agentic: 0,
                area_engineering: 3,
                area_research: 0,
                blocked: 0,
                embedding: off_topic,
            })
            .unwrap();
        index
            .upsert_video(&VideoVec {
                rowid: 3,
                vid: "seen-vidxxxxx".into(),
                source: "aietalks".into(),
                shelf_life: "evergreen".into(),
                area_agentic: 3,
                area_engineering: 0,
                area_research: 0,
                blocked: 1,
                embedding: blocked,
            })
            .unwrap();
        let hits = index
            .knn_videos(
                &unit(0),
                5,
                &KnnFilter {
                    area: Some("agentic-engineering".into()),
                    hide_blocked: true,
                    hide_news: true,
                    ..KnnFilter::default()
                },
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].vid, "on-topic-vid");
    }
}
