use std::path::PathBuf;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::MigrationHarness;

use offline_chess_puzzles::puzzle_import::{
    self, MIGRATIONS, PuzzleFileImportResult,
};

const MAX_SAFETY_LIMIT: usize = 100_000;

struct Args {
    csv: PathBuf,
    db: PathBuf,
    max_rows: usize,
    chunk_size: usize,
    resume: bool,
}

enum ParseOutcome {
    Run(Args),
    Help,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  import_puzzles --csv <PATH> --db <PATH> --max-rows <N> [--chunk-size <N>] [--resume]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  --csv <PATH>         Path to the Lichess CSV file (required)");
    eprintln!("  --db <PATH>          Path to the SQLite database (required)");
    eprintln!("  --max-rows <N>       Maximum total rows to import (required, 1..=100000)");
    eprintln!("  --chunk-size <N>     Rows per transaction chunk (optional, default: 10000)");
    eprintln!("  --resume             Resume an existing import (required if DB exists)");
}

fn parse_args_from(args: &[String]) -> Result<ParseOutcome, String> {
    if args.is_empty() {
        return Err("no arguments provided".into());
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }

    let mut csv: Option<PathBuf> = None;
    let mut db: Option<PathBuf> = None;
    let mut max_rows: Option<usize> = None;
    let mut chunk_size: Option<usize> = None;
    let mut resume = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => {
                i += 1;
                csv = Some(PathBuf::from(args.get(i).ok_or("--csv requires a value")?));
            }
            "--db" => {
                i += 1;
                db = Some(PathBuf::from(args.get(i).ok_or("--db requires a value")?));
            }
            "--max-rows" => {
                i += 1;
                let val = args.get(i).ok_or("--max-rows requires a value")?;
                let n: usize = val.parse().map_err(|_| format!("invalid --max-rows: {}", val))?;
                max_rows = Some(n);
            }
            "--chunk-size" => {
                i += 1;
                let val = args.get(i).ok_or("--chunk-size requires a value")?;
                let n: usize = val.parse().map_err(|_| format!("invalid --chunk-size: {}", val))?;
                chunk_size = Some(n);
            }
            "--resume" => {
                resume = true;
            }
            other => {
                return Err(format!("Unknown argument: {}", other));
            }
        }
        i += 1;
    }

    let csv = csv.ok_or("Missing required argument: --csv")?;
    let db = db.ok_or("Missing required argument: --db")?;
    let max_rows = max_rows.ok_or("Missing required argument: --max-rows")?;

    if max_rows == 0 {
        return Err("max_rows must be greater than 0".into());
    }
    if max_rows > MAX_SAFETY_LIMIT {
        return Err(format!(
            "CMS-009 safety limit: --max-rows cannot exceed {}",
            MAX_SAFETY_LIMIT
        ));
    }

    let chunk_size = chunk_size.unwrap_or(10_000);
    if chunk_size == 0 {
        return Err("chunk_size must be greater than 0".into());
    }

    Ok(ParseOutcome::Run(Args {
        csv,
        db,
        max_rows,
        chunk_size,
        resume,
    }))
}

fn parse_args() -> Result<ParseOutcome, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(&args)
}

fn canonicalize_if_exists(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.exists() {
        std::fs::canonicalize(path)
            .map_err(|e| format!("cannot canonicalize {}: {}", path.display(), e))
    } else {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("cannot get cwd: {}", e))?
                .join(path)
        };
        Ok(abs)
    }
}

/// Check whether a given path resolves to the project's `ocp.db`.
///
/// Anchored to `CARGO_MANIFEST_DIR`, not the current working directory.
fn is_ocp_db(path: &std::path::Path) -> bool {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_ocp = project_root.join("ocp.db");

    let target = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(_) => {
            let abs = if path.is_absolute() {
                path.to_path_buf()
            } else {
                match std::env::current_dir() {
                    Ok(cwd) => cwd.join(path),
                    Err(_) => return false,
                }
            };
            abs
        }
    };

    let protected = match std::fs::canonicalize(&project_ocp) {
        Ok(c) => c,
        Err(_) => project_ocp,
    };

    target == protected
}

fn validate_args(args: &Args) -> Result<(), String> {
    let csv_canon = canonicalize_if_exists(&args.csv)?;
    let db_canon = canonicalize_if_exists(&args.db)?;

    if csv_canon == db_canon {
        return Err("--csv and --db cannot resolve to the same file".into());
    }

    if is_ocp_db(&args.db) {
        return Err("Refusing to use protected database: ocp.db".into());
    }

    if args.db.exists() && !args.resume {
        return Err("Database already exists. Use --resume explicitly.".into());
    }

    if !args.db.exists() && args.resume {
        return Err("Cannot resume: database does not exist".into());
    }

    Ok(())
}

fn establish_connection(db_path: &std::path::Path) -> Result<SqliteConnection, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create DB directory: {}", e))?;
    }
    let path_str = db_path.to_str().ok_or("invalid DB path (non-UTF-8)")?;
    SqliteConnection::establish(path_str).map_err(|e| format!("cannot open DB: {}", e))
}

fn run_migrations(conn: &mut SqliteConnection) -> Result<(), String> {
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("migration failed: {}", e))?;
    Ok(())
}

fn row_count(conn: &mut SqliteConnection) -> Result<i64, String> {
    use offline_chess_puzzles::schema::puzzles;
    puzzles::table
        .count()
        .get_result::<i64>(conn)
        .map_err(|e| format!("count query failed: {}", e))
}

