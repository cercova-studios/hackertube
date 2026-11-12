//! Catalog sources. Register a new one in [`ingest_source`].

use crate::areas::{self, Signals};
use crate::db::{self, Catalog, Video};
use crate::embed::EmbedStatus;
use crate::error::AppError;
use crate::insights::{self, Segment};
use crate::paths::now_iso;
use crate::vecstore::{ChunkVec, SqliteVec, VectorIndex, VideoVec};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const DATASET_URL: &str = "https://hackertube.org/download/hackertube-data.zip";
pub const TRANSCRIPTS_URL: &str = "https://ai.engineer/api/data/transcripts?format=json";
pub const TALKS_URL: &str = "https://ai.engineer/api/data/talks?format=json";
const ALLOWED_HOSTS: &[&str] = &[
    "ai.engineer",
    "www.ai.engineer",
    "aietalks.com",
    "www.aietalks.com",
    "hackertube.org",
    "www.hackertube.org",
];

#[derive(Clone, Debug)]
pub struct SourceResult {
    pub source: String,
    pub videos: i64,
    pub embedded: i64,
    pub changed: bool,
    pub detail: String,
}

pub struct IngestContext<'a> {
    pub catalog: &'a Catalog,
    pub home: &'a Path,
    pub path: Option<PathBuf>,
    pub limit: Option<usize>,
    pub refresh: bool,
    pub embedder: &'a EmbedStatus,
}

pub fn ingest_source(ctx: &IngestContext<'_>, name: &str) -> Result<SourceResult, AppError> {
    match name {
        "hackertube" => ingest_hackertube(ctx),
        "aietalks" => ingest_aietalks(ctx),
        other => Err(AppError::new(
            "unknown_source",
            format!("unknown source {other}"),
        )),
    }
}

pub fn ingest_hackertube(ctx: &IngestContext<'_>) -> Result<SourceResult, AppError> {
    let root = resolve_dataset(ctx)?;
    let videos_path = root.join("videos.jsonl");
    let digest = fingerprint(&root);
    let embedder_flag = ctx.embedder.model_id();
    let previous = db::meta_get(&ctx.catalog.conn, "hackertube_sha256")?;
    let previous_embedder = db::meta_get(&ctx.catalog.conn, "hackertube_embedder")?;
    let have: i64 = ctx.catalog.conn.query_row(
        "SELECT COUNT(*) FROM videos WHERE source = 'hackertube'",
        [],
        |row| row.get(0),
    )?;
    if previous.as_deref() == Some(digest.as_str())
        && previous_embedder.as_deref() == Some(embedder_flag)
        && have > 0
        && !ctx.refresh
    {
        let embedded: i64 = ctx.catalog.conn.query_row(
            "SELECT COUNT(*) FROM videos WHERE source = 'hackertube' AND embed_row IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        return Ok(SourceResult {
            source: "hackertube".into(),
            videos: have,
            embedded,
            changed: false,
            detail: "catalog unchanged".into(),
        });
    }
    let seeds = seed_vectors(ctx.embedder);
    let ids = read_ids(&root.join("embedding_ids.json"))?;
    let matrix = read_f16_matrix(&root.join("embeddings.npy"), ids.len())?;
    let mut id_to_row = std::collections::HashMap::new();
    for (index, vid) in ids.iter().enumerate() {
        id_to_row.insert(vid.clone(), index);
    }
    let text = fs::read_to_string(&videos_path)?;
    let mut count = 0_i64;
    let mut embedded = 0_i64;
    let index = SqliteVec {
        conn: &ctx.catalog.conn,
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: Value =
            serde_json::from_str(line).map_err(|err| AppError::new("dataset", err.to_string()))?;
        let mut video = video_from_json(&row)?;
        if db::get_video(&ctx.catalog.conn, &video.vid)?
            .is_some_and(|existing| existing.source == "aietalks")
        {
            // Same YouTube id already ingested as a talk, with a transcript.
            // Leave that row in place.
            continue;
        }
        let vector = id_to_row
            .get(&video.vid)
            .and_then(|index| matrix.get(*index))
            .cloned();
        let cosines = vector
            .as_ref()
            .and_then(|vector| cosine_to_seeds(vector, seeds.as_ref()));
        apply_areas(&mut video, cosines);
        if let Some(vector) = vector {
            let rowid = video
                .embed_row
                .unwrap_or(db::next_id(&ctx.catalog.conn, "vec_video_id")?);
            video.embed_row = Some(rowid);
            index.upsert_video(&VideoVec {
                rowid,
                vid: video.vid.clone(),
                source: video.source.clone(),
                shelf_life: video.shelf_life.clone(),
                area_agentic: video.area_agentic,
                area_engineering: video.area_engineering,
                area_research: video.area_research,
                blocked: 0,
                embedding: vector.clone(),
            })?;
            db::store_embedding(&ctx.catalog.conn, &video.vid, &vector)?;
            embedded += 1;
        }
        video.updated_at = now_iso();
        db::upsert_video(&ctx.catalog.conn, &video)?;
        count += 1;
    }
    load_mentions(&ctx.catalog.conn, &root)?;
    load_related(&ctx.catalog.conn, &root)?;
    db::rebuild_video_fts(&ctx.catalog.conn)?;
    db::meta_set(&ctx.catalog.conn, "hackertube_sha256", &digest)?;
    db::meta_set(&ctx.catalog.conn, "hackertube_embedder", embedder_flag)?;
    Ok(SourceResult {
        source: "hackertube".into(),
        videos: count,
        embedded,
        changed: true,
        detail: format!(
            "loaded {count} videos from {}",
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("dataset")
        ),
    })
}

