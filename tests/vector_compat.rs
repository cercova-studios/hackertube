use hackertube::db::Catalog;
use hackertube::vecstore::{self, ChunkVec, KnnFilter, SqliteVec, VectorIndex, VideoVec};
use rusqlite::Connection;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn vector(x: f32, y: f32) -> Vec<f32> {
    let mut values = vec![0.0; 384];
    values[0] = x;
    values[1] = y;
    values
}

fn video(id: i64, x: f32, y: f32) -> VideoVec {
    VideoVec {
        rowid: id,
        vid: format!("{id:011}"),
        source: "aietalks".into(),
        shelf_life: "evergreen".into(),
        area_agentic: 3,
        area_engineering: 2,
        area_research: 0,
        blocked: 0,
        embedding: vector(x, y),
    }
}

fn insert(index: &SqliteVec<'_>, row: &VideoVec) {
    index.upsert_video(row).unwrap();
    index
        .upsert_chunk(&ChunkVec {
            rowid: row.rowid,
            vid: row.vid.clone(),
            start_s: row.rowid * 10,
            source: row.source.clone(),
            area_agentic: row.area_agentic,
            area_engineering: row.area_engineering,
            area_research: row.area_research,
            blocked: row.blocked,
            embedding: row.embedding.clone(),
        })
        .unwrap();
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    1.0 - dot / (norm(a) * norm(b))
}

#[test]
fn registration_is_per_connection() {
    let first = Connection::open_in_memory().unwrap();
    let second = Connection::open_in_memory().unwrap();
    vecstore::install(&first).unwrap();
    let version: String = first
        .query_row("SELECT vec_version()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, "v0.1.10-alpha.4");
    assert!(second.prepare("SELECT vec_version()").is_err());
    vecstore::install(&second).unwrap();
    SqliteVec { conn: &second }.ensure_schema().unwrap();
}

#[test]
fn cosine_and_metadata_filters_match_offline_oracle() {
    let conn = Connection::open_in_memory().unwrap();
    vecstore::install(&conn).unwrap();
    let index = SqliteVec { conn: &conn };
    index.ensure_schema().unwrap();
    let mut rows = vec![
        video(1, 3.0, 4.0),
        video(2, 1.0, 0.0),
        video(3, 0.0, 2.0),
        video(4, -1.0, 0.0),
        video(5, 1.0, 1.0),
        video(6, 1.0, 2.0),
    ];
    rows[2].source = "hackertube".into();
    rows[3].area_agentic = 0;
    rows[4].shelf_life = "news".into();
    rows[5].blocked = 1;
    for row in &rows {
        insert(&index, row);
    }
    let query = vector(2.0, 1.5);
    let mut expected: Vec<_> = rows
        .iter()
        .map(|r| (&r.vid, cosine(&query, &r.embedding)))
        .collect();
    expected.sort_by(|a, b| a.1.total_cmp(&b.1));
    let videos = index.knn_videos(&query, 20, &KnnFilter::default()).unwrap();
    let chunks = index.knn_chunks(&query, 20, &KnnFilter::default()).unwrap();
    for ((video, chunk), (vid, distance)) in videos.iter().zip(&chunks).zip(&expected) {
        assert_eq!(&video.vid, *vid);
        assert_eq!(chunk.vid, video.vid);
        assert_eq!(chunk.start_s, chunk.rowid * 10);
        assert!((video.distance - distance).abs() < 1e-6);
        assert!((chunk.distance - distance).abs() < 1e-6);
    }
    assert_eq!(videos.len(), rows.len());
    assert_eq!(chunks.len(), rows.len());
    // Each predicate excludes a different row; remaining candidates must fill k.
    let filter = KnnFilter {
        source: Some("aietalks".into()),
        area: Some("agentic-engineering".into()),
        hide_news: true,
        hide_blocked: true,
        exclude_vid: Some(rows[0].vid.clone()),
    };
    let hits = index.knn_videos(&query, 2, &filter).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].vid, rows[1].vid);
    assert_eq!(
        index.knn_videos(&query, 1, &filter).unwrap()[0].vid,
        rows[1].vid
    );
    let chunk_filter = KnnFilter {
        hide_news: false,
        ..filter.clone()
    };
    let hits = index.knn_chunks(&query, 10, &chunk_filter).unwrap();
    assert_eq!(
        hits.iter().map(|h| &h.vid).collect::<Vec<_>>(),
        vec![&rows[4].vid, &rows[1].vid]
    );
    for area in ["ai-engineering", "ai-research"] {
        let filter = KnnFilter {
            area: Some(area.into()),
            ..KnnFilter::default()
        };
        let expected = if area == "ai-engineering" { 6 } else { 0 };
        assert_eq!(
            index.knn_videos(&query, 10, &filter).unwrap().len(),
            expected
        );
        assert_eq!(
            index.knn_chunks(&query, 10, &filter).unwrap().len(),
            expected
        );
    }
    index.set_blocked(&[rows[1].vid.clone()]).unwrap();
    let hits = index.knn_videos(&query, 2, &filter).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].vid, rows[5].vid);
    index.set_blocked(&[]).unwrap();
    assert_eq!(index.knn_videos(&query, 2, &filter).unwrap().len(), 2);
    let empty = KnnFilter {
        source: Some("missing".into()),
        ..KnnFilter::default()
    };
    assert!(index.knn_chunks(&query, 10, &empty).unwrap().is_empty());
    assert!(index.knn_videos(&query, 0, &empty).unwrap().is_empty());
    assert!(index.knn_chunks(&[1.0], 1, &empty).unwrap().is_empty());
}