fn checkpoint_value(conn: &mut SqliteConnection, source_key: &str) -> Result<i64, String> {
    use offline_chess_puzzles::schema::puzzle_import_progress;
    puzzle_import_progress::table
        .filter(puzzle_import_progress::dsl::source_key.eq(source_key))
        .select(puzzle_import_progress::dsl::completed_rows)
        .first::<i64>(conn)
        .optional()
        .map_err(|e| format!("checkpoint query failed: {}", e))?
        .ok_or_else(|| format!("checkpoint not found for source_key: {}", source_key))
}

fn main() {
    match parse_args() {
        Ok(ParseOutcome::Help) => {
            print_usage();
            return;
        }
        Ok(ParseOutcome::Run(args)) => {
            if let Err(e) = validate_args(&args) {
                eprintln!("Error: {}", e);
                std::process::exit(2);
            }

            let mut conn = match establish_connection(&args.db) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = run_migrations(&mut conn) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }

            let starting_rows = match row_count(&mut conn) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };

            let mode = if args.resume { "resume" } else { "new" };

            // Measure full-file fingerprint + limited import together (CMS-007 design).
            let start = std::time::Instant::now();
            let result: PuzzleFileImportResult =
                match puzzle_import::import_puzzles_from_file_chunked_limited(
                    &mut conn,
                    &args.csv,
                    args.chunk_size,
                    args.max_rows,
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Import error: {}", e);
                        std::process::exit(1);
                    }
                };
            let elapsed = start.elapsed();

            let final_rows = match row_count(&mut conn) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };

            let final_checkpoint = match checkpoint_value(&mut conn, &result.source_key) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };

            // Post-import invariants
            if final_checkpoint > args.max_rows as i64 {
                eprintln!(
                    "Invariant violation: final_checkpoint ({}) > max_rows ({})",
                    final_checkpoint, args.max_rows
                );
                std::process::exit(1);
            }
            if final_rows > args.max_rows as i64 {
                eprintln!(
                    "Invariant violation: final_rows ({}) > max_rows ({})",
                    final_rows, args.max_rows
                );
                std::process::exit(1);
            }

            let elapsed_ms = elapsed.as_millis();
            let rows_per_second = if result.inserted_rows > 0 && elapsed.as_secs_f64() > 0.0 {
                result.inserted_rows as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            println!("CMS IMPORT");
            println!("CSV: {}", args.csv.display());
            println!("DB: {}", args.db.display());
            println!("max_rows: {}", args.max_rows);
            println!("chunk_size: {}", args.chunk_size);
            println!("mode: {}", mode);
            println!("source_key: {}", result.source_key);
            println!("starting_rows: {}", starting_rows);
            println!("inserted_rows: {}", result.inserted_rows);
            println!("final_rows: {}", final_rows);
            println!("final_checkpoint: {}", final_checkpoint);
            println!("elapsed_ms: {}", elapsed_ms);
            println!("rows_per_second: {:.0}", rows_per_second);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            print_usage();
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parser tests (all go through the ONE shared parse_args_from) ──

    #[test]
    fn test_parse_full_args() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "5000".into(),
            "--chunk-size".into(),
            "500".into(),
            "--resume".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.csv, PathBuf::from("data.csv"));
                assert_eq!(parsed.db, PathBuf::from("test.sqlite"));
                assert_eq!(parsed.max_rows, 5000);
                assert_eq!(parsed.chunk_size, 500);
                assert!(parsed.resume);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    #[test]
    fn test_parse_minimal_args() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "50000".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.chunk_size, 10_000);
                assert!(!parsed.resume);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

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

    #[test]
    fn test_parse_missing_csv() {
        let args = vec![
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_missing_db() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--max-rows".into(),
            "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_missing_max_rows() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_max_rows_zero() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "0".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_max_rows_exceeds_limit() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100001".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_chunk_size_zero() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
            "--chunk-size".into(),
            "0".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_chunk_size_exceeds_max_rows() {
        // chunk_size > max_rows is allowed; the importer internally caps it.
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
            "--chunk-size".into(),
            "200".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.chunk_size, 200);
                assert_eq!(parsed.max_rows, 100);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    #[test]
    fn test_parse_unknown_argument() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
            "--bogus".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // ── Validation tests ──

    fn tmp_path(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("cli_{}_{}_{}", name, std::process::id(), id))
    }

    #[test]
    fn test_validate_ocp_db_rejected() {
        let args = Args {
            csv: PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv"),
            db: PathBuf::from("ocp.db"),
            max_rows: 100,
            chunk_size: 10,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_ocp_db_absolute_rejected() {
        let ocp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ocp.db");
        let args = Args {
            csv: PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv"),
            db: ocp,
            max_rows: 100,
            chunk_size: 10,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_existing_db_without_resume() {
        let p = tmp_path("existing_db");
        std::fs::write(&p, b"fake").unwrap();
        let csv = PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv");
        let args = Args {
            csv,
            db: p.clone(),
            max_rows: 100,
            chunk_size: 10,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_validate_existing_db_with_resume() {
        let p = tmp_path("existing_db_resume");
        std::fs::write(&p, b"fake").unwrap();
        let csv = PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv");
        let args = Args {
            csv,
            db: p.clone(),
            max_rows: 100,
            chunk_size: 10,
            resume: true,
        };
        assert!(validate_args(&args).is_ok());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_validate_resume_nonexistent_db() {
        let p = tmp_path("nonexistent_db_resume");
        let csv = PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv");
        let args = Args {
            csv,
            db: p,
            max_rows: 100,
            chunk_size: 10,
            resume: true,
        };
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_csv_equals_db() {
        let p = tmp_path("csv_eq_db");
        std::fs::write(&p, b"fake").unwrap();
        let args = Args {
            csv: p.clone(),
            db: p.clone(),
            max_rows: 100,
            chunk_size: 10,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
        std::fs::remove_file(&p).ok();
    }
}
