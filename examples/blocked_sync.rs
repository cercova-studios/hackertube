//! Benchmark the production synchronization function on an explicit disposable copy.
use hackertube::{db, vecstore};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::error::Error;
use std::io::{self, Read};
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    let conn = Connection::open(request["database"].as_str().ok_or("database required")?)?;
    vecstore::install(&conn)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA cache_size=-2048;")?;
    let vids: Vec<String> = serde_json::from_value(request["vids"].clone())?;
    let before = conn.total_changes();
    let start = Instant::now();
    db::sync_blocked(&conn, &vids)?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut counts = Vec::new();
    for table in ["videos", "vec_videos", "vec_chunks"] {
        let count: i64 = conn.query_row(
            &format!("SELECT count(*) FROM {table} WHERE blocked=1"),
            [],
            |r| r.get(0),
        )?;
        counts.push(count);
    }
    println!(
        "{}",
        json!({"ms":ms,"changes":conn.total_changes()-before,"blocked_counts":counts})
    );
    Ok(())
}
