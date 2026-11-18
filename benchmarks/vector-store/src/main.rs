//! Local benchmark worker; requests and results are retained by run.py.
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::error::Error;
use std::io::{self, Read};
use std::time::Instant;

#[cfg(all(feature = "fork", feature = "upstream"))]
compile_error!("choose exactly one extension");

fn install(conn: &Connection) -> Result<(), Box<dyn Error>> {
    #[cfg(feature = "upstream")]
    // SAFETY: upstream's untyped symbol implements this C entrypoint ABI.
    let init = unsafe {
        std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut *mut std::ffi::c_char,
                *const std::ffi::c_void,
            ) -> i32,
        >(sqlite_vec_upstream::sqlite3_vec_init as *const ())
    };
    #[cfg(feature = "fork")]
    let init = sqlite_vec_fork::sqlite3_vec_init;
    // SAFETY: the live connection and static extension share bundled SQLite.
    let rc = unsafe { init(conn.handle().cast(), std::ptr::null_mut(), std::ptr::null()) };
    if rc != rusqlite::ffi::SQLITE_OK {
        return Err(format!("registration: {rc}").into());
    }
    Ok(())
}

fn resources() -> Value {
    // SAFETY: getrusage initializes a valid, correctly sized output object.
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(rc, 0);
    let rss = usage.ru_maxrss as u64;
    json!({
        "max_rss_bytes": if cfg!(target_os = "macos") { rss } else { rss * 1024 },
        "cpu_s": usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1e6
            + usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1e6,
        "major_faults": usage.ru_majflt,
    })
}

fn blob(query: &Value) -> Vec<u8> {
    query
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|x| (x.as_f64().unwrap() as f32).to_le_bytes())
        .collect()
}

