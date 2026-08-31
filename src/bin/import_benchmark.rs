// CMS-005 Import Benchmark
//
// This benchmark intentionally mirrors the production puzzle mapping
// (CsvRow → NewPuzzle) and exists to isolate transaction overhead;
// it is not the production importer itself.

#![allow(clippy::module_inception)]

use std::fs::File;
use std::path::Path;

#[macro_use]
extern crate diesel;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

#[path = "../schema.rs"]
mod schema;
#[path = "../models.rs"]
mod models;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

const MATRIX: [(u64, &str); 4] = [
    (1000, "no-transaction"),
    (1000, "transaction"),
    (10000, "no-transaction"),
    (10000, "transaction"),
];

#[derive(serde::Deserialize)]
struct CsvRow {
    #[serde(rename = "PuzzleId")]
    puzzle_id: String,
    #[serde(rename = "FEN")]
    fen: String,
    #[serde(rename = "Moves")]
    moves: String,
    #[serde(rename = "Rating")]
    rating: i32,
    #[serde(rename = "RatingDeviation")]
    rating_deviation: i32,
    #[serde(rename = "Popularity")]
    popularity: i32,
    #[serde(rename = "NbPlays")]
    nb_plays: i32,
    #[serde(rename = "Themes")]
    themes: String,
    #[serde(rename = "GameUrl")]
    game_url: String,
    #[serde(rename = "OpeningTags")]
    opening_tags: String,
}

fn new_puzzle(row: &CsvRow) -> models::NewPuzzle<'_> {
    models::NewPuzzle {
        puzzle_id: &row.puzzle_id,
        fen: &row.fen,
        moves: &row.moves,
        rating: row.rating,
        rating_deviation: row.rating_deviation,
        popularity: row.popularity,
        nb_plays: row.nb_plays,
        themes: &row.themes,
        game_url: &row.game_url,
        opening_tags: &row.opening_tags,
    }
}

fn import_rows(
    conn: &mut SqliteConnection,
    path: &Path,
    limit: u64,
) -> Result<usize, BoxError> {
    let file = File::open(path)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let mut count = 0usize;
    for result in reader.deserialize::<CsvRow>().take(limit as usize) {
        let row = result?;
        diesel::insert_into(schema::puzzles::table)
            .values(&new_puzzle(&row))
            .execute(conn)?;
        count += 1;
    }
    Ok(count)
}

fn import_rows_transactional(
    conn: &mut SqliteConnection,
    path: &Path,
    limit: u64,
) -> Result<usize, BoxError> {
    conn.transaction(|conn| import_rows(conn, path, limit))
}

fn row_count(conn: &mut SqliteConnection) -> Result<i64, BoxError> {
    let count: i64 = schema::puzzles::table.count().get_result(conn)?;
    Ok(count)
}

fn db_path(strategy: &str, limit: u64) -> Result<std::path::PathBuf, BoxError> {
    let cwd = std::env::current_dir()?;
    Ok(cwd
        .join("target")
        .join("cms_benchmark")
        .join(format!("benchmark_{}_{}.sqlite", strategy, limit)))
}

fn open_fresh_benchmark_db(path: &Path) -> Result<SqliteConnection, BoxError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut conn = SqliteConnection::establish(
        path.to_str().ok_or("invalid db path")?,
    )?;
    conn.run_pending_migrations(MIGRATIONS)?;
    Ok(conn)
}

fn run_phase(csv: &Path, strategy: &str, limit: u64) -> Result<(), BoxError> {
    let db = db_path(strategy, limit)?;
    let mut conn = open_fresh_benchmark_db(&db)?;

    let start = std::time::Instant::now();
    let inserted = match strategy {
        "no-transaction" => import_rows(&mut conn, csv, limit)?,
        "transaction" => import_rows_transactional(&mut conn, csv, limit)?,
        other => return Err(format!("unknown strategy: {}", other).into()),
    };
    let elapsed = start.elapsed();

    let rows = row_count(&mut conn)?;

    println!("CMS-005 Import Benchmark");
    println!();
    println!("Strategy: {}", strategy);
    println!("Requested: {}", limit);
    println!("Inserted: {}", inserted);
    println!("SQLite rows: {}", rows);
    println!("Elapsed: {} ms", elapsed.as_millis());
    let secs = elapsed.as_secs_f64();
    let rate = if secs > 0.0 {
        inserted as f64 / secs
    } else {
        f64::INFINITY
    };
    println!("Rate: {:.0} puzzles/s", rate);
    println!();

    let ok = limit == inserted as u64 && inserted as i64 == rows;
    if !ok {
        return Err(format!(
            "validation failed for {} {}: requested={} inserted={} rows={}",
            strategy, limit, limit, inserted, rows
        )
        .into());
    }
    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  import_benchmark <csv>                          runs the full matrix (1000/10000 x both strategies)");
    eprintln!("  import_benchmark <csv> <limit> <strategy>       runs a single phase");
    eprintln!("    <limit>    e.g. 1000 or 10000");
    eprintln!("    <strategy> 'no-transaction' | 'transaction'");
}

fn parse_arg<T: std::str::FromStr>(value: &str, label: &str) -> Result<T, BoxError> {
    value.parse::<T>().map_err(|_| format!("invalid {}: {}", label, value).into())
}

fn main() -> Result<(), BoxError> {
    eprintln!("CMS-005 Import Benchmark (real CSV -> SQLite)");
    eprintln!();

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    let csv = std::path::PathBuf::from(&args[0]);
    if !csv.exists() {
        return Err(format!("CSV not found: {}", csv.display()).into());
    }

    if args.len() == 1 {
        eprintln!("CSV source: {}", csv.display());
        eprintln!();
        for (limit, strategy) in MATRIX {
            run_phase(&csv, strategy, limit)?;
        }
        return Ok(());
    }

    if args.len() != 3 {
        print_usage();
        std::process::exit(2);
    }

    let limit: u64 = parse_arg(&args[1], "limit")?;
    let strategy = &args[2];
    if strategy != "no-transaction" && strategy != "transaction" {
        return Err(format!("unknown strategy: {} (expected 'no-transaction' or 'transaction')", strategy).into());
    }

    eprintln!("CSV source: {}", csv.display());
    eprintln!();
    run_phase(&csv, strategy, limit)?;
    Ok(())
}
