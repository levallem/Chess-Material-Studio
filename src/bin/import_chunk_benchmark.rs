// CMS-010 — Chunk Size & Import Throughput Benchmark
//
// Compares chunk_size on a 1M-row import from a real Lichess CSV.
// Each chunk size gets an independent SQLite under target/cms_benchmark/.
//
// This is a benchmark only — no production defaults are modified.

use std::path::{Path, PathBuf};

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::MigrationHarness;

use offline_chess_puzzles::puzzle_import::{
    import_puzzles_from_reader_chunked_limited, puzzle_source_key_from_file, MIGRATIONS,
};
use offline_chess_puzzles::schema;

const CHUNK_SIZES: &[usize] = &[
    1_000,
    5_000,
    10_000,
    25_000,
    50_000,
    100_000,
];

const MAX_ROWS: usize = 1_000_000;

// ── CLI ────────────────────────────────────────────────────────────────

struct Args {
    csv: PathBuf,
    rows: usize,
}

enum ParseOutcome {
    Run(Args),
    Help,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  import_chunk_benchmark --csv <PATH> --rows <N>");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  --csv <PATH>   Path to the Lichess CSV file (required)");
    eprintln!("  --rows <N>     Number of rows to import per chunk config (required, 1..=1000000)");
}

fn parse_args_from(args: &[String]) -> Result<ParseOutcome, String> {
    if args.is_empty() {
        return Err("no arguments provided".into());
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }

    let mut csv: Option<PathBuf> = None;
    let mut rows: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => {
                i += 1;
                csv = Some(PathBuf::from(args.get(i).ok_or("--csv requires a value")?));
            }
            "--rows" => {
                i += 1;
                let val = args.get(i).ok_or("--rows requires a value")?;
                let n: usize = val.parse().map_err(|_| format!("invalid --rows: {}", val))?;
                rows = Some(n);
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }

    let csv = csv.ok_or("missing required argument: --csv")?;
    let rows = rows.ok_or("missing required argument: --rows")?;

    if rows == 0 {
        return Err("rows must be greater than 0".into());
    }
    if rows > MAX_ROWS {
        return Err(format!(
            "rows limit: maximum is {} (got {})",
            MAX_ROWS, rows
        ));
    }

    Ok(ParseOutcome::Run(Args { csv, rows }))
}

fn parse_args() -> Result<ParseOutcome, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(&args)
}

// ── Helpers ────────────────────────────────────────────────────────────

fn ceiling_div(n: usize, d: usize) -> usize {
    (n + d - 1) / d
}

fn row_count(conn: &mut SqliteConnection) -> Result<i64, String> {
    schema::puzzles::table
        .count()
        .get_result::<i64>(conn)
        .map_err(|e| format!("count query failed: {}", e))
}

fn checkpoint_value(conn: &mut SqliteConnection, source_key: &str) -> Result<i64, String> {
    schema::puzzle_import_progress::table
        .filter(schema::puzzle_import_progress::dsl::source_key.eq(source_key))
        .select(schema::puzzle_import_progress::dsl::completed_rows)
        .first::<i64>(conn)
        .optional()
        .map_err(|e| format!("checkpoint query failed: {}", e))?
        .ok_or_else(|| format!("checkpoint not found for source_key: {}", source_key))
}

fn scratch_dir() -> Result<PathBuf, String> {
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system time error: {}", e))?
        .as_secs();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest
        .join("target")
        .join("cms_benchmark")
        .join(format!("cms010_{}_{}", pid, ts)))
}

fn db_path(dir: &Path, chunk_size: usize) -> PathBuf {
    dir.join(format!("chunk_{}.sqlite", chunk_size))
}

fn validate_scratch_db_count(actual: usize) -> Result<(), String> {
    if actual != CHUNK_SIZES.len() {
        return Err(format!(
            "scratch DB count mismatch: expected {}, got {}",
            CHUNK_SIZES.len(),
            actual
        ));
    }
    Ok(())
}

fn open_fresh_db(path: &Path) -> Result<SqliteConnection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create DB directory: {}", e))?;
    }
    let path_str = path.to_str().ok_or("invalid db path (non-UTF-8)")?;
    let mut conn =
        SqliteConnection::establish(path_str).map_err(|e| format!("cannot open DB: {}", e))?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("migration failed: {}", e))?;
    Ok(conn)
}

// ── Benchmark result ───────────────────────────────────────────────────