fn video_from_json(row: &Value) -> Result<Video, AppError> {
    let vid = row
        .get("vid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if vid.len() != 11 {
        return Err(AppError::new("dataset", format!("bad vid {vid}")));
    }
    let categories = row
        .get("categories")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let tags = string_list(row.get("tags"));
    let quote = row
        .get("quote")
        .and_then(|quote| quote.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let recommenders = row
        .get("hn")
        .and_then(|hn| hn.get("recommenders"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok(Video {
        vid: vid.clone(),
        source: "hackertube".into(),
        url: row
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        page_url: "https://hackertube.org".into(),
        title: row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        channel: row
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        summary: row
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        why: row
            .get("why_hn")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        content_type: row
            .get("content_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        level: row
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        shelf_life: row
            .get("shelf_life")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        primary_topic: row
            .get("primary_topic")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tags,
        categories,
        topic_slugs: Vec::new(),
        quote,
        hn_recommenders: recommenders,
        score: row.get("score").and_then(Value::as_f64).unwrap_or(0.0),
        published: String::new(),
        event: String::new(),
        transcript_path: String::new(),
        embed_row: None,
        area_agentic: 0,
        area_engineering: 0,
        area_research: 0,
        cos_agentic: None,
        cos_engineering: None,
        cos_research: None,
        updated_at: String::new(),
    })
}

fn apply_areas(video: &mut Video, cosines: Option<[f32; 3]>) {
    let signals = Signals {
        title: &video.title,
        summary: &video.summary,
        why: &video.why,
        topic: &video.primary_topic,
        tags: &video.tags.join(" "),
        topic_slugs: &video.topic_slugs,
        categories: &video.categories,
        content_type: &video.content_type,
        cosines,
    };
    let affinity = areas::classify(&signals);
    video.area_agentic = affinity.agentic;
    video.area_engineering = affinity.engineering;
    video.area_research = affinity.research;
    if let Some(cosines) = cosines {
        video.cos_agentic = Some(f64::from(cosines[0]));
        video.cos_engineering = Some(f64::from(cosines[1]));
        video.cos_research = Some(f64::from(cosines[2]));
    }
}

pub const AREAS_REV: &str = "4";

/// Recompute focus areas when the classifier or embedding model changed.
/// Seed vectors come from the active embedder. With the model off, only
/// keyword rules run.
pub fn reclassify_if_stale(catalog: &Catalog, home: &Path) -> Result<(), AppError> {
    if let Some(id) = crate::embed::EmbedStatus::recorded_model_id(home) {
        let want = format!("{AREAS_REV}:{id}");
        if db::meta_get(&catalog.conn, "areas_version")?.as_deref() == Some(want.as_str()) {
            return Ok(());
        }
    }
    let embedder = EmbedStatus::load(home);
    let want = format!("{AREAS_REV}:{}", embedder.model_id());
    if db::meta_get(&catalog.conn, "areas_version")?.as_deref() == Some(want.as_str()) {
        return Ok(());
    }
    reclassify(catalog, &embedder)?;
    db::meta_set(&catalog.conn, "areas_version", &want)?;
    Ok(())
}

fn reclassify(catalog: &Catalog, embedder: &EmbedStatus) -> Result<(), AppError> {
    let seeds = seed_vectors(embedder);
    let index = SqliteVec {
        conn: &catalog.conn,
    };
    let mut stmt = catalog.conn.prepare(
        "SELECT vid, title, summary, why, primary_topic, tags_json, categories_json, topics_json, content_type
         FROM videos",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    catalog.conn.execute_batch("BEGIN")?;
    for (vid, title, summary, why, topic, tags_json, categories_json, topics_json, content_type) in
        rows
    {
        let tags = json_string_list(&tags_json);
        let categories = json_string_list(&categories_json);
        let topic_slugs = json_string_list(&topics_json);
        let vector = db::load_embedding(&catalog.conn, &vid)?;
        let cosines = vector
            .as_ref()
            .and_then(|vector| cosine_to_seeds(vector, seeds.as_ref()));
        let affinity = areas::classify(&Signals {
            title: &title,
            summary: &summary,
            why: &why,
            topic: &topic,
            tags: &tags.join(" "),
            topic_slugs: &topic_slugs,
            categories: &categories,
            content_type: &content_type,
            cosines,
        });
        let (cos_agentic, cos_engineering, cos_research) = match cosines {
            Some(values) => (
                Some(f64::from(values[0])),
                Some(f64::from(values[1])),
                Some(f64::from(values[2])),
            ),
            None => (None, None, None),
        };
        catalog.conn.execute(
            "UPDATE videos SET area_agentic = ?1, area_engineering = ?2, area_research = ?3,
             cos_agentic = ?4, cos_engineering = ?5, cos_research = ?6 WHERE vid = ?7",
            rusqlite::params![
                affinity.agentic,
                affinity.engineering,
                affinity.research,
                cos_agentic,
                cos_engineering,
                cos_research,
                vid
            ],
        )?;
        index.set_areas(
            &vid,
            affinity.agentic,
            affinity.engineering,
            affinity.research,
        )?;
    }
    catalog.conn.execute_batch("COMMIT")?;
    Ok(())
}

fn json_string_list(text: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(text).unwrap_or_default()
}

fn seed_vectors(embedder: &EmbedStatus) -> Option<[Vec<f32>; 3]> {
    let texts = areas::SEEDS
        .iter()
        .map(|text| (*text).to_string())
        .collect::<Vec<_>>();
    embedder.embed(&texts).ok().and_then(|vectors| {
        if vectors.len() == 3 {
            Some([vectors[0].clone(), vectors[1].clone(), vectors[2].clone()])
        } else {
            None
        }
    })
}

fn cosine_to_seeds(vector: &[f32], seeds: Option<&[Vec<f32>; 3]>) -> Option<[f32; 3]> {
    let seeds = seeds?;
    Some([
        areas::cosine(vector, &seeds[0]),
        areas::cosine(vector, &seeds[1]),
        areas::cosine(vector, &seeds[2]),
    ])
}

pub fn ingest_aietalks(ctx: &IngestContext<'_>) -> Result<SourceResult, AppError> {
    let (transcripts, talks) = load_indexes(ctx)?;
    let mut by_slug = std::collections::HashMap::new();
    for talk in &talks {
        by_slug.insert(talk.slug.clone(), talk);
    }
    let mut selected: Vec<(TranscriptRow, Talk)> = Vec::new();
    for row in &transcripts {
        let Some(talk) = by_slug.get(&row.talk_slug) else {
            continue;
        };
        if !is_vid(&talk.video_id) {
            continue;
        }
        selected.push((row.clone(), (*talk).clone()));
        if let Some(limit) = ctx.limit {
            if limit > 0 && selected.len() >= limit {
                break;
            }
        }
    }
    if selected.is_empty() && !transcripts.is_empty() {
        return Err(AppError::new(
            "aietalks_empty",
            "transcript index did not match any talk with a video id",
        ));
    }
    let digest = selection_digest(&selected);
    let previous = db::meta_get(&ctx.catalog.conn, "aietalks_sha256")?;
    let previous_embedder = db::meta_get(&ctx.catalog.conn, "aietalks_embedder")?;
    if previous.as_deref() == Some(digest.as_str())
        && previous_embedder.as_deref() == Some(ctx.embedder.model_id())
        && !ctx.refresh
    {
        let mut missing = Vec::new();
        for item in &selected {
            let owned: i64 = ctx.catalog.conn.query_row(
                "SELECT COUNT(*) FROM videos WHERE vid = ?1 AND source = 'aietalks'",
                [&item.1.video_id],
                |row| row.get(0),
            )?;
            if owned == 0 {
                missing.push(item.clone());
            }
        }
        if missing.is_empty() {
            let count: i64 = ctx.catalog.conn.query_row(
                "SELECT COUNT(*) FROM videos WHERE source = 'aietalks'",
                [],
                |row| row.get(0),
            )?;
            return Ok(SourceResult {
                source: "aietalks".into(),
                videos: count,
                embedded: count,
                changed: false,
                detail: "catalog unchanged".into(),
            });
        }
        // A later dataset refresh can overwrite a shared YouTube id. Put the
        // talk row back without re-embedding the rest of the catalog.
        selected = missing;
    }
    if ctx.path.is_none() {
        download_markdowns(ctx, &selected)?;
    }
    let seeds = seed_vectors(ctx.embedder);
    let index = SqliteVec {
        conn: &ctx.catalog.conn,
    };
    let mut records = 0_i64;
    let mut embedded = 0_i64;
    let mut skipped = 0_i64;
    let mut pending_chunks: Vec<(i64, Vec<f32>, ChunkVec)> = Vec::new();
    for (row, talk) in &selected {
        match materialize(ctx, row, talk, seeds.as_ref(), &index, &mut pending_chunks) {
            Ok(did_embed) => {
                records += 1;
                if did_embed {
                    embedded += 1;
                }
                if records % 100 == 0 {
                    eprintln!("aietalks {records} embedded {embedded}");
                }
            }
            Err(_) => skipped += 1,
        }
    }
    flush_chunk_vectors(&index, &mut pending_chunks)?;
    db::rebuild_video_fts(&ctx.catalog.conn)?;
    db::meta_set(&ctx.catalog.conn, "aietalks_sha256", &digest)?;
    db::meta_set(
        &ctx.catalog.conn,
        "aietalks_embedder",
        ctx.embedder.model_id(),
    )?;
    let stored: i64 = ctx.catalog.conn.query_row(
        "SELECT COUNT(*) FROM videos WHERE source = 'aietalks'",
        [],
        |row| row.get(0),
    )?;
    let stored_embedded: i64 = ctx.catalog.conn.query_row(
        "SELECT COUNT(*) FROM videos WHERE source = 'aietalks' AND embed_row IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    records = stored;
    embedded = stored_embedded;
    let mut detail = format!("ingested {records} of {} talks", transcripts.len());
    if transcripts.len() > records as usize {
        detail.push_str("; pass --limit 0 for the full index when a limit was set");
    }
    if skipped > 0 {
        detail.push_str(&format!("; skipped {skipped}"));
    }
    if ctx.embedder.notice().is_some() {
        detail.push_str("; embeddings deferred (model unavailable)");
    }
    Ok(SourceResult {
        source: "aietalks".into(),
        videos: records,
        embedded,
        changed: true,
        detail,
    })
}

fn materialize(
    ctx: &IngestContext<'_>,
    row: &TranscriptRow,
    talk: &Talk,
    seeds: Option<&[Vec<f32>; 3]>,
    index: &SqliteVec<'_>,
    pending: &mut Vec<(i64, Vec<f32>, ChunkVec)>,
) -> Result<bool, AppError> {
    let markdown = markdown_for(ctx, &talk.video_id, &talk.slug, &talk.url)?;
    let (found, published) = insights::identity(&markdown).ok_or_else(|| {
        AppError::new(
            "transcript_identity",
            format!("{} has no video id", talk.slug),
        )
    })?;
    if found != talk.video_id {
        return Err(AppError::new(
            "transcript_identity",
            format!("{} markdown video {found} != {}", talk.slug, talk.video_id),
        ));
    }
    let topics = talk.topics.clone().unwrap_or_default();
    let slugs: Vec<String> = topics.iter().map(|topic| topic.slug.clone()).collect();
    let names: Vec<String> = topics.iter().map(|topic| topic.name.clone()).collect();
    let speakers = talk
        .speakers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|speaker| speaker.name)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let summary = talk.summary.clone().unwrap_or_default();
    let why = first_sentence(&summary).unwrap_or_else(|| {
        if names.is_empty() {
            "AI Engineer talk".into()
        } else {
            format!("AI Engineer talk on {}", names[0])
        }
    });
    let mut video = Video {
        vid: talk.video_id.clone(),
        source: "aietalks".into(),
        url: format!("https://www.youtube.com/watch?v={}", talk.video_id),
        page_url: talk.url.clone(),
        title: if talk.title.is_empty() {
            row.title.clone()
        } else {
            talk.title.clone()
        },
        channel: if speakers.is_empty() {
            "AI Engineer".into()
        } else {
            speakers
        },
        summary: summary.clone(),
        why,
        content_type: "talk".into(),
        level: "intermediate".into(),
        shelf_life: "evergreen".into(),
        primary_topic: names.first().cloned().unwrap_or_default(),
        tags: names,
        categories: Vec::new(),
        topic_slugs: slugs,
        quote: String::new(),
        hn_recommenders: 0,
        score: aie_score(&topics),
        published,
        event: talk.event.clone().unwrap_or_default(),
        transcript_path: format!("transcripts/{}.md", talk.video_id),
        embed_row: None,
        area_agentic: 0,
        area_engineering: 0,
        area_research: 0,
        cos_agentic: None,
        cos_engineering: None,
        cos_research: None,
        updated_at: now_iso(),
    };
    let embed_text = format!(
        "{}\n{}\n{}\n{}\n{}",
        video.title,
        video.channel,
        video.tags.join(" "),
        video.summary,
        video.why
    );
    let existing = db::get_video(&ctx.catalog.conn, &video.vid)?;
    if let Some(existing) = &existing {
        if existing.hn_recommenders > video.hn_recommenders {
            video.hn_recommenders = existing.hn_recommenders;
        }
        if existing.score > video.score {
            video.score = existing.score;
        }
    }
    let mut did_embed = false;
    if let Ok(vectors) = ctx.embedder.embed(&[embed_text]) {
        if let Some(vector) = vectors.into_iter().next() {
            let cosines = cosine_to_seeds(&vector, seeds);
            apply_areas(&mut video, cosines);
            let rowid = existing
                .as_ref()
                .and_then(|row| row.embed_row)
                .unwrap_or(db::next_id(&ctx.catalog.conn, "vec_video_id")?);
            video.embed_row = Some(rowid);
            index.upsert_video(&VideoVec {
                rowid,
                vid: video.vid.clone(),
                source: video.source.clone(),
                shelf_life: video.shelf_life.clone(),
                area_agentic: video.area_agentic,
                area_engineering: video.area_engineering,
                area_research: video.area_research,
                blocked: 0,
                embedding: vector.clone(),
            })?;
            db::store_embedding(&ctx.catalog.conn, &video.vid, &vector)?;
            did_embed = true;
            embed_chunks(ctx, &video, &markdown, index, pending)?;
        }
    } else {
        apply_areas(&mut video, None);
        let segments = insights::parse_markdown(&markdown, &video.vid);
        let grouped = insights::chunks_from_segments(&segments);
        let rows: Vec<_> = grouped
            .iter()
            .enumerate()
            .map(|(idx, segment)| {
                (
                    idx as i64,
                    segment.start_s,
                    segment.text.clone(),
                    segment.url.clone(),
                )
            })
            .collect();
        db::replace_chunks(&ctx.catalog.conn, &video.vid, &rows)?;
    }
    let relative = write_transcript(ctx.home, &video.vid, &markdown)?;
    video.transcript_path = relative;
    db::upsert_video(&ctx.catalog.conn, &video)?;
    Ok(did_embed)
}

fn embed_chunks(
    ctx: &IngestContext<'_>,
    video: &Video,
    markdown: &str,
    index: &SqliteVec<'_>,
    pending: &mut Vec<(i64, Vec<f32>, ChunkVec)>,
) -> Result<(), AppError> {
    let segments = insights::parse_markdown(markdown, &video.vid);
    let grouped = if segments.is_empty() {
        vec![Segment {
            start_s: 0,
            url: video.url.clone(),
            text: video.summary.clone(),
            heading: String::new(),
            section: "summary".into(),
        }]
    } else {
        insights::chunks_from_segments(&segments)
    };
    let rows: Vec<_> = grouped
        .iter()
        .enumerate()
        .map(|(idx, segment)| {
            (
                idx as i64,
                segment.start_s,
                segment.text.clone(),
                segment.url.clone(),
            )
        })
        .collect();
    let ids = db::replace_chunks(&ctx.catalog.conn, &video.vid, &rows)?;
    let texts: Vec<String> = rows.iter().map(|(_, _, text, _)| text.clone()).collect();
    let vectors = ctx.embedder.embed(&texts).unwrap_or_default();
    for ((id, (idx, start_s, _, _)), vector) in ids.iter().zip(rows.iter()).zip(vectors) {
        let _ = idx;
        pending.push((
            *id,
            vector.clone(),
            ChunkVec {
                rowid: *id,
                vid: video.vid.clone(),
                start_s: *start_s,
                source: video.source.clone(),
                area_agentic: video.area_agentic,
                area_engineering: video.area_engineering,
                area_research: video.area_research,
                blocked: 0,
                embedding: vector,
            },
        ));
        if pending.len() >= 64 {
            flush_chunk_vectors(index, pending)?;
        }
    }
    Ok(())
}

fn flush_chunk_vectors(
    index: &SqliteVec<'_>,
    pending: &mut Vec<(i64, Vec<f32>, ChunkVec)>,
) -> Result<(), AppError> {
    for (_, _, row) in pending.drain(..) {
        index.upsert_chunk(&row)?;
    }
    Ok(())
}

fn aie_score(topics: &[Topic]) -> f64 {
    let slugs: Vec<&str> = topics.iter().map(|topic| topic.slug.as_str()).collect();
    let mut score: f64 = 4.0;
    if slugs.iter().any(|slug| {
        areas::SEEDS[0].contains("agent")
            && matches!(
                *slug,
                "coding-agents"
                    | "agent-engineering"
                    | "model-context-protocol"
                    | "harness-engineering"
                    | "context-engineering"
                    | "agent-skills"
                    | "multi-agent-systems"
            )
    }) {
        score += 1.5;
    }
    if slugs.iter().any(|slug| {
        matches!(
            *slug,
            "harness-engineering"
                | "coding-agents"
                | "model-context-protocol"
                | "agent-skills"
                | "multi-agent-systems"
                | "agent-orchestration"
        )
    }) {
        score += 1.5;
    }
    score.min(9.0)
}

fn first_sentence(text: &str) -> Option<String> {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return None;
    }
    for mark in [". ", "? "] {
        if let Some((head, _)) = cleaned.split_once(mark) {
            return Some(format!("{head}{}", mark.trim()));
        }
    }
    Some(cleaned)
}

fn write_transcript(home: &Path, vid: &str, markdown: &str) -> Result<String, AppError> {
    let dir = home.join("transcripts");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{vid}.md")), markdown)?;
    Ok(format!("transcripts/{vid}.md"))
}

fn markdown_for(
    ctx: &IngestContext<'_>,
    vid: &str,
    slug: &str,
    page: &str,
) -> Result<String, AppError> {
    if let Some(root) = &ctx.path {
        for candidate in [
            root.join("markdown").join(format!("{vid}.md")),
            root.join("markdown").join(format!("{slug}.md")),
            root.join(format!("{vid}.md")),
            root.join(format!("{slug}.md")),
        ] {
            if candidate.exists() {
                return Ok(fs::read_to_string(candidate)?);
            }
        }
        return Err(AppError::new(
            "aietalks_missing",
            format!("no markdown for {vid} under {}", root.display()),
        ));
    }
    let cache = ctx
        .home
        .join("cache")
        .join("aietalks")
        .join(format!("{vid}.md"));
    if cache.exists() && !ctx.refresh {
        return Ok(fs::read_to_string(cache)?);
    }
    if page.is_empty() {
        return Err(AppError::new(
            "aietalks_url",
            format!("{slug} has no talk url"),
        ));
    }
    let url = if page.ends_with(".md") {
        page.to_string()
    } else {
        format!("{page}.md")
    };
    let text = fetch_text(&url, 2_000_000)?;
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cache, &text)?;
    Ok(text)
}

fn download_markdowns(
    ctx: &IngestContext<'_>,
    selected: &[(TranscriptRow, Talk)],
) -> Result<(), AppError> {
    let jobs: Vec<(String, PathBuf)> = selected
        .iter()
        .filter_map(|(_, talk)| {
            let cache = ctx
                .home
                .join("cache")
                .join("aietalks")
                .join(format!("{}.md", talk.video_id));
            if cache.exists() && !ctx.refresh {
                return None;
            }
            let page = &talk.url;
            if page.is_empty() {
                return None;
            }
            let url = if page.ends_with(".md") {
                page.clone()
            } else {
                format!("{page}.md")
            };
            Some((url, cache))
        })
        .collect();
    if jobs.is_empty() {
        return Ok(());
    }
    if let Some(parent) = jobs[0].1.parent() {
        fs::create_dir_all(parent)?;
    }
    let queue = Mutex::new(jobs.into_iter());
    let errors = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(40))
                    .build();
                loop {
                    let next = queue.lock().expect("queue").next();
                    let Some((url, path)) = next else { break };
                    if let Err(err) = fetch_to(&agent, &url, &path, 2_000_000) {
                        errors.lock().expect("errors").push(err);
                    }
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap_or_default();
    if !errors.is_empty() && errors.len() == selected.len() {
        return Err(errors.into_iter().next().unwrap());
    }
    Ok(())
}

fn load_indexes(ctx: &IngestContext<'_>) -> Result<(Vec<TranscriptRow>, Vec<Talk>), AppError> {
    if let Some(root) = &ctx.path {
        let transcripts = read_json_file(&root.join("transcripts.json"))?;
        let talks = read_json_file(&root.join("talks.json"))?;
        return Ok((transcripts, talks));
    }
    let cache = ctx.home.join("cache").join("aietalks");
    fs::create_dir_all(&cache)?;
    let transcripts = cached_json(
        &cache.join("transcripts.json"),
        TRANSCRIPTS_URL,
        ctx.refresh,
        8_000_000,
    )?;
    let talks = cached_json(
        &cache.join("talks.json"),
        TALKS_URL,
        ctx.refresh,
        40_000_000,
    )?;
    Ok((transcripts, talks))
}

fn cached_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    url: &str,
    refresh: bool,
    cap: usize,
) -> Result<Vec<T>, AppError> {
    if path.exists() && !refresh {
        return read_json_file(path);
    }
    let text = fetch_text(url, cap)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &text)?;
    serde_json::from_str(&text).map_err(|err| AppError::new("aietalks_schema", err.to_string()))
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AppError> {
    let text = fs::read_to_string(path)
        .map_err(|_| AppError::new("aietalks_missing", format!("missing {}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|err| AppError::new("aietalks_schema", format!("{}: {err}", path.display())))
}

#[derive(Clone, Debug, Deserialize)]
struct TranscriptRow {
    #[serde(rename = "talkSlug")]
    talk_slug: String,
    #[serde(default)]
    title: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Talk {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(rename = "videoId", default)]
    video_id: String,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    speakers: Option<Vec<Speaker>>,
    #[serde(default)]
    topics: Option<Vec<Topic>>,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Speaker {
    #[serde(default)]
    name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Topic {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
}

fn selection_digest(selected: &[(TranscriptRow, Talk)]) -> String {
    let mut digest = Sha256::new();
    let mut rows: Vec<String> = selected
        .iter()
        .map(|(_, talk)| format!("{}|{}", talk.video_id, talk.title))
        .collect();
    rows.sort();
    for row in rows {
        digest.update(row.as_bytes());
        digest.update(b"\n");
    }
    hex::encode(digest.finalize())
}

fn resolve_dataset(ctx: &IngestContext<'_>) -> Result<PathBuf, AppError> {
    if let Some(path) = &ctx.path {
        return dataset_root(path);
    }
    let cache = ctx.home.join("cache");
    fs::create_dir_all(&cache)?;
    let zip_path = cache.join("hackertube-data.zip");
    if ctx.refresh || !zip_path.exists() {
        fetch_to(
            &ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(120))
                .build(),
            DATASET_URL,
            &zip_path,
            200_000_000,
        )?;
    }
    let extracted = cache.join("hackertube-data");
    if ctx.refresh
        || !extracted.join("videos.jsonl").exists()
            && extracted
                .read_dir()
                .ok()
                .and_then(|mut dir| dir.find_map(|entry| entry.ok()))
                .is_none()
    {
        safe_extract(&zip_path, &extracted)?;
    }
    dataset_root(&extracted)
}

fn dataset_root(path: &Path) -> Result<PathBuf, AppError> {
    let mut path = path.to_path_buf();
    if path.is_file() && path.extension().is_some_and(|ext| ext == "zip") {
        let dest = path.with_extension("extracted");
        safe_extract(&path, &dest)?;
        path = dest;
    }
    if !path.is_dir() {
        return Err(AppError::new(
            "dataset_path",
            format!("{} is not a directory or zip", path.display()),
        ));
    }
    if path.join("videos.jsonl").exists() {
        return Ok(path);
    }
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() && child.join("videos.jsonl").exists() {
                return Ok(child);
            }
        }
    }
    Err(AppError::new(
        "dataset_path",
        format!("no videos.jsonl under {}", path.display()),
    ))
}

pub fn safe_extract(zip_path: &Path, dest: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dest)?;
    let file = File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| AppError::new("unsafe_zip", err.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| AppError::new("unsafe_zip", err.to_string()))?;
        let name = entry.name().to_string();
        if name.starts_with('/') || name.split(['/', '\\']).any(|part| part == "..") {
            return Err(AppError::new(
                "unsafe_zip",
                format!("refusing zip member {name}"),
            ));
        }
        let out = dest.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&out)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    Ok(())
}

fn fetch_text(url: &str, cap: usize) -> Result<String, AppError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let path = std::env::temp_dir().join(format!(
        "hackertube-{}",
        hex::encode(Sha256::digest(url.as_bytes()))
    ));
    fetch_to(&agent, url, &path, cap)?;
    let text = fs::read_to_string(&path)?;
    let _ = fs::remove_file(path);
    Ok(text)
}

fn fetch_to(agent: &ureq::Agent, url: &str, dest: &Path, cap: usize) -> Result<(), AppError> {
    url::parse_allowed(url)?;
    let response = agent
        .get(url)
        .set("User-Agent", "hackertube/0.2")
        .set("Accept", "text/markdown, application/json")
        .call()
        .map_err(|err| AppError::new("download_failed", format!("{url} failed: {err}")))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > cap {
            return Err(AppError::new(
                "download_too_large",
                format!("{url} exceeded {cap} bytes"),
            ));
        }
        bytes.extend_from_slice(&buf[..read]);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = dest.with_extension("partial");
    File::create(&partial)?.write_all(&bytes)?;
    fs::rename(partial, dest)?;
    Ok(())
}

mod url {
    use super::{AppError, ALLOWED_HOSTS};
    pub fn parse_allowed(url: &str) -> Result<(), AppError> {
        let Some(rest) = url.strip_prefix("https://") else {
            return Err(AppError::new("disallowed_url", format!("refusing {url}")));
        };
        let host = rest.split(['/', ':', '?']).next().unwrap_or("");
        if !ALLOWED_HOSTS.contains(&host) {
            return Err(AppError::new("disallowed_url", format!("refusing {url}")));
        }
        Ok(())
    }
}

fn fingerprint(root: &Path) -> String {
    let mut digest = Sha256::new();
    for name in ["videos.jsonl", "embeddings.npy", "embedding_ids.json"] {
        let path = root.join(name);
        digest.update(name.as_bytes());
        if let Ok(bytes) = fs::read(path) {
            digest.update(&bytes);
        }
    }
    hex::encode(digest.finalize())
}

fn read_ids(path: &Path) -> Result<Vec<String>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|err| AppError::new("bad_embeddings", err.to_string()))
}

