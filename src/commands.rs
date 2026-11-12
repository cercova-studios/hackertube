//! Command handlers. Each one returns a document and an exit code.

use crate::areas;
use crate::db::{self, Catalog, Video};
use crate::embed::EmbedStatus;
use crate::error::{AppError, UsageError};
use crate::ingest::{self, IngestContext};
use crate::insights::{self};
use crate::output::{self, Doc, Val};
use crate::paths::{self, display_path};
use crate::rank::{self, RankInput, RankParts};
use crate::state::{self, Mark};
use crate::vecstore::{KnnFilter, SqliteVec, VectorIndex};
use regex::Regex;
use rusqlite::params;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

static VID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{11}$").unwrap());

const VIDEO_FIELDS: &[&str] = &[
    "vid", "title", "channel", "score", "reason", "area", "source", "url", "match", "sim",
    "shared", "level", "topic", "page", "state",
];

#[derive(Clone, Debug)]
pub struct Flags {
    pub json: bool,
    pub full: bool,
    pub fields: Option<Vec<String>>,
}

pub fn home(flags: &Flags) -> Result<(Doc, i32), AppError> {
    let home = paths::data_home();
    let marks = state::load(&home)?;
    let mut doc = Doc::new();
    doc.add("description", "Ranked videos for an AI/ML engineer on agentic engineering, AI engineering, and AI research.");
    doc.add("version", env!("CARGO_PKG_VERSION"));
    doc.add("bin", paths::bin_path());
    doc.add("home", display_path(&home));
    doc.add("catalog", display_path(&home.join("catalog.sqlite")));
    doc.add("state_file", display_path(&state::path_for(&home)));
    let embedder = EmbedStatus::load(&home);
    doc.add("embedder", embedder.label());
    doc.add("model", embedder.model_id());
    if let Some(notice) = embedder.notice() {
        doc.add("notice", notice);
    }
    if !Catalog::exists(&home) {
        doc.add("videos", 0_i64);
        doc.add("state", state::state_line(&marks));
        doc.add("empty", "no catalog yet");
        doc.help(&[
            "hackertube ingest --path <zip-or-dir>",
            "hackertube recommend --area agentic-engineering",
            "hackertube search \"coding agents\"",
        ]);
        output::emit(&doc, flags.json, flags.full);
        return Ok((doc, 0));
    }
    let catalog = Catalog::open(&home)?;
    sync(&catalog, &marks)?;
    ingest::reclassify_if_stale(&catalog, &home)?;
    let videos = db::video_count(&catalog.conn)?;
    let sources = db::source_counts(&catalog.conn)?;
    doc.add("videos", videos);
    doc.add("state", state::state_line(&marks));
    doc.add("sources", source_line(&sources));
    let mut coverage = Vec::new();
    let mut picks: Vec<Vec<(String, Val)>> = Vec::new();
    let mut used = HashSet::new();
    let mut first = String::new();
    for area in areas::AREA_IDS {
        let (rows, matched, _, _) = recommend_rows(&catalog, area, 5, false, None, &marks)?;
        coverage.push(format!("{area} {matched}"));
        if let Some(row) = rows.into_iter().find(|row| used.insert(row.0.clone())) {
            if first.is_empty() {
                first = row.0.clone();
            }
            picks.push(vec![
                ("vid".into(), Val::from(row.0)),
                ("title".into(), Val::from(row.1)),
                ("area".into(), Val::from(area)),
                ("score".into(), Val::from(row.2)),
            ]);
        }
    }
    doc.add("coverage", coverage.join(", "));
    let (fields, projected) = output::project_fields(
        &picks,
        &["vid", "title", "area", "score"],
        flags.fields.as_deref(),
        VIDEO_FIELDS,
    )
    .map_err(usage_as_app)?;
    doc.table(
        "picks",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    if picks.is_empty() {
        doc.add("empty", "no eligible picks");
    }
    let help = if first.is_empty() {
        vec![
            "hackertube ingest --path <zip-or-dir>".to_string(),
            "hackertube recommend --area agentic-engineering".into(),
            "hackertube search \"coding agents\"".into(),
        ]
    } else {
        vec![
            format!("hackertube show {first}"),
            "hackertube recommend --area agentic-engineering".into(),
            format!("hackertube insights {first}"),
        ]
    };
    doc.help(&help.iter().map(String::as_str).collect::<Vec<_>>());
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn ingest_cmd(
    flags: &Flags,
    path: Option<&str>,
    source: &str,
    limit: Option<i64>,
    refresh: bool,
) -> Result<(Doc, i32), AppError> {
    let home = paths::data_home();
    let catalog = Catalog::open(&home)?;
    let embedder = EmbedStatus::load(&home);
    let names: Vec<&str> = if source == "all" {
        vec!["hackertube", "aietalks"]
    } else {
        vec![source]
    };
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for name in names {
        let source_path = if source == name || (source == "all" && name == "hackertube") {
            path.map(Path::new).map(|path| path.to_path_buf())
        } else {
            None
        };
        let ctx = IngestContext {
            catalog: &catalog,
            home: &home,
            path: source_path,
            limit: limit.map(|value| value as usize),
            refresh,
            embedder: &embedder,
        };
        match ingest::ingest_source(&ctx, name) {
            Ok(result) => rows.push(vec![
                ("id".into(), Val::from(result.source)),
                ("videos".into(), Val::from(result.videos)),
                ("embedded".into(), Val::from(result.embedded)),
                ("changed".into(), Val::from(result.changed)),
                ("detail".into(), Val::from(result.detail)),
            ]),
            Err(err) => errors.push(format!("{name}: {}", err.detail)),
        }
    }
    let mut doc = Doc::new();
    let (fields, projected) = output::project_fields(
        &rows,
        &["id", "videos", "embedded", "changed"],
        flags.fields.as_deref(),
        &["id", "videos", "embedded", "changed", "detail"],
    )
    .map_err(usage_as_app)?;
    doc.table(
        "sources",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    if !errors.is_empty() {
        doc.add("ok", false);
        doc.add("error", "ingest_failed");
        doc.add("detail", errors.join("; "));
        doc.add("exit", 1_i64);
        doc.help(&["hackertube ingest --help", "hackertube"]);
        output::emit(&doc, flags.json, flags.full);
        return Ok((doc, 1));
    }
    doc.add("ok", true);
    if let Some(notice) = embedder.notice() {
        doc.add("notice", notice);
    }
    doc.help(&[
        "hackertube",
        "hackertube recommend --area agentic-engineering",
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn recommend(
    flags: &Flags,
    area: Option<&str>,
    limit: Option<i64>,
    include_news: bool,
    source: Option<&str>,
) -> Result<(Doc, i32), AppError> {
    let limit = bounded(limit, 5, flags.full).map_err(usage_as_app)?;
    let home = paths::data_home();
    let marks = state::load(&home)?;
    if !Catalog::exists(&home) {
        let mut doc = Doc::new();
        doc.add("count", "0 of 0");
        doc.add("empty", "no catalog yet");
        doc.table("videos", &["vid", "title", "score", "reason"], vec![]);
        doc.help(&["hackertube ingest --path <zip-or-dir>"]);
        output::emit(&doc, flags.json, flags.full);
        return Ok((doc, 0));
    }
    let catalog = Catalog::open(&home)?;
    sync(&catalog, &marks)?;
    ingest::reclassify_if_stale(&catalog, &home)?;
    let areas = area.map(|area| vec![area.to_string()]).unwrap_or_else(|| {
        areas::AREA_IDS
            .iter()
            .map(|area| (*area).to_string())
            .collect()
    });
    let mut doc = Doc::new();
    let mut summaries = Vec::new();
    let mut first = String::new();
    for area_id in &areas {
        let (rows, matched, excluded, hidden_news) =
            recommend_rows(&catalog, area_id, limit, include_news, source, &marks)?;
        let records = rows
            .iter()
            .map(
                |(vid, title, score, reason, channel, source, url, level, topic)| {
                    vec![
                        ("vid".into(), Val::from(vid.clone())),
                        ("title".into(), Val::from(title.clone())),
                        ("score".into(), Val::from(*score)),
                        ("reason".into(), Val::from(reason.clone())),
                        ("channel".into(), Val::from(channel.clone())),
                        ("source".into(), Val::from(source.clone())),
                        ("url".into(), Val::from(url.clone())),
                        ("level".into(), Val::from(level.clone())),
                        ("topic".into(), Val::from(topic.clone())),
                        ("area".into(), Val::from(area_id.clone())),
                    ]
                },
            )
            .collect::<Vec<_>>();
        let (fields, projected) = output::project_fields(
            &records,
            &["vid", "title", "score", "reason"],
            flags.fields.as_deref(),
            VIDEO_FIELDS,
        )
        .map_err(usage_as_app)?;
        let key = if area.is_some() {
            "videos"
        } else {
            area_id.as_str()
        };
        doc.table(
            key,
            &fields.iter().map(String::as_str).collect::<Vec<_>>(),
            projected,
        );
        summaries.push(format!("{area_id} {} of {matched}", rows.len()));
        if area.is_some() {
            doc.add("area", area_id.clone());
            doc.add("count", format!("{} of {matched} eligible", rows.len()));
            doc.add(
                "excluded",
                format!(
                    "seen {}, disliked {}, snoozed {}",
                    excluded[0], excluded[1], excluded[2]
                ),
            );
            doc.add("hidden_news", hidden_news);
        }
        if first.is_empty() {
            if let Some(row) = rows.first() {
                first = row.0.clone();
            }
        }
        if rows.is_empty() {
            doc.add("empty", format!("no eligible videos in {area_id}"));
        }
    }
    if area.is_none() {
        doc.add("count", summaries.join(", "));
    }
    let help = if !first.is_empty() {
        vec![
            format!("hackertube show {first}"),
            format!("hackertube insights {first}"),
            format!("hackertube seen {first}"),
        ]
    } else if !Catalog::exists(&home) {
        vec!["hackertube ingest --path <zip-or-dir>".to_string()]
    } else {
        vec!["hackertube recommend --area ai-engineering".to_string()]
    };
    doc.help(&help.iter().map(String::as_str).collect::<Vec<_>>());
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

type Pick = (
    String,
    String,
    f64,
    String,
    String,
    String,
    String,
    String,
    String,
);

fn recommend_rows(
    catalog: &Catalog,
    area: &str,
    limit: i64,
    include_news: bool,
    source: Option<&str>,
    marks: &BTreeMap<String, Mark>,
) -> Result<(Vec<Pick>, i64, [i64; 3], i64), AppError> {
    let column = db::area_column(area)
        .ok_or_else(|| AppError::new("usage", format!("unknown area {area}")))?;
    let news = if include_news { "1" } else { "0" };
    let source_sql = source.map(|_| "AND source = ?1").unwrap_or("");
    let count_sql = format!(
        "SELECT COUNT(*) FROM videos WHERE {column} >= 2 AND content_type NOT IN ('comedy','music') AND (shelf_life != 'news' OR {news} = 1) AND (source != 'hackertube' OR hn_recommenders > 0) AND blocked = 0 {source_sql}"
    );
    let matched: i64 = if let Some(source) = source {
        catalog
            .conn
            .query_row(&count_sql, params![source], |row| row.get(0))?
    } else {
        catalog.conn.query_row(&count_sql, [], |row| row.get(0))?
    };
    let list_sql = format!(
        "SELECT vid, title, score, why, summary, topics_json, primary_topic, channel, source, url, level,
                shelf_life, published, hn_recommenders, {column}, cos_agentic, cos_engineering, cos_research
         FROM videos WHERE {column} >= 2 AND content_type NOT IN ('comedy','music')
         AND (shelf_life != 'news' OR {news} = 1) AND (source != 'hackertube' OR hn_recommenders > 0) AND blocked = 0 {source_sql}"
    );
    let mut stmt = catalog.conn.prepare(&list_sql)?;
    let map = |row: &rusqlite::Row<'_>| {
        Ok(Candidate {
            vid: row.get(0)?,
            title: row.get(1)?,
            source_score: row.get(2)?,
            why: row.get(3)?,
            summary: row.get(4)?,
            slugs: row.get(5)?,
            topic: row.get(6)?,
            channel: row.get(7)?,
            source: row.get(8)?,
            url: row.get(9)?,
            level: row.get(10)?,
            shelf_life: row.get(11)?,
            published: row.get(12)?,
            recommenders: row.get(13)?,
            affinity: row.get(14)?,
            cos_agentic: row.get(15)?,
            cos_engineering: row.get(16)?,
            cos_research: row.get(17)?,
        })
    };
    let collected = if let Some(source) = source {
        stmt.query_map(params![source], map)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], map)?.collect::<Result<Vec<_>, _>>()?
    };
    drop(stmt);
    let like = centroid(&catalog.conn, &marked(marks, "liked"))?;
    let dislike = centroid(&catalog.conn, &marked(marks, "disliked"))?;
    let mut ranked = Vec::with_capacity(collected.len());
    for candidate in collected {
        let seed = seed_cosine(
            area,
            candidate.cos_agentic,
            candidate.cos_engineering,
            candidate.cos_research,
        );
        let (like_cosine, dislike_cosine) =
            profile_cosines(&catalog.conn, &candidate.vid, &like, &dislike)?;
        let score = rank::recommendation_score(&RankInput {
            affinity: candidate.affinity,
            seed_cosine: seed,
            like_cosine,
            dislike_cosine,
            source_score: candidate.source_score,
            recommenders: candidate.recommenders,
            shelf_life: &candidate.shelf_life,
            published: &candidate.published,
        });
        let parts = RankParts {
            score,
            affinity: candidate.affinity,
            seed_cosine: seed.unwrap_or(0.0),
            quality: rank::source_quality_of(candidate.source_score, candidate.recommenders),
            fresh: rank::freshness_of(&candidate.shelf_life, &candidate.published),
            title: candidate.title.clone(),
        };
        ranked.push((candidate, parts));
    }
    ranked.sort_by(|left, right| rank::compare_rank(&left.1, &right.1));
    let rows = ranked
        .into_iter()
        .take(limit as usize)
        .map(|(candidate, parts)| {
            let topic_slugs =
                serde_json::from_str::<Vec<String>>(&candidate.slugs).unwrap_or_default();
            let reason = areas::reason(
                &candidate.why,
                &candidate.summary,
                &topic_slugs,
                &candidate.topic,
                area,
            );
            (
                candidate.vid,
                candidate.title,
                parts.score,
                reason,
                candidate.channel,
                candidate.source,
                candidate.url,
                candidate.level,
                candidate.topic,
            )
        })
        .collect();
    let hidden_sql = format!("SELECT COUNT(*) FROM videos WHERE {column} >= 2 AND shelf_life = 'news' AND (source != 'hackertube' OR hn_recommenders > 0)");
    let hidden_news: i64 = catalog.conn.query_row(&hidden_sql, [], |row| row.get(0))?;
    let excluded = excluded_counts(&catalog.conn, column, marks)?;
    Ok((rows, matched, excluded, hidden_news))
}

struct Candidate {
    vid: String,
    title: String,
    source_score: f64,
    why: String,
    summary: String,
    slugs: String,
    topic: String,
    channel: String,
    source: String,
    url: String,
    level: String,
    shelf_life: String,
    published: String,
    recommenders: i64,
    affinity: i32,
    cos_agentic: Option<f64>,
    cos_engineering: Option<f64>,
    cos_research: Option<f64>,
}

fn seed_cosine(
    area: &str,
    agentic: Option<f64>,
    engineering: Option<f64>,
    research: Option<f64>,
) -> Option<f64> {
    match area {
        "agentic-engineering" => agentic,
        "ai-engineering" => engineering,
        "ai-research" => research,
        _ => None,
    }
}

fn marked(marks: &BTreeMap<String, Mark>, status: &str) -> Vec<String> {
    marks
        .iter()
        .filter(|(_, mark)| mark.status == status)
        .map(|(vid, _)| vid.clone())
        .collect()
}

fn centroid(conn: &rusqlite::Connection, vids: &[String]) -> Result<Option<Vec<f32>>, AppError> {
    let mut sum = vec![0.0_f32; crate::embed::DIM];
    let mut count = 0_usize;
    for vid in vids {
        let Some(vector) = db::load_embedding(conn, vid)? else {
            continue;
        };
        if vector.len() != sum.len() {
            continue;
        }
        for (slot, value) in sum.iter_mut().zip(vector) {
            *slot += value;
        }
        count += 1;
    }
    if count == 0 {
        return Ok(None);
    }
    for value in &mut sum {
        *value /= count as f32;
    }
    crate::vecstore::normalize(&mut sum);
    Ok(Some(sum))
}

fn profile_cosines(
    conn: &rusqlite::Connection,
    vid: &str,
    like: &Option<Vec<f32>>,
    dislike: &Option<Vec<f32>>,
) -> Result<(Option<f64>, Option<f64>), AppError> {
    if like.is_none() && dislike.is_none() {
        return Ok((None, None));
    }
    let Some(vector) = db::load_embedding(conn, vid)? else {
        return Ok((None, None));
    };
    let cos = |profile: &Option<Vec<f32>>| {
        profile
            .as_ref()
            .map(|profile| f64::from(areas::cosine(&vector, profile)))
    };
    Ok((cos(like), cos(dislike)))
}

fn excluded_counts(
    conn: &rusqlite::Connection,
    column: &str,
    marks: &BTreeMap<String, Mark>,
) -> Result<[i64; 3], AppError> {
    let sql = format!("SELECT vid FROM videos WHERE {column} >= 2 AND blocked = 1");
    let mut stmt = conn.prepare(&sql)?;
    let vids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = 0;
    let mut disliked = 0;
    let mut snoozed = 0;
    for vid in vids {
        match marks.get(&vid).map(|mark| mark.status.as_str()) {
            Some("seen") => seen += 1,
            Some("disliked") => disliked += 1,
            Some("snoozed") => snoozed += 1,
            _ => {}
        }
    }
    Ok([seen, disliked, snoozed])
}

pub fn search(
    flags: &Flags,
    query: &str,
    area: Option<&str>,
    where_in: &str,
    source: Option<&str>,
    limit: Option<i64>,
) -> Result<(Doc, i32), AppError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(usage_as_app(UsageError::new("search needs a query")));
    }
    let limit = bounded(limit, 8, flags.full).map_err(usage_as_app)?;
    let home = paths::data_home();
    if !Catalog::exists(&home) {
        let mut doc = Doc::new();
        doc.add("query", query);
        doc.add("in", where_in);
        doc.add("count", "0 of 0");
        doc.add("empty", "no catalog yet");
        doc.table("videos", &["vid", "title", "score", "match"], vec![]);
        doc.help(&["hackertube ingest --path <zip-or-dir>"]);
        output::emit(&doc, flags.json, flags.full);
        return Ok((doc, 0));
    }
    let catalog = Catalog::open(&home)?;
    let marks = state::load(&home)?;
    sync(&catalog, &marks)?;
    ingest::reclassify_if_stale(&catalog, &home)?;
    let embedder = EmbedStatus::load(&home);
    if where_in == "transcripts" {
        return search_transcripts(flags, &catalog, &embedder, query, area, source, limit);
    }
    let keyword = db::keyword_videos(&catalog.conn, query, 50)?;
    let mut semantic: HashMap<String, f64> = HashMap::new();
    let mut mode = "keyword".to_string();
    if let EmbedStatus::Ready(model) = &embedder {
        if let Ok(vectors) = model.embed(&[query.to_string()]) {
            if let Some(vector) = vectors.first() {
                let hits = SqliteVec {
                    conn: &catalog.conn,
                }
                .knn_videos(
                    vector,
                    80,
                    &KnnFilter {
                        source: source.map(str::to_string),
                        area: area.map(str::to_string),
                        hide_news: true,
                        hide_blocked: true,
                        exclude_vid: None,
                    },
                )?;
                for hit in hits {
                    semantic.insert(hit.vid, similarity(hit.distance));
                }
                mode = "semantic+keyword".into();
            }
        }
    }
    let mut keyword_map: HashMap<String, f64> = keyword.into_iter().collect();
    if let Some(source) = source {
        keyword_map.retain(|vid, _| video_source(&catalog, vid).as_deref() == Some(source));
    }
    if let Some(area) = area {
        let column = db::area_column(area).unwrap_or("area_agentic");
        keyword_map.retain(|vid, _| area_at_least(&catalog, vid, column, 2));
    }
    let mut ids: HashSet<String> = semantic.keys().cloned().collect();
    ids.extend(keyword_map.keys().cloned());
    let mut ranked: Vec<(String, f64)> = ids
        .into_iter()
        .map(|vid| {
            let sem = semantic.get(&vid).copied().unwrap_or(0.0);
            let kw = keyword_map.get(&vid).copied().unwrap_or(0.0);
            let score = if mode == "keyword" {
                kw
            } else {
                0.7 * sem + 0.3 * kw
            };
            (vid, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let total = ranked.len();
    let shown = ranked.into_iter().take(limit as usize).collect::<Vec<_>>();
    let mut records = Vec::new();
    for (vid, score) in &shown {
        if let Some(video) = db::get_video(&catalog.conn, vid)? {
            records.push(video_record(
                &video,
                &[("match", Val::from(round3(*score)))],
            ));
        }
    }
    let mut doc = Doc::new();
    doc.add("query", query);
    doc.add("in", "catalog");
    doc.add("mode", mode.clone());
    doc.add(
        "semantic",
        if mode == "semantic+keyword" {
            "on"
        } else {
            "off"
        },
    );
    if let Some(notice) = embedder.notice() {
        doc.add("notice", notice);
    }
    doc.add("count", format!("{} of {total}", records.len()));
    let (fields, projected) = output::project_fields(
        &records,
        &["vid", "title", "score", "match"],
        flags.fields.as_deref(),
        VIDEO_FIELDS,
    )
    .map_err(usage_as_app)?;
    doc.table(
        "videos",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    if records.is_empty() {
        doc.add("empty", format!("no videos match \"{query}\""));
    }
    let help = if let Some(first) = shown.first() {
        vec![
            format!("hackertube show {}", first.0),
            format!("hackertube similar {}", first.0),
        ]
    } else {
        vec!["hackertube search <query> --in transcripts".to_string()]
    };
    doc.help(&help.iter().map(String::as_str).collect::<Vec<_>>());
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

fn search_transcripts(
    flags: &Flags,
    catalog: &Catalog,
    embedder: &EmbedStatus,
    query: &str,
    area: Option<&str>,
    source: Option<&str>,
    limit: i64,
) -> Result<(Doc, i32), AppError> {
    let mut hits: HashMap<(String, i64), f64> = HashMap::new();
    let mut mode = "keyword".to_string();
    if let EmbedStatus::Ready(model) = embedder {
        if let Ok(vectors) = model.embed(&[query.to_string()]) {
            if let Some(vector) = vectors.first() {
                let found = SqliteVec {
                    conn: &catalog.conn,
                }
                .knn_chunks(
                    vector,
                    (limit as usize).saturating_mul(8).max(40),
                    &KnnFilter {
                        source: source.map(str::to_string),
                        area: area.map(str::to_string),
                        hide_blocked: true,
                        hide_news: false,
                        exclude_vid: None,
                    },
                )?;
                for hit in found {
                    hits.insert((hit.vid, hit.start_s), similarity(hit.distance));
                }
                mode = "semantic+keyword".into();
            }
        }
    }
    for (vid, start_s, _idx, score) in db::keyword_chunks(&catalog.conn, query, limit * 4)? {
        hits.entry((vid, start_s))
            .and_modify(|current| *current = 0.7 * *current + 0.3 * score)
            .or_insert(score);
    }
    let mut ranked: Vec<((String, i64), f64)> = hits.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let total = ranked.len();
    let mut records = Vec::new();
    for ((vid, start_s), score) in ranked.into_iter().take(limit as usize) {
        let text = chunk_by_start(&catalog.conn, &vid, start_s)?.unwrap_or_default();
        records.push(vec![
            ("vid".into(), Val::from(vid)),
            ("at".into(), Val::from(insights::format_at(start_s))),
            ("text".into(), Val::from(text.0)),
            ("score".into(), Val::from(round3(score))),
            ("url".into(), Val::from(text.1)),
        ]);
    }
    let mut doc = Doc::new();
    doc.add("query", query);
    doc.add("in", "transcripts");
    doc.add("mode", mode);
    doc.add(
        "semantic",
        if embedder.notice().is_none() {
            "on"
        } else {
            "off"
        },
    );
    if let Some(notice) = embedder.notice() {
        doc.add("notice", notice);
    }
    doc.add("count", format!("{} of {total}", records.len()));
    let (fields, projected) = output::project_fields(
        &records,
        &["vid", "at", "text", "score"],
        flags.fields.as_deref(),
        &["vid", "at", "text", "score", "url"],
    )
    .map_err(usage_as_app)?;
    doc.table(
        "segments",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    if records.is_empty() {
        doc.add("empty", format!("no transcript segments match \"{query}\""));
    }
    doc.help(&[
        "hackertube transcript <vid>",
        "hackertube ingest --source aietalks",
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn similar(flags: &Flags, vid: &str, limit: Option<i64>) -> Result<(Doc, i32), AppError> {
    let vid = require_vid(vid).map_err(usage_as_app)?;
    let limit = bounded(limit, 5, flags.full).map_err(usage_as_app)?;
    let catalog = open_required()?;
    let video = require_known(&catalog, &vid)?;
    let mut doc = Doc::new();
    doc.add("vid", vid.clone());
    doc.add("title", video.title.clone());
    if let Some(vector) = db::load_embedding(&catalog.conn, &vid)? {
        let hits = SqliteVec {
            conn: &catalog.conn,
        }
        .knn_videos(
            &vector,
            (limit as usize) + 1,
            &KnnFilter {
                exclude_vid: Some(vid.clone()),
                hide_blocked: false,
                ..KnnFilter::default()
            },
        )?;
        let mut records = Vec::new();
        for hit in hits
            .into_iter()
            .filter(|hit| hit.vid != vid)
            .take(limit as usize)
        {
            if let Some(other) = db::get_video(&catalog.conn, &hit.vid)? {
                records.push(video_record(
                    &other,
                    &[("sim", Val::from(similarity(hit.distance)))],
                ));
            }
        }
        doc.add("mode", "embedding");
        doc.add("count", format!("{} of {}", records.len(), records.len()));
        let (fields, projected) = output::project_fields(
            &records,
            &["vid", "title", "score", "sim"],
            flags.fields.as_deref(),
            VIDEO_FIELDS,
        )
        .map_err(usage_as_app)?;
        doc.table(
            "videos",
            &fields.iter().map(String::as_str).collect::<Vec<_>>(),
            projected,
        );
        if records.is_empty() {
            doc.add("empty", format!("no neighbors for {vid}"));
        }
    } else {
        let related = db::related_to(&catalog.conn, &vid, limit)?;
        let mut records = Vec::new();
        for (other, shared) in &related {
            if let Some(video) = db::get_video(&catalog.conn, other)? {
                records.push(video_record(&video, &[("shared", Val::from(*shared))]));
            }
        }
        doc.add("mode", "co-recommended");
        doc.add("count", format!("{} of {}", records.len(), related.len()));
        let (fields, projected) = output::project_fields(
            &records,
            &["vid", "title", "score", "shared"],
            flags.fields.as_deref(),
            VIDEO_FIELDS,
        )
        .map_err(usage_as_app)?;
        doc.table(
            "videos",
            &fields.iter().map(String::as_str).collect::<Vec<_>>(),
            projected,
        );
        if records.is_empty() {
            doc.add("empty", format!("no neighbors for {vid}"));
        }
    }
    doc.help(&[
        &format!("hackertube show {vid}"),
        "hackertube recommend --area ai-engineering",
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn show(flags: &Flags, vid: &str) -> Result<(Doc, i32), AppError> {
    let vid = require_vid(vid).map_err(usage_as_app)?;
    let home = paths::data_home();
    let catalog = open_required()?;
    let video = require_known(&catalog, &vid)?;
    let marks = state::load(&home)?;
    let state_name = marks
        .get(&vid)
        .map(|mark| mark.status.as_str())
        .unwrap_or("unseen");
    let mut area_names = Vec::new();
    if video.area_agentic >= 2 {
        area_names.push("agentic-engineering");
    }
    if video.area_engineering >= 2 {
        area_names.push("ai-engineering");
    }
    if video.area_research >= 2 {
        area_names.push("ai-research");
    }
    let areas = if area_names.is_empty() {
        "none".to_string()
    } else {
        area_names.join(",")
    };
    let mut doc = Doc::new();
    doc.add("vid", video.vid.clone());
    doc.add("title", video.title);
    doc.add("channel", video.channel);
    doc.add("source", video.source);
    doc.add("score", video.score);
    doc.add(
        "areas",
        if areas.is_empty() {
            "none".into()
        } else {
            areas
        },
    );
    doc.add("url", video.url);
    doc.add("page", video.page_url);
    doc.add("state", state_name);
    doc.add("level", video.level);
    doc.add("content_type", video.content_type);
    doc.add("shelf_life", video.shelf_life);
    doc.add("topic", video.primary_topic);
    doc.add("summary", video.summary);
    doc.add("why", video.why);
    doc.add("published", video.published);
    doc.add("event", video.event);
    let cached = home.join("transcripts").join(format!("{vid}.md")).exists();
    doc.add("transcript", if cached { "cached" } else { "absent" });
    if !video.quote.is_empty() {
        doc.add("quote", video.quote);
    }
    if let Some(fields) = &flags.fields {
        let mut filtered = Doc::new();
        for field in fields {
            if let Some((_, value)) = doc.entries.iter().find(|(key, _)| key == field) {
                filtered.add(field, value.clone());
            } else {
                return Err(usage_as_app(
                    UsageError::new(format!("unknown field {field}")).flag("--fields"),
                ));
            }
        }
        doc = filtered;
    }
    doc.help(&[
        &format!("hackertube insights {vid}"),
        &format!("hackertube similar {vid}"),
        &format!("hackertube seen {vid}"),
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn transcript(flags: &Flags, vid: &str) -> Result<(Doc, i32), AppError> {
    let vid = require_vid(vid).map_err(usage_as_app)?;
    let home = paths::data_home();
    let (markdown, origin) = ensure_transcript(&home, &vid)?;
    let shown = if flags.full {
        markdown.clone()
    } else {
        output::truncate(&markdown, 500)
    };
    let mut doc = Doc::new();
    doc.add("vid", vid.clone());
    doc.add("origin", origin);
    doc.add("source_url", source_url(&vid, &markdown));
    doc.add("chars", markdown.chars().count() as i64);
    doc.add(
        "showing",
        format!(
            "{} of {}",
            shown.chars().count().min(markdown.chars().count()),
            markdown.chars().count()
        ),
    );
    doc.add("text", if flags.full { markdown } else { shown });
    doc.help(&[
        &format!("hackertube insights {vid}"),
        &format!("hackertube transcript {vid} --full"),
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn insights_cmd(flags: &Flags, vid: &str, llm: bool) -> Result<(Doc, i32), AppError> {
    let vid = require_vid(vid).map_err(usage_as_app)?;
    let home = paths::data_home();
    let (markdown, origin) = ensure_transcript(&home, &vid)?;
    let segments = insights::parse_markdown(&markdown, &vid);
    let scale = if flags.full { 8 } else { 1 };
    let mut extracted = insights::extract(&segments, &markdown, &vid, scale);
    if llm {
        let excerpt: String = markdown.chars().take(12000).collect();
        let raw = post_llm(&format!("Video {vid}. Extract up to 5 grounded claims as JSON {{\"claims\":[{{\"at\":\"m:ss\",\"text\":\"verbatim\"}}]}}.\n\n{excerpt}"))?;
        let payload = serde_json::from_str::<serde_json::Value>(&strip_fence(&raw))
            .unwrap_or(serde_json::json!({}));
        extracted = insights::merge_llm_claims(
            extracted,
            &payload,
            &insights::known_timestamps(&segments),
            &markdown,
        );
    }
    let mut doc = Doc::new();
    doc.add("vid", vid.clone());
    doc.add("origin", origin);
    doc.add("method", extracted.method.clone());
    doc.add("source_url", source_url(&vid, &markdown));
    doc.add(
        "chapters_count",
        format!(
            "{} of {}",
            extracted.chapters.len(),
            extracted.chapters_total
        ),
    );
    doc.add(
        "claims_count",
        format!("{} of {}", extracted.claims.len(), extracted.claims_total),
    );
    doc.add(
        "mentions_count",
        format!(
            "{} of {}",
            extracted.mentions.len(),
            extracted.mentions_total
        ),
    );
    doc.add(
        "quotes_count",
        format!("{} of {}", extracted.quotes.len(), extracted.quotes_total),
    );
    doc.table(
        "chapters",
        &["at", "text", "url"],
        items_to_rows(&extracted.chapters),
    );
    doc.table(
        "claims",
        &["at", "text", "url"],
        items_to_rows(&extracted.claims),
    );
    doc.table(
        "mentions",
        &["kind", "name", "at", "url"],
        extracted
            .mentions
            .iter()
            .map(|item| {
                vec![
                    Val::from(item.kind.clone()),
                    Val::from(item.name.clone()),
                    Val::from(item.at.clone()),
                    Val::from(item.url.clone()),
                ]
            })
            .collect(),
    );
    doc.table(
        "quotes",
        &["at", "text", "url"],
        items_to_rows(&extracted.quotes),
    );
    if extracted.claims.is_empty() && extracted.quotes.is_empty() {
        doc.add("empty", "no claims or quotes parsed");
    }
    doc.help(&[
        &format!("hackertube transcript {vid}"),
        &format!("hackertube show {vid}"),
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn mark(
    flags: &Flags,
    verb: &str,
    vid: &str,
    days: Option<i64>,
    until: Option<&str>,
) -> Result<(Doc, i32), AppError> {
    let vid = require_vid(vid).map_err(usage_as_app)?;
    let home = paths::data_home();
    let (status, until_value) = match verb {
        "seen" => ("seen", String::new()),
        "like" => ("liked", String::new()),
        "dislike" => ("disliked", String::new()),
        "snooze" => {
            if until.is_some() && days.is_some() {
                return Err(usage_as_app(UsageError::new(
                    "pass --until or --days, not both",
                )));
            }
            let until_value = if let Some(until) = until {
                if paths::parse_day(until).is_none() {
                    return Err(usage_as_app(UsageError::new("--until must be YYYY-MM-DD")));
                }
                until.to_string()
            } else {
                let span = days.unwrap_or(7);
                if span < 0 {
                    return Err(usage_as_app(UsageError::new("--days must be >= 0")));
                }
                (paths::today() + chrono::Duration::days(span))
                    .format("%Y-%m-%d")
                    .to_string()
            };
            ("snoozed", until_value)
        }
        "forget" => {
            let changed = state::forget(&home, &vid)?;
            let mut doc = Doc::new();
            doc.add("ok", true);
            doc.add("action", "forget");
            doc.add("vid", vid.clone());
            doc.add("changed", changed);
            doc.add("status", "absent");
            doc.help(&[
                &format!("hackertube show {vid}"),
                "hackertube recommend --area ai-engineering",
            ]);
            output::emit(&doc, flags.json, flags.full);
            return Ok((doc, 0));
        }
        _ => {
            return Err(usage_as_app(UsageError::new(format!(
                "unknown command {verb}"
            ))))
        }
    };
    if verb != "forget" {
        let catalog = open_required()?;
        require_known(&catalog, &vid)?;
    }
    let changed = state::set_mark(&home, &vid, status, &until_value)?;
    if Catalog::exists(&home) {
        let catalog = Catalog::open(&home)?;
        let marks = state::load(&home)?;
        sync(&catalog, &marks)?;
    }
    let stored = state::load(&home)?;
    let mut doc = Doc::new();
    doc.add("ok", true);
    doc.add("action", status);
    doc.add("vid", vid.clone());
    doc.add("changed", changed);
    doc.add("status", status);
    doc.add(
        "at",
        stored
            .get(&vid)
            .map(|mark| mark.at.clone())
            .unwrap_or_default(),
    );
    if !until_value.is_empty() {
        doc.add("until", until_value);
    }
    doc.help(&["hackertube state", &format!("hackertube forget {vid}")]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn state_cmd(flags: &Flags, limit: Option<i64>) -> Result<(Doc, i32), AppError> {
    let limit = bounded(limit, 20, flags.full).map_err(usage_as_app)?;
    let home = paths::data_home();
    let marks = state::load(&home)?;
    let rows: Vec<Vec<(String, Val)>> = marks
        .iter()
        .take(limit as usize)
        .map(|(vid, mark)| {
            vec![
                ("vid".into(), Val::from(vid.clone())),
                ("status".into(), Val::from(mark.status.clone())),
                ("at".into(), Val::from(mark.at.clone())),
                (
                    "until".into(),
                    if mark.until.is_empty() {
                        Val::Null
                    } else {
                        Val::from(mark.until.clone())
                    },
                ),
            ]
        })
        .collect();
    let (fields, projected) = output::project_fields(
        &rows,
        &["vid", "status", "at", "until"],
        flags.fields.as_deref(),
        &["vid", "status", "at", "until"],
    )
    .map_err(usage_as_app)?;
    let mut doc = Doc::new();
    doc.add("state_file", display_path(&state::path_for(&home)));
    doc.add("count", format!("{} of {}", rows.len(), marks.len()));
    doc.add("active", state::state_line(&marks));
    doc.table(
        "items",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    if marks.is_empty() {
        doc.add("empty", "no saved marks");
    }
    doc.help(&["hackertube seen <vid>", "hackertube snooze <vid> --days 7"]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn sources_cmd(flags: &Flags) -> Result<(Doc, i32), AppError> {
    let home = paths::data_home();
    let loaded = if Catalog::exists(&home) {
        db::source_counts(&Catalog::open(&home)?.conn)?
    } else {
        Vec::new()
    };
    let mut records = Vec::new();
    for name in ["hackertube", "aietalks"] {
        let videos = loaded
            .iter()
            .find(|(source, _)| source == name)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        records.push(vec![
            ("id".into(), Val::from(name)),
            ("videos".into(), Val::from(videos)),
            (
                "status".into(),
                Val::from(if videos > 0 { "loaded" } else { "empty" }),
            ),
        ]);
    }
    let (fields, projected) = output::project_fields(
        &records,
        &["id", "videos", "status"],
        flags.fields.as_deref(),
        &["id", "videos", "status"],
    )
    .map_err(usage_as_app)?;
    let mut doc = Doc::new();
    doc.table(
        "sources",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    doc.help(&[
        "hackertube ingest --source hackertube --path <dir>",
        "hackertube ingest --source aietalks",
    ]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn areas_cmd(flags: &Flags) -> Result<(Doc, i32), AppError> {
    let home = paths::data_home();
    let mut records = Vec::new();
    if Catalog::exists(&home) {
        let catalog = Catalog::open(&home)?;
        let marks = state::load(&home)?;
        sync(&catalog, &marks)?;
        ingest::reclassify_if_stale(&catalog, &home)?;
        for area in areas::AREA_IDS {
            let (_, matched, _, _) = recommend_rows(&catalog, area, 1, false, None, &marks)?;
            records.push(vec![
                ("id".into(), Val::from(area)),
                ("videos".into(), Val::from(matched)),
                ("description".into(), Val::from(area_description(area))),
            ]);
        }
    } else {
        for area in areas::AREA_IDS {
            records.push(vec![
                ("id".into(), Val::from(area)),
                ("videos".into(), Val::from(0_i64)),
                ("description".into(), Val::from(area_description(area))),
            ]);
        }
    }
    let (fields, projected) = output::project_fields(
        &records,
        &["id", "videos", "description"],
        flags.fields.as_deref(),
        &["id", "videos", "description"],
    )
    .map_err(usage_as_app)?;
    let mut doc = Doc::new();
    doc.table(
        "areas",
        &fields.iter().map(String::as_str).collect::<Vec<_>>(),
        projected,
    );
    doc.help(&["hackertube recommend --area agentic-engineering"]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

pub fn setup(flags: &Flags) -> Result<(Doc, i32), AppError> {
    let home = paths::data_home();
    std::fs::create_dir_all(home.join("hooks"))?;
    let script = home.join("hooks").join("session.sh");
    let script_body = "#!/bin/sh\n# HackerTube session hook. Prints the live home view. Safe to run repeatedly.\nexec hackertube\n";
    let script_changed = write_if_changed(&script, script_body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    }
    let fragment = home.join("hooks").join("claude.settings.fragment.json");
    let fragment_body = "{\n  \"hooks\": {\n    \"SessionStart\": [\n      {\"hooks\": [{\"type\": \"command\", \"command\": \"hackertube\"}]}\n    ]\n  }\n}\n";
    let _fragment_changed = write_if_changed(&fragment, fragment_body)?;
    let settings = paths::user_home().join(".claude").join("settings.json");
    let claude_changed = merge_claude(&settings)?;
    let mut doc = Doc::new();
    doc.add("ok", true);
    doc.add("changed", script_changed || claude_changed);
    doc.add("script", display_path(&script));
    doc.add("fragment", display_path(&fragment));
    doc.add("claude_settings", display_path(&settings));
    doc.add("script_changed", script_changed);
    doc.add("claude_changed", claude_changed);
    doc.help(&["hackertube", "restart the agent session so the hook runs"]);
    output::emit(&doc, flags.json, flags.full);
    Ok((doc, 0))
}

fn write_if_changed(path: &Path, body: &str) -> Result<bool, AppError> {
    if path.exists() && std::fs::read_to_string(path).ok().as_deref() == Some(body) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(true)
}

fn merge_claude(path: &Path) -> Result<bool, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut current = if path.exists() {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str::<serde_json::Value>(&text).map_err(|err| {
            AppError::new(
                "settings_invalid",
                format!("{} is not JSON; hook not written: {err}", path.display()),
            )
            .with("path", path.display().to_string())
        })?
    } else {
        serde_json::json!({})
    };
    let hooks = current
        .as_object_mut()
        .ok_or_else(|| AppError::new("settings_invalid", "settings must be an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let session = hooks
        .as_object_mut()
        .ok_or_else(|| AppError::new("settings_invalid", "settings.hooks must be an object"))?
        .entry("SessionStart")
        .or_insert_with(|| serde_json::json!([]));
    let list = session.as_array_mut().ok_or_else(|| {
        AppError::new(
            "settings_invalid",
            "settings.hooks.SessionStart must be a list",
        )
    })?;
    let already = list.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .is_some_and(|hooks| {
                hooks
                    .iter()
                    .any(|hook| hook.get("command").and_then(|c| c.as_str()) == Some("hackertube"))
            })
    });
    if already {
        return Ok(false);
    }
    list.push(serde_json::json!({"hooks": [{"type": "command", "command": "hackertube"}]}));
    std::fs::write(
        path,
        serde_json::to_string_pretty(&current).unwrap_or_else(|_| "{}".into()) + "\n",
    )?;
    Ok(true)
}

fn ensure_transcript(home: &Path, vid: &str) -> Result<(String, String), AppError> {
    let path = home.join("transcripts").join(format!("{vid}.md"));
    if path.exists() {
        let markdown = std::fs::read_to_string(&path)?;
        let origin = if markdown
            .lines()
            .take(6)
            .any(|line| line.contains("Origin: youtube-captions"))
        {
            "captions"
        } else if Catalog::exists(home) {
            db::get_video(&Catalog::open(home)?.conn, vid)?
                .map(|video| {
                    if video.source == "aietalks" {
                        "aietalks"
                    } else {
                        "cache"
                    }
                })
                .unwrap_or("cache")
        } else {
            "cache"
        };
        return Ok((markdown, origin.to_string()));
    }
    if std::env::var("HACKERTUBE_CAPTIONS").ok().as_deref() == Some("off") {
        return Err(AppError::new(
            "transcript_unavailable",
            "no cached transcript and captions are disabled",
        )
        .with("vid", vid)
        .with("tried", "cache,youtube_captions"));
    }
    match fetch_captions(vid) {
        Ok(cues) => {
            let mut lines = vec![
                format!("Source: https://www.youtube.com/watch?v={vid}"),
                "Origin: youtube-captions".into(),
                String::new(),
                "## Full transcript".into(),
                String::new(),
            ];
            for (start, text) in cues {
                let seconds = start as i64;
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if cleaned.is_empty() {
                    continue;
                }
                lines.push(format!(
                    "[{}]({}) {cleaned}",
                    insights::format_at(seconds),
                    insights::youtube_url(vid, Some(seconds))
                ));
                lines.push(String::new());
            }
            let markdown = lines.join("\n");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &markdown)?;
            Ok((markdown, "captions".into()))
        }
        Err(err) => Err(AppError::new("transcript_unavailable", err.detail)
            .with("vid", vid)
            .with("tried", "cache,youtube_captions")),
    }
}

fn fetch_captions(vid: &str) -> Result<Vec<(f64, String)>, AppError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build();
    let page = agent
        .get(&format!("https://www.youtube.com/watch?v={vid}"))
        .set("User-Agent", "hackertube/0.2")
        .call()
        .map_err(|err| AppError::new("captions", err.to_string()))?
        .into_string()
        .map_err(|err| AppError::new("captions", err.to_string()))?;
    let Some(encoded) = page
        .split("captionTracks\":[{\"baseUrl\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
    else {
        return Err(AppError::new("captions", "no caption track"));
    };
    let url = encoded.replace("\\u0026", "&");
    let xml = agent
        .get(&url)
        .set("User-Agent", "hackertube/0.2")
        .call()
        .map_err(|err| AppError::new("captions", err.to_string()))?
        .into_string()
        .map_err(|err| AppError::new("captions", err.to_string()))?;
    let mut cues = Vec::new();
    for chunk in xml.split("<text ") {
        let Some(start) = chunk
            .split("start=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
        else {
            continue;
        };
        let Some(body) = chunk
            .split('>')
            .nth(1)
            .and_then(|rest| rest.split("</text>").next())
        else {
            continue;
        };
        let text = body
            .replace("&amp;", "&")
            .replace("&#39;", "'")
            .replace("&quot;", "\"");
        if let Ok(start) = start.parse::<f64>() {
            cues.push((start, text));
        }
    }
    if cues.is_empty() {
        return Err(AppError::new("captions", "captions response was empty"));
    }
    Ok(cues)
}

fn post_llm(prompt: &str) -> Result<String, AppError> {
    let key = std::env::var("HACKERTUBE_LLM_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return Err(AppError::new(
            "llm_unconfigured",
            "set HACKERTUBE_LLM_API_KEY or omit --llm",
        ));
    }
    let base = std::env::var("HACKERTUBE_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let model = std::env::var("HACKERTUBE_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": "Return JSON {\"claims\":[{\"at\":\"m:ss\",\"text\":\"verbatim quote\"}]}. Quote the transcript. Do not paraphrase."},
            {"role": "user", "content": prompt}
        ]
    });
    let response = ureq::post(&format!("{}/chat/completions", base.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|err| AppError::new("llm_failed", err.to_string()))?;
    let payload: serde_json::Value = response
        .into_json()
        .map_err(|err| AppError::new("llm_failed", err.to_string()))?;
    payload
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| AppError::new("llm_failed", "model response had no message content"))
}

fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

fn items_to_rows(items: &[insights::Item]) -> Vec<Vec<Val>> {
    items
        .iter()
        .map(|item| {
            vec![
                Val::from(item.at.clone()),
                Val::from(item.text.clone()),
                Val::from(item.url.clone()),
            ]
        })
        .collect()
}

fn source_url(vid: &str, markdown: &str) -> String {
    for line in markdown.lines() {
        if let Some(rest) = line.strip_prefix("Source: ") {
            return rest.trim().to_string();
        }
        if let Some(index) = line.find("https://www.youtube.com/watch?v=") {
            let rest = &line[index..];
            let end = rest
                .find(|ch: char| ch.is_whitespace() || ch == ')')
                .unwrap_or(rest.len());
            return rest[..end].to_string();
        }
    }
    format!("https://www.youtube.com/watch?v={vid}")
}

fn video_record(video: &Video, extra: &[(&str, Val)]) -> Vec<(String, Val)> {
    let mut row = vec![
        ("vid".into(), Val::from(video.vid.clone())),
        ("title".into(), Val::from(video.title.clone())),
        ("score".into(), Val::from(video.score)),
        ("channel".into(), Val::from(video.channel.clone())),
        ("source".into(), Val::from(video.source.clone())),
        ("url".into(), Val::from(video.url.clone())),
        ("topic".into(), Val::from(video.primary_topic.clone())),
        ("level".into(), Val::from(video.level.clone())),
    ];
    for (key, value) in extra {
        row.push(((*key).to_string(), value.clone()));
    }
    row
}

fn sync(catalog: &Catalog, marks: &BTreeMap<String, Mark>) -> Result<(), AppError> {
    db::sync_blocked(&catalog.conn, &state::blocked_vids(marks, paths::today()))
}

fn open_required() -> Result<Catalog, AppError> {
    let home = paths::data_home();
    if !Catalog::exists(&home) {
        return Err(AppError::new(
            "catalog_missing",
            "ingest a catalog before this command",
        ));
    }
    Catalog::open(&home)
}

fn require_known(catalog: &Catalog, vid: &str) -> Result<Video, AppError> {
    db::get_video(&catalog.conn, vid)?
        .ok_or_else(|| AppError::new("not_found", format!("no video {vid}")).with("vid", vid))
}

fn require_vid(vid: &str) -> Result<String, UsageError> {
    if VID_RE.is_match(vid) {
        Ok(vid.to_string())
    } else {
        Err(UsageError::new("vid must be an 11-character YouTube id"))
    }
}

fn bounded(value: Option<i64>, default: i64, full: bool) -> Result<i64, UsageError> {
    let Some(parsed) = value else {
        return Ok(default);
    };
    if parsed < 1 {
        return Err(UsageError::new("--limit must be >= 1"));
    }
    let cap = if full { 200 } else { 50 };
    if parsed > cap {
        return Err(UsageError::new(format!("--limit max is {cap}")));
    }
    Ok(parsed)
}

fn usage_as_app(err: UsageError) -> AppError {
    let mut app = AppError::new(
        if err.flag.is_some() {
            "unknown_flag"
        } else {
            "usage"
        },
        err.detail,
    );
    if let Some(flag) = err.flag {
        app = app.with("flag", flag);
    }
    app.extra.push(("__exit".into(), "2".into()));
    app
}

fn source_line(sources: &[(String, i64)]) -> String {
    if sources.is_empty() {
        "none".into()
    } else {
        sources
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn area_description(area: &str) -> &'static str {
    match area {
        "agentic-engineering" => {
            "Coding agents, harnesses, MCP, context engineering, evals, and sandboxes"
        }
        "ai-engineering" => "LLMs, RAG, evals, fine-tuning, inference, and applied model building",
        "ai-research" => "Model research, alignment, safety, and lecture-depth ML",
        _ => "",
    }
}

fn similarity(distance: f64) -> f64 {
    round3((1.0 - distance).clamp(0.0, 1.0))
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn video_source(catalog: &Catalog, vid: &str) -> Option<String> {
    db::get_video(&catalog.conn, vid)
        .ok()
        .flatten()
        .map(|video| video.source)
}

fn area_at_least(catalog: &Catalog, vid: &str, column: &str, min: i32) -> bool {
    let sql = format!("SELECT {column} FROM videos WHERE vid = ?1");
    catalog
        .conn
        .query_row(&sql, params![vid], |row| row.get::<_, i32>(0))
        .ok()
        .is_some_and(|value| value >= min)
}

fn chunk_by_start(
    conn: &rusqlite::Connection,
    vid: &str,
    start_s: i64,
) -> Result<Option<(String, String)>, AppError> {
    let row = conn
        .query_row(
            "SELECT text, url FROM chunks WHERE vid = ?1 AND start_s = ?2 ORDER BY idx LIMIT 1",
            params![vid, start_s],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row)
}

use rusqlite::OptionalExtension;