struct BenchResult {
    chunk_size: usize,
    #[allow(dead_code)]
    rows: usize,
    transaction_count: usize,
    import_ms: u128,
    import_rows_per_second: f64,
    estimated_e2e_ms: u128,
    estimated_e2e_rows_per_second: f64,
    db_size_bytes: u64,
    projected_6m_import_seconds: f64,
    projected_6m_e2e_seconds: f64,
}

// ── Recommendation logic (exposed for testing) ────────────────────────

fn choose_recommended(results: &[BenchResult]) -> Option<&BenchResult> {
    if results.is_empty() {
        return None;
    }

    let fastest_rps = results
        .iter()
        .map(|r| r.import_rows_per_second)
        .fold(0.0_f64, f64::max);

    let threshold = fastest_rps * 0.95;

    // Among candidates >= 95% of fastest, pick the smallest chunk_size.
    results
        .iter()
        .filter(|r| r.import_rows_per_second >= threshold)
        .min_by_key(|r| r.chunk_size)
}

// ── Main ───────────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let args = match parse_args()? {
        ParseOutcome::Help => {
            print_usage();
            return Ok(());
        }
        ParseOutcome::Run(a) => a,
    };

    if !args.csv.exists() {
        return Err(format!("CSV not found: {}", args.csv.display()));
    }

    // ── Fingerprint (once) ──────────────────────────────────────────

    let fp_start = std::time::Instant::now();
    let source_key = puzzle_source_key_from_file(&args.csv)
        .map_err(|e| format!("fingerprint failed: {}", e))?;
    let fingerprint_ms = fp_start.elapsed().as_millis();

    let csv_size = std::fs::metadata(&args.csv)
        .map_err(|e| format!("cannot stat CSV: {}", e))?
        .len();

    // ── Scratch directory ───────────────────────────────────────────

    let scratch = scratch_dir()?;
    std::fs::create_dir_all(&scratch)
        .map_err(|e| format!("cannot create scratch dir: {}", e))?;

    eprintln!("CMS-010 Chunk Benchmark");
    eprintln!();
    eprintln!("CSV: {}", args.csv.display());
    eprintln!("CSV size: {} bytes", csv_size);
    eprintln!("rows: {}", args.rows);
    eprintln!("source_key: {}", source_key);
    eprintln!("fingerprint_ms: {}", fingerprint_ms);
    eprintln!("benchmark_type: warm-source");
    eprintln!("scratch: {}", scratch.display());
    eprintln!();

    // ── Run benchmarks ──────────────────────────────────────────────

    let mut results: Vec<BenchResult> = Vec::with_capacity(CHUNK_SIZES.len());

    for &chunk_size in CHUNK_SIZES {
        let db = db_path(&scratch, chunk_size);
        let mut conn = open_fresh_db(&db)?;

        // Verify empty
        let initial_rows = row_count(&mut conn)?;
        if initial_rows != 0 {
            return Err(format!(
                "DB for chunk={} should start with 0 rows, got {}",
                chunk_size, initial_rows
            ));
        }

        // Open CSV for import (excludes fingerprint from timing)
        let file = std::fs::File::open(&args.csv)
            .map_err(|e| format!("cannot open CSV for import: {}", e))?;

        let import_start = std::time::Instant::now();
        let inserted = import_puzzles_from_reader_chunked_limited(
            &mut conn,
            file,
            &source_key,
            chunk_size,
            args.rows,
        )
        .map_err(|e| format!("import failed for chunk={}: {}", chunk_size, e))?;
        let import_ms = import_start.elapsed().as_millis();

        // ── Verifications ──────────────────────────────────────────

        if inserted != args.rows {
            return Err(format!(
                "chunk={}: inserted_rows {} != requested {}",
                chunk_size, inserted, args.rows
            ));
        }

        let final_rows = row_count(&mut conn)?;
        if final_rows != args.rows as i64 {
            return Err(format!(
                "chunk={}: final_rows {} != requested {}",
                chunk_size, final_rows, args.rows
            ));
        }

        let final_checkpoint = checkpoint_value(&mut conn, &source_key)?;
        if final_checkpoint != args.rows as i64 {
            return Err(format!(
                "chunk={}: checkpoint {} != requested {}",
                chunk_size, final_checkpoint, args.rows
            ));
        }

        // ── Metrics ────────────────────────────────────────────────

        let transaction_count = ceiling_div(args.rows, chunk_size);
        let import_seconds = import_ms as f64 / 1000.0;
        let import_rows_per_second = if import_seconds > 0.0 {
            args.rows as f64 / import_seconds
        } else {
            f64::INFINITY
        };

        let fingerprint_seconds = fingerprint_ms as f64 / 1000.0;
        let estimated_e2e_ms = fingerprint_ms + import_ms;
        let estimated_e2e_rows_per_second = if (fingerprint_seconds + import_seconds) > 0.0 {
            args.rows as f64 / (fingerprint_seconds + import_seconds)
        } else {
            f64::INFINITY
        };

        let db_size_bytes = std::fs::metadata(&db)
            .map_err(|e| format!("cannot stat DB: {}", e))?
            .len();

        let projected_6m_import_seconds = if import_rows_per_second > 0.0 {
            6_000_000.0 / import_rows_per_second
        } else {
            f64::INFINITY
        };
        let projected_6m_e2e_seconds = fingerprint_seconds + projected_6m_import_seconds;

        results.push(BenchResult {
            chunk_size,
            rows: args.rows,
            transaction_count,
            import_ms,
            import_rows_per_second,
            estimated_e2e_ms,
            estimated_e2e_rows_per_second,
            db_size_bytes,
            projected_6m_import_seconds,
            projected_6m_e2e_seconds,
        });

        eprintln!(
            "  chunk={:>7}  txns={:>5}  import={:>8}ms  rows/s={:>10.0}  e2e={:>8}ms  db={:>12} bytes",
            chunk_size, transaction_count, import_ms, import_rows_per_second,
            estimated_e2e_ms, db_size_bytes
        );
    }

    // ── Source integrity check ──────────────────────────────────────

    let source_key_after = puzzle_source_key_from_file(&args.csv)
        .map_err(|e| format!("final fingerprint failed: {}", e))?;
    if source_key != source_key_after {
        return Err(format!(
            "SOURCE INTEGRITY VIOLATION: before={} after={}",
            source_key, source_key_after
        ));
    }

    // ── Recommendation ──────────────────────────────────────────────

    let recommended = choose_recommended(&results)
        .ok_or("no results to recommend from")?;

    let fastest = results
        .iter()
        .max_by(|a, b| {
            a.import_rows_per_second
                .partial_cmp(&b.import_rows_per_second)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    let diff_pct = if fastest.import_rows_per_second > 0.0 {
        ((fastest.import_rows_per_second - recommended.import_rows_per_second)
            / fastest.import_rows_per_second)
            * 100.0
    } else {
        0.0
    };

    // ── Print results table ─────────────────────────────────────────

    println!();
    println!("CMS-010 CHUNK BENCHMARK");
    println!();
    println!("CSV:             {}", args.csv.display());
    println!("CSV size:        {} bytes", csv_size);
    println!("rows:            {}", args.rows);
    println!("source_key:      {}", source_key);
    println!("fingerprint_ms:  {}", fingerprint_ms);
    println!("benchmark_type:  warm-source");
    println!();
    println!(
        "{:>7} {:>5} {:>10} {:>10} {:>10} {:>14} {:>12} {:>16}",
        "chunk", "txns", "import_ms", "rows/s", "e2e_ms",
        "e2e_rows/s", "db_bytes", "proj_6m_s"
    );
    for r in &results {
        println!(
            "{:>7} {:>5} {:>10} {:>10.0} {:>10} {:>14.0} {:>12} {:>16.1}",
            r.chunk_size,
            r.transaction_count,
            r.import_ms,
            r.import_rows_per_second,
            r.estimated_e2e_ms,
            r.estimated_e2e_rows_per_second,
            r.db_size_bytes,
            r.projected_6m_import_seconds,
        );
    }

    println!();
    println!("fastest_chunk:              {}", fastest.chunk_size);
    println!(
        "fastest_rows_per_second:    {:.0}",
        fastest.import_rows_per_second
    );
    println!();
    println!("recommended_chunk:          {}", recommended.chunk_size);
    println!(
        "recommended_rows_per_second:{:.0}",
        recommended.import_rows_per_second
    );
    println!("difference_from_fastest:    {:.1}%", diff_pct);
    println!("reason:                     smallest chunk within 5% of fastest");
    println!();
    println!("projected_6m_import_seconds:    {:.1}", recommended.projected_6m_import_seconds);
    println!("projected_6m_e2e_seconds:       {:.1}", recommended.projected_6m_e2e_seconds);
    println!("warning: linear estimate only — not a measured 6M import");

    // ── Source integrity output ──────────────────────────────────────

    println!();
    println!("source_key_before: {}", source_key);
    println!("source_key_after:  {}", source_key_after);
    println!("source_unchanged:  YES");

    // ── Scratch summary ─────────────────────────────────────────────

    println!();
    println!("scratch directory: {}", scratch.display());
    let sqlite_files: Vec<_> = std::fs::read_dir(&scratch)
        .map_err(|e| format!("cannot read scratch dir: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "sqlite").unwrap_or(false))
        .collect();
    let total_size: u64 = sqlite_files
        .iter()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    println!("scratch db count: {}", sqlite_files.len());
    println!("scratch total size: {} bytes", total_size);

    validate_scratch_db_count(sqlite_files.len())?;

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Valid parse
    #[test]
    fn test_parse_full_args() {
        let args = vec![
            "--csv".into(),
            "puzzles/lichess_db_puzzle.csv".into(),
            "--rows".into(),
            "1000000".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.csv, PathBuf::from("puzzles/lichess_db_puzzle.csv"));
                assert_eq!(parsed.rows, 1_000_000);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    // 2. --help
    #[test]
    fn test_parse_help() {
        let args = vec!["--help".into()];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Help => {}
            ParseOutcome::Run(_) => panic!("expected Help, got Run"),
        }
    }

    #[test]
    fn test_parse_help_short() {
        let args = vec!["-h".into()];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Help => {}
            ParseOutcome::Run(_) => panic!("expected Help, got Run"),
        }
    }

    // 3. Missing --csv
    #[test]
    fn test_parse_missing_csv() {
        let args = vec!["--rows".into(), "1000".into()];
        assert!(parse_args_from(&args).is_err());
    }

    // 4. Missing --rows
    #[test]
    fn test_parse_missing_rows() {
        let args = vec!["--csv".into(), "data.csv".into()];
        assert!(parse_args_from(&args).is_err());
    }

    // 5. rows=0
    #[test]
    fn test_parse_rows_zero() {
        let args = vec!["--csv".into(), "data.csv".into(), "--rows".into(), "0".into()];
        assert!(parse_args_from(&args).is_err());
    }

    // 6. rows=1000001 (over limit)
    #[test]
    fn test_parse_rows_over_limit() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--rows".into(),
            "1000001".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 7. Unknown argument
    #[test]
    fn test_parse_unknown_argument() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--rows".into(),
            "1000".into(),
            "--bogus".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 8. Ceiling division
    #[test]
    fn test_ceiling_div_exact() {
        assert_eq!(ceiling_div(10, 10), 1);
        assert_eq!(ceiling_div(100, 10), 10);
    }

    #[test]
    fn test_ceiling_div_remainder() {
        assert_eq!(ceiling_div(11, 10), 2);
        assert_eq!(ceiling_div(1, 10), 1);
        assert_eq!(ceiling_div(999, 100), 10);
    }

    #[test]
    fn test_ceiling_div_one_million() {
        assert_eq!(ceiling_div(1_000_000, 1_000), 1_000);
        assert_eq!(ceiling_div(1_000_000, 5_000), 200);
        assert_eq!(ceiling_div(1_000_000, 10_000), 100);
        assert_eq!(ceiling_div(1_000_000, 25_000), 40);
        assert_eq!(ceiling_div(1_000_000, 50_000), 20);
        assert_eq!(ceiling_div(1_000_000, 100_000), 10);
    }

    // 9. Recommendation: fastest wins clearly
    #[test]
    fn test_recommendation_fastest_wins() {
        let results = vec![
            BenchResult {
                chunk_size: 1_000,
                rows: 1_000_000,
                transaction_count: 1_000,
                import_ms: 20_000,
                import_rows_per_second: 50_000.0,
                estimated_e2e_ms: 25_000,
                estimated_e2e_rows_per_second: 40_000.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 120.0,
                projected_6m_e2e_seconds: 125.0,
            },
            BenchResult {
                chunk_size: 10_000,
                rows: 1_000_000,
                transaction_count: 100,
                import_ms: 10_000,
                import_rows_per_second: 100_000.0,
                estimated_e2e_ms: 15_000,
                estimated_e2e_rows_per_second: 66_666.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 60.0,
                projected_6m_e2e_seconds: 65.0,
            },
            BenchResult {
                chunk_size: 100_000,
                rows: 1_000_000,
                transaction_count: 10,
                import_ms: 12_000,
                import_rows_per_second: 83_333.0,
                estimated_e2e_ms: 17_000,
                estimated_e2e_rows_per_second: 58_823.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 72.0,
                projected_6m_e2e_seconds: 77.0,
            },
        ];
        let rec = choose_recommended(&results).unwrap();
        assert_eq!(rec.chunk_size, 10_000);
    }

    // 10. Recommendation: within 5% picks smaller
    #[test]
    fn test_recommendation_within_5pct_picks_smaller() {
        let results = vec![
            BenchResult {
                chunk_size: 10_000,
                rows: 1_000_000,
                transaction_count: 100,
                import_ms: 10_000,
                import_rows_per_second: 110_000.0,
                estimated_e2e_ms: 15_000,
                estimated_e2e_rows_per_second: 66_666.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 54.5,
                projected_6m_e2e_seconds: 59.5,
            },
            BenchResult {
                chunk_size: 25_000,
                rows: 1_000_000,
                transaction_count: 40,
                import_ms: 9_800,
                import_rows_per_second: 113_000.0,
                estimated_e2e_ms: 14_800,
                estimated_e2e_rows_per_second: 67_567.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 53.1,
                projected_6m_e2e_seconds: 58.1,
            },
            BenchResult {
                chunk_size: 50_000,
                rows: 1_000_000,
                transaction_count: 20,
                import_ms: 9_700,
                import_rows_per_second: 114_000.0,
                estimated_e2e_ms: 14_700,
                estimated_e2e_rows_per_second: 68_027.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 52.6,
                projected_6m_e2e_seconds: 57.6,
            },
        ];
        let rec = choose_recommended(&results).unwrap();
        // 10000 is 110k/s, fastest is 114k/s => 110/114 = 96.5% >= 95%
        assert_eq!(rec.chunk_size, 10_000);
    }

    // 11. Recommendation: exact tie picks smallest
    #[test]
    fn test_recommendation_exact_tie() {
        let results = vec![
            BenchResult {
                chunk_size: 1_000,
                rows: 1_000_000,
                transaction_count: 1_000,
                import_ms: 10_000,
                import_rows_per_second: 100_000.0,
                estimated_e2e_ms: 15_000,
                estimated_e2e_rows_per_second: 66_666.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 60.0,
                projected_6m_e2e_seconds: 65.0,
            },
            BenchResult {
                chunk_size: 10_000,
                rows: 1_000_000,
                transaction_count: 100,
                import_ms: 10_000,
                import_rows_per_second: 100_000.0,
                estimated_e2e_ms: 15_000,
                estimated_e2e_rows_per_second: 66_666.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 60.0,
                projected_6m_e2e_seconds: 65.0,
            },
            BenchResult {
                chunk_size: 100_000,
                rows: 1_000_000,
                transaction_count: 10,
                import_ms: 10_000,
                import_rows_per_second: 100_000.0,
                estimated_e2e_ms: 15_000,
                estimated_e2e_rows_per_second: 66_666.0,
                db_size_bytes: 0,
                projected_6m_import_seconds: 60.0,
                projected_6m_e2e_seconds: 65.0,
            },
        ];
        let rec = choose_recommended(&results).unwrap();
        assert_eq!(rec.chunk_size, 1_000);
    }

    // 12. Empty results returns None
    #[test]
    fn test_recommendation_empty() {
        assert!(choose_recommended(&[]).is_none());
    }

    // 13. rows at exact boundary (1_000_000)
    #[test]
    fn test_parse_rows_exact_limit() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--rows".into(),
            "1000000".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => assert_eq!(parsed.rows, 1_000_000),
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 14. rows=1 (minimum valid)
    #[test]
    fn test_parse_rows_minimum() {
        let args = vec!["--csv".into(), "data.csv".into(), "--rows".into(), "1".into()];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => assert_eq!(parsed.rows, 1),
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 15. Multiple unknown args
    #[test]
    fn test_parse_multiple_unknown() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--rows".into(),
            "1000".into(),
            "--foo".into(),
            "--bar".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 16. validate_scratch_db_count: exact 6 → Ok
    #[test]
    fn test_validate_scratch_db_count_exact() {
        assert!(validate_scratch_db_count(CHUNK_SIZES.len()).is_ok());
    }

    // 17. validate_scratch_db_count: 5 → Err
    #[test]
    fn test_validate_scratch_db_count_too_few() {
        assert!(validate_scratch_db_count(5).is_err());
    }

    // 18. validate_scratch_db_count: 7 → Err
    #[test]
    fn test_validate_scratch_db_count_too_many() {
        assert!(validate_scratch_db_count(7).is_err());
    }
}