fn read_f16_matrix(path: &Path, rows: usize) -> Result<Vec<Vec<f32>>, AppError> {
    if !path.exists() || rows == 0 {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path)?;
    let (header_end, shape_cols) = parse_npy_header(&bytes)?;
    if shape_cols != 384 {
        return Err(AppError::new(
            "bad_embeddings",
            format!("expected 384 columns, got {shape_cols}"),
        ));
    }
    let data = &bytes[header_end..];
    let mut matrix = Vec::with_capacity(rows);
    for row in 0..rows {
        let start = row * 384 * 2;
        let end = start + 384 * 2;
        if end > data.len() {
            break;
        }
        let mut vector = Vec::with_capacity(384);
        for offset in (start..end).step_by(2) {
            let bits = u16::from_le_bytes([data[offset], data[offset + 1]]);
            vector.push(half::f16::from_bits(bits).to_f32());
        }
        crate::vecstore::normalize(&mut vector);
        matrix.push(vector);
    }
    Ok(matrix)
}

fn parse_npy_header(bytes: &[u8]) -> Result<(usize, usize), AppError> {
    if bytes.len() < 10 || &bytes[0..6] != b"\x93NUMPY" {
        return Err(AppError::new("bad_embeddings", "not an npy file"));
    }
    let major = bytes[6];
    let header_len = if major == 1 {
        u16::from_le_bytes([bytes[8], bytes[9]]) as usize
    } else {
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize
    };
    let header_start = if major == 1 { 10 } else { 12 };
    let header = std::str::from_utf8(&bytes[header_start..header_start + header_len])
        .map_err(|err| AppError::new("bad_embeddings", err.to_string()))?;
    let cols = Regex::new(r"\(\s*\d+\s*,\s*(\d+)\s*\)")
        .unwrap()
        .captures(header)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    Ok((header_start + header_len, cols))
}