#[test]
fn replacement_deletion_and_nested_rollback_preserve_both_indexes() {
    let conn = Connection::open_in_memory().unwrap();
    vecstore::install(&conn).unwrap();
    let index = SqliteVec { conn: &conn };
    index.ensure_schema().unwrap();
    let original = video(1, 1.0, 0.0);
    insert(&index, &original);
    conn.execute_batch("BEGIN; SAVEPOINT outer_scope;").unwrap();
    insert(&index, &video(1, 0.0, 1.0));
    conn.execute_batch("SAVEPOINT inner_scope;").unwrap();
    let mut duplicate = video(2, -1.0, 0.0);
    duplicate.vid.clone_from(&original.vid);
    index.upsert_video(&duplicate).unwrap();
    conn.execute_batch("DELETE FROM vec_chunks WHERE rowid=1; RELEASE inner_scope;")
        .unwrap();
    assert!(index
        .knn_chunks(&original.embedding, 5, &KnnFilter::default())
        .unwrap()
        .is_empty());
    let hits = index
        .knn_videos(&original.embedding, 5, &KnnFilter::default())
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!((hits[0].distance - 2.0).abs() < 1e-6);
    conn.execute_batch("ROLLBACK TO outer_scope; RELEASE outer_scope; COMMIT;")
        .unwrap();
    for table in ["vec_videos", "vec_chunks"] {
        let rowid: i64 = conn
            .query_row(&format!("SELECT rowid FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(rowid, 1);
    }
    conn.execute_batch("BEGIN;").unwrap();
    insert(&index, &video(1, 0.0, 1.0));
    // Read-your-writes must flush the fork's buffered vector writes.
    assert!(
        (index
            .knn_chunks(&original.embedding, 1, &KnnFilter::default())
            .unwrap()[0]
            .distance
            - 1.0)
            .abs()
            < 1e-6
    );
    conn.execute_batch("ROLLBACK;").unwrap();
    assert!(
        index
            .knn_videos(&original.embedding, 1, &KnnFilter::default())
            .unwrap()[0]
            .distance
            .abs()
            < 1e-6
    );
    assert!(
        index
            .knn_chunks(&original.embedding, 1, &KnnFilter::default())
            .unwrap()[0]
            .distance
            .abs()
            < 1e-6
    );
}

fn upstream(binary: &Path, database: &Path, sql: &str) {
    let mut child = Command::new(binary)
        .arg(database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(sql.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "upstream: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn upstream_cosine_catalog_reopens_with_fork_and_safe_older_writer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = root.join("target/upstream-compat");
    let output = Command::new(env!("CARGO"))
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(root.join("tests/upstream/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "upstream build: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = target.join(format!(
        "debug/hackertube-upstream-compat{}",
        std::env::consts::EXE_SUFFIX
    ));
    let home = std::env::temp_dir().join(format!("ht-vec-compat-{}", std::process::id()));
    std::fs::create_dir(&home).unwrap();
    let database = home.join("catalog.sqlite");
    // Derive the unchanged production schema; upstream creates its own file.
    let schema: Vec<String> = {
        let catalog = Catalog::open(&home.join("schema")).unwrap();
        let mut stmt = catalog.conn.prepare("SELECT sql FROM sqlite_master WHERE name IN ('vec_videos', 'vec_chunks') ORDER BY name").unwrap();
        let rows = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows
    };
    let blob = hex::encode(vecstore::f32_bytes(&vector(3.0, 4.0)));
    upstream(&binary, &database, &format!(
        "{}; CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO meta VALUES('schema_version','2');
         INSERT INTO vec_videos(rowid,embedding,vid,source,shelf_life,area_agentic,area_engineering,area_research,blocked)
         VALUES(1,x'{blob}','00000000001','aietalks','evergreen',3,2,0,0);
         INSERT INTO vec_chunks(rowid,embedding,vid,start_s,source,area_agentic,area_engineering,area_research,blocked)
         VALUES(1,x'{blob}','00000000001',10,'aietalks',3,2,0,0);", schema.join(";")));
    let marks = b"{\"version\":1,\"items\":{}}\n";
    std::fs::write(home.join("state.json"), marks).unwrap();
    {
        let catalog = Catalog::open(&home).unwrap();
        let index = SqliteVec {
            conn: &catalog.conn,
        };
        assert!(
            index
                .knn_videos(&vector(3.0, 4.0), 2, &KnnFilter::default())
                .unwrap()[0]
                .distance
                .abs()
                < 1e-6
        );
        assert!(
            index
                .knn_chunks(&vector(3.0, 4.0), 2, &KnnFilter::default())
                .unwrap()[0]
                .distance
                .abs()
                < 1e-6
        );
        insert(&index, &video(2, 1.0, 0.0));
        catalog
            .conn
            .execute_batch(
                "INSERT INTO videos(vid,source,url,title) VALUES
             ('00000000001','aietalks','https://www.youtube.com/watch?v=00000000001','query'),
             ('00000000002','aietalks','https://www.youtube.com/watch?v=00000000002','neighbor');",
            )
            .unwrap();
        hackertube::db::store_embedding(&catalog.conn, "00000000001", &vector(3.0, 4.0)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_hackertube"))
        .args(["similar", "00000000001", "--json"])
        .env("HOME", &home)
        .env("HACKERTUBE_HOME", &home)
        .env("HACKERTUBE_EMBED", "off")
        .env("HACKERTUBE_CAPTIONS", "off")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["mode"], "embedding");
    assert_eq!(doc["videos"][0]["vid"], "00000000002");
    assert_eq!(doc["videos"][0]["sim"], 0.6);
    assert!(doc["help"].is_array());
    let catalog = Catalog::open(&home).unwrap();
    let index = SqliteVec {
        conn: &catalog.conn,
    };
    assert_eq!(
        index
            .knn_videos(&vector(1.0, 0.0), 5, &KnnFilter::default())
            .unwrap()[0]
            .vid,
        "00000000002"
    );
    drop(catalog);
    upstream(
        &binary,
        &database,
        "CREATE TEMP TABLE assertions(ok INTEGER CHECK(ok=1));
         INSERT INTO assertions SELECT count(*)=2 FROM vec_videos;
         INSERT INTO assertions SELECT count(*)=2 FROM vec_chunks;",
    );
    // Also qualify files created by the fork, whose fresh chunks have zone maps.
    let downgrade_home = home.join("fork-created");
    let catalog = Catalog::open(&downgrade_home).unwrap();
    let index = SqliteVec {
        conn: &catalog.conn,
    };
    insert(&index, &video(1, 3.0, 4.0));
    insert(&index, &video(2, 1.0, 0.0));
    let database = catalog.path.clone();
    // Clear fork-only pruning bounds before an older writer changes metadata.
    catalog.conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    for table in ["vec_videos", "vec_chunks"] {
        let deleted = catalog
            .conn
            .execute(
                &format!("DELETE FROM {table}_info WHERE key GLOB 'zonemap_*'"),
                [],
            )
            .unwrap();
        assert!(deleted > 0, "fresh {table} must exercise zone-map cleanup");
    }
    catalog.conn.execute_batch("COMMIT;").unwrap();
    drop(catalog);
    upstream(
        &binary,
        &database,
        &format!("CREATE TEMP TABLE assertions(ok INTEGER CHECK(ok=1));
         INSERT INTO assertions SELECT count(*)=2 FROM vec_videos;
         INSERT INTO assertions SELECT count(*)=2 FROM vec_chunks;
         INSERT INTO assertions SELECT abs(distance)<0.000001 FROM vec_videos WHERE embedding MATCH x'{blob}' AND k=1;
         UPDATE vec_videos SET area_agentic=1 WHERE rowid=2;
         UPDATE vec_chunks SET area_agentic=1 WHERE rowid=2;"),
    );
    let catalog = Catalog::open(&downgrade_home).unwrap();
    let index = SqliteVec {
        conn: &catalog.conn,
    };
    let filter = KnnFilter {
        area: Some("agentic-engineering".into()),
        ..KnnFilter::default()
    };
    assert_eq!(
        index
            .knn_videos(&vector(1.0, 0.0), 5, &filter)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        index
            .knn_chunks(&vector(1.0, 0.0), 5, &filter)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(std::fs::read(home.join("state.json")).unwrap(), marks);
    drop(catalog);
    std::fs::remove_dir_all(home).unwrap();
}