fn run(request: &Value) -> Result<Value, Box<dyn Error>> {
    let opened = Instant::now();
    let conn = Connection::open(request["database"].as_str().ok_or("database required")?)?;
    install(&conn)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA cache_size=-2048;")?;
    let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
    let operation = request["operation"].as_str().ok_or("operation required")?;
    let mut result = match operation {
        "capture" => {
            conn.execute_batch("CREATE TABLE benchmark_rows AS SELECT rowid AS rid, embedding,vid,source,shelf_life,area_agentic,area_engineering,area_research,blocked FROM vec_videos;
                CREATE UNIQUE INDEX benchmark_rows_rid ON benchmark_rows(rid);
                UPDATE benchmark_rows SET blocked=(rid % 10 = 0);
                CREATE TABLE benchmark_meta AS SELECT rid,vid,source,shelf_life,area_agentic,area_engineering,area_research,blocked FROM benchmark_rows;
                UPDATE vec_videos SET blocked=(rowid % 10 = 0);")?;
            let count: i64 =
                conn.query_row("SELECT count(*) FROM benchmark_rows", [], |r| r.get(0))?;
            json!({"rows":count})
        }
        "build" => {
            let start = Instant::now();
            let ef = request["ef"].as_u64().unwrap_or(64);
            let cache = request["cache_mb"].as_u64().unwrap_or(32);
            conn.execute_batch(&format!(
                "BEGIN; CREATE VIRTUAL TABLE ann_videos USING vec0(embedding float[384] distance_metric=cosine indexed by hnsw(m=16,ef_construction=200,ef_search={ef},cache_mb={cache}));
                 INSERT INTO ann_videos(rowid,embedding) SELECT rid,embedding FROM benchmark_rows ORDER BY rid; COMMIT;"))?;
            let elapsed = start.elapsed().as_secs_f64();
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            json!({"build_s":elapsed})
        }
        "probe" => {
            let error = conn.execute_batch("CREATE VIRTUAL TABLE unsupported USING vec0(embedding float[384] distance_metric=cosine indexed by hnsw(), blocked integer);").unwrap_err();
            json!({"metadata_error":error.to_string()})
        }
        "older_writer" => {
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                DELETE FROM vec_videos_info WHERE key GLOB 'zonemap_*';
                DELETE FROM vec_chunks_info WHERE key GLOB 'zonemap_*'; COMMIT;",
            )?;
            json!({"cleared":true})
        }
        "write" => {
            let batch = request["batch"].as_bool().unwrap();
            let rows: Vec<(i64, Vec<u8>, String)> = conn
                .prepare("SELECT rid,embedding,vid FROM benchmark_rows ORDER BY rid LIMIT 1000")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            let before = resources();
            let start = Instant::now();
            if batch {
                conn.execute_batch("BEGIN IMMEDIATE;")?;
            }
            for (id, embedding, vid) in &rows {
                conn.execute("DELETE FROM vec_videos WHERE rowid=?1", [id])?;
                conn.execute("DELETE FROM vec_videos WHERE vid=?1", [vid])?;
                conn.execute("INSERT INTO vec_videos(rowid,embedding,vid,source,shelf_life,area_agentic,area_engineering,area_research,blocked)
                    SELECT rid,?2,vid,source,shelf_life,area_agentic,area_engineering,area_research,blocked FROM benchmark_rows WHERE rid=?1",params![id,embedding])?;
            }
            if batch {
                conn.execute_batch("COMMIT;")?;
            }
            json!({"write_s":start.elapsed().as_secs_f64(),"rows":rows.len(),"before":before})
        }
        "legacy_sync" => {
            let start = Instant::now();
            conn.execute_batch("UPDATE videos SET blocked=0; UPDATE vec_videos SET blocked=0; UPDATE vec_chunks SET blocked=0;")?;
            let ids: Vec<String> = conn
                .prepare("SELECT vid FROM benchmark_rows WHERE blocked=1")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for vid in &ids {
                conn.execute("UPDATE videos SET blocked=1 WHERE vid=?1", [vid])?;
                conn.execute("UPDATE vec_videos SET blocked=1 WHERE vid=?1", [vid])?;
                conn.execute("UPDATE vec_chunks SET blocked=1 WHERE vid=?1", [vid])?;
            }
            json!({"sync_s":start.elapsed().as_secs_f64(),"marks":ids.len()})
        }
        "query" => {
            let ann = request["ann"].as_bool().unwrap_or(false);
            if ann {
                let ef = request["ef"].as_u64().unwrap_or(64);
                let cache = request["cache_mb"].as_u64().unwrap_or(32);
                conn.execute_batch(&format!("INSERT INTO ann_videos(ann_videos) VALUES('ef_search={ef}'),('cache_mb={cache}');"))?;
            }
            let queries = request["queries"].as_array().ok_or("queries required")?;
            let cases = request["cases"].as_array().ok_or("cases required")?;
            let rounds = request["rounds"].as_u64().unwrap_or(3);
            let mut samples = Vec::new();
            for round in 0..rounds {
                for (ci, case) in cases.iter().enumerate() {
                    let predicate = case["predicate"].as_str().unwrap();
                    let k = case["k"].as_i64().unwrap();
                    for (qi, query) in queries.iter().enumerate() {
                        let bytes = blob(&query["vector"]);
                        let exclude = query["exclude"].as_i64().unwrap();
                        let restriction = if predicate.is_empty() {
                            "rid != ?3".to_string()
                        } else {
                            format!("rid != ?3 AND {predicate}")
                        };
                        let unfiltered_ann = ann && predicate.is_empty();
                        let sql = if unfiltered_ann {
                            "SELECT rowid,distance FROM ann_videos WHERE embedding MATCH ?1 AND k=?2 AND ?3 IS NOT NULL ORDER BY distance".to_string()
                        } else if ann {
                            format!("SELECT rowid,distance FROM ann_videos WHERE embedding MATCH ?1 AND k=?2 AND rowid IN (SELECT rid FROM benchmark_meta WHERE {restriction}) ORDER BY distance")
                        } else {
                            let restriction = if predicate.is_empty() {
                                "vid != ?3".to_string()
                            } else {
                                format!("vid != ?3 AND {predicate}")
                            };
                            format!("SELECT rowid,distance FROM vec_videos WHERE embedding MATCH ?1 AND k=?2 AND {restriction} ORDER BY distance")
                        };
                        let start = Instant::now();
                        let identity = if ann {
                            rusqlite::types::Value::Integer(exclude)
                        } else {
                            rusqlite::types::Value::Text(query["vid"].as_str().unwrap().to_string())
                        };
                        let mut hits: Vec<(i64, f64)> = conn
                            .prepare(&sql)?
                            .query_map(
                                params![bytes, if unfiltered_ann { k + 1 } else { k }, identity],
                                |r| Ok((r.get(0)?, r.get(1)?)),
                            )?
                            .collect::<Result<_, _>>()?;
                        if unfiltered_ann {
                            hits.retain(|(rid, _)| *rid != exclude);
                            hits.truncate(k as usize);
                        }
                        let ms = start.elapsed().as_secs_f64() * 1000.0;
                        samples
                            .push(json!({"round":round,"case":ci,"query":qi,"ms":ms,"hits":hits}));
                    }
                }
            }
            json!({"samples":samples})
        }
        _ => return Err(format!("unknown operation {operation}").into()),
    };
    result["resources"] = resources();
    result["open_ms"] = json!(open_ms);
    result["sqlite_version"] = json!(rusqlite::version());
    result["vec_version"] = conn
        .query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0))?
        .into();
    Ok(result)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    println!("{}", run(&request)?);
    Ok(())
}