fn load_mentions(conn: &rusqlite::Connection, root: &Path) -> Result<(), AppError> {
    let path = root.join("mentions.jsonl");
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let item: Value =
            serde_json::from_str(line).map_err(|err| AppError::new("dataset", err.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO mentions(hn_comment_id, vid, hn_url, stance, why, quote, month) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                item.get("hn_comment_id").and_then(Value::as_i64).unwrap_or(0),
                item.get("vid").and_then(Value::as_str).unwrap_or(""),
                item.get("hn_url").and_then(Value::as_str).unwrap_or(""),
                item.get("stance").and_then(Value::as_str).unwrap_or(""),
                item.get("why").and_then(Value::as_str).unwrap_or(""),
                item.get("quote").and_then(Value::as_str).unwrap_or(""),
                item.get("month").and_then(Value::as_str).unwrap_or("")
            ],
        )?;
    }
    Ok(())
}

fn load_related(conn: &rusqlite::Connection, root: &Path) -> Result<(), AppError> {
    let path = root.join("related.jsonl");
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let item: Value =
            serde_json::from_str(line).map_err(|err| AppError::new("dataset", err.to_string()))?;
        let vid = item.get("vid").and_then(Value::as_str).unwrap_or("");
        let Some(related) = item.get("related").and_then(Value::as_array) else {
            continue;
        };
        for other in related {
            let Some(other_vid) = other.get("vid").and_then(Value::as_str) else {
                continue;
            };
            let shared = other
                .get("shared_recommenders")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            conn.execute(
                "INSERT OR REPLACE INTO related(vid, other_vid, shared) VALUES (?1, ?2, ?3)",
                rusqlite::params![vid, other_vid, shared],
            )?;
        }
    }
    Ok(())
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn is_vid(value: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9_-]{11}$").unwrap().is_match(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn zip_slip_is_refused() {
        let dir = std::env::temp_dir().join(format!("ht-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("bad.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../escape.txt", options).unwrap();
            zip.write_all(b"nope").unwrap();
            zip.finish().unwrap();
        }
        let err = safe_extract(&zip_path, &dir.join("out")).unwrap_err();
        assert_eq!(err.code, "unsafe_zip");
        let _ = fs::remove_dir_all(&dir);
    }
}
