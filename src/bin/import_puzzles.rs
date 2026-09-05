use std::path::{Path, PathBuf};

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::MigrationHarness;

use chess_material_studio::puzzle_import::{
    self, MIGRATIONS, PuzzleFileImportResult,
};

const DEFAULT_CHUNK_SIZE: usize = 50_000;
const MAX_CHUNK_SIZE: usize = 100_000;
const MAX_SAFETY_LIMIT: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportMode {
    Limited { max_rows: usize },
    Full,
}

#[derive(Debug)]
struct Args {
    csv: PathBuf,
    db: PathBuf,
    mode: ImportMode,
    chunk_size: usize,
    resume: bool,
}

#[derive(Debug)]
enum ParseOutcome {
    Run(Args),
    Help,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  import_puzzles --csv <PATH> --db <PATH> (--max-rows <N> | --full [--confirm-full-import]) [--chunk-size <N>] [--resume]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  --csv <PATH>                 Path to the Lichess CSV file (required)");
    eprintln!("  --db <PATH>                  Path to the SQLite database (required)");
    eprintln!("  --max-rows <N>               Limited mode: max total rows (1..=100000)");
    eprintln!("  --full                       Full mode: import all rows until EOF");
    eprintln!("  --confirm-full-import        Required with --full to confirm full import");
    eprintln!("  --chunk-size <N>             Rows per transaction chunk (optional, default: 50000)");
    eprintln!("  --resume                     Resume an existing import (required if DB exists)");
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
    let mut full = false;
    let mut confirm_full_import = false;
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
            "--full" => {
                full = true;
            }
            "--confirm-full-import" => {
                confirm_full_import = true;
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

    // ── Mode validation ──

    if max_rows.is_some() && full {
        return Err("Choose exactly one import mode: --max-rows <N> or --full".into());
    }

    if max_rows.is_none() && !full {
        return Err("Choose exactly one import mode: --max-rows <N> or --full".into());
    }

    if full && !confirm_full_import {
        return Err("Full import requires --confirm-full-import".into());
    }

    if confirm_full_import && !full {
        return Err("--confirm-full-import requires --full".into());
    }

    let mode = if let Some(n) = max_rows {
        if n == 0 {
            return Err("max_rows must be greater than 0".into());
        }
        if n > MAX_SAFETY_LIMIT {
            return Err(format!(
                "CMS-009 safety limit: --max-rows cannot exceed {}",
                MAX_SAFETY_LIMIT
            ));
        }
        ImportMode::Limited { max_rows: n }
    } else {
        ImportMode::Full
    };

    let chunk_size = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
    if chunk_size == 0 {
        return Err("chunk_size must be greater than 0".into());
    }
    if chunk_size > MAX_CHUNK_SIZE {
        return Err(format!(
            "chunk_size cannot exceed {}",
            MAX_CHUNK_SIZE
        ));
    }

    Ok(ParseOutcome::Run(Args {
        csv,
        db,
        mode,
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

/// Normalize a path by canonicalizing the closest existing ancestor
/// and re-joining remaining non-existing suffix components.
///
/// This avoids the Windows `\\?\` prefix mismatch that occurs when
/// mixing `canonicalize` (returns UNC) with `cwd.join()` (returns regular).
fn normalize_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return std::fs::canonicalize(path)
            .map_err(|e| format!("cannot canonicalize {}: {}", path.display(), e));
    }

    // Collect non-existing suffix components bottom-up
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path;

    while !current.exists() {
        match current.file_name() {
            Some(name) => suffix.push(name.to_os_string()),
            None => break,
        }
        current = match current.parent() {
            Some(p) => p,
            None => break,
        };
    }

    // Empty path "" is semantically cwd — canonicalize "." instead
    let base = if current.as_os_str().is_empty() {
        std::fs::canonicalize(".")
            .map_err(|e| format!("cannot canonicalize current dir: {}", e))?
    } else {
        std::fs::canonicalize(current)
            .map_err(|e| format!("cannot canonicalize base {}: {}", current.display(), e))?
    };

    let mut result = base;
    for component in suffix.iter().rev() {
        result = result.join(component);
    }
    Ok(result)
}

/// Check whether a DB path is within `<CARGO_MANIFEST_DIR>/target/cms_full_import/`.
///
/// Both the allowed dir and the candidate are normalized through the same
/// `normalize_path` function (canonicalize closest existing ancestor, re-join
/// suffix) to ensure consistent representation on Windows.
fn is_path_within_full_import_dir(path: &Path) -> Result<bool, String> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let allowed_dir = project_root.join("target").join("cms_full_import");

    let allowed_canon = normalize_path(&allowed_dir)
        .map_err(|e| format!("cannot normalize allowed dir: {}", e))?;
    let target_canon = normalize_path(path)
        .map_err(|e| format!("cannot normalize {}: {}", path.display(), e))?;

    Ok(target_canon.starts_with(&allowed_canon))
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

    // Full mode: DB must be within target/cms_full_import/
    if args.mode == ImportMode::Full {
        if !is_path_within_full_import_dir(&args.db)? {
            return Err(
                "Full import DB must be within target/cms_full_import/ directory".into(),
            );
        }
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
    use chess_material_studio::schema::puzzles;
    puzzles::table
        .count()
        .get_result::<i64>(conn)
        .map_err(|e| format!("count query failed: {}", e))
}

fn checkpoint_value(conn: &mut SqliteConnection, source_key: &str) -> Result<i64, String> {
    use chess_material_studio::schema::puzzle_import_progress;
    puzzle_import_progress::table
        .filter(puzzle_import_progress::dsl::source_key.eq(source_key))
        .select(puzzle_import_progress::dsl::completed_rows)
        .first::<i64>(conn)
        .optional()
        .map_err(|e| format!("checkpoint query failed: {}", e))?
        .ok_or_else(|| format!("checkpoint not found for source_key: {}", source_key))
}

/// Count the number of distinct source_keys in puzzle_import_progress.
fn checkpoint_count(conn: &mut SqliteConnection) -> Result<i64, String> {
    use chess_material_studio::schema::puzzle_import_progress;
    puzzle_import_progress::table
        .count()
        .get_result::<i64>(conn)
        .map_err(|e| format!("checkpoint count query failed: {}", e))
}

/// Query the single source_key stored in puzzle_import_progress.
///
/// Precondition: exactly one row exists (caller must verify via `checkpoint_count`).
fn single_checkpoint_source_key(conn: &mut SqliteConnection) -> Result<String, String> {
    use chess_material_studio::schema::puzzle_import_progress;
    puzzle_import_progress::table
        .select(puzzle_import_progress::dsl::source_key)
        .first::<String>(conn)
        .map_err(|e| format!("checkpoint source_key query failed: {}", e))
}

/// Full resume preflight: verify that the existing DB has exactly one checkpoint
/// whose source_key matches the expected one from the CSV.
fn validate_full_resume_source(
    conn: &mut SqliteConnection,
    csv_path: &Path,
) -> Result<String, String> {
    let expected_source_key = puzzle_import::puzzle_source_key_from_file(csv_path)
        .map_err(|e| format!("failed to compute CSV source key: {}", e))?;

    let count = checkpoint_count(conn)?;
    if count == 0 {
        return Err("Cannot resume full import: no checkpoint found".into());
    }
    if count > 1 {
        return Err("Cannot resume full import: multiple source checkpoints found".into());
    }

    // Query the single stored source_key directly
    let stored_key = single_checkpoint_source_key(conn)?;
    if stored_key != expected_source_key {
        return Err(
            "Cannot resume full import: CSV source identity does not match checkpoint".into(),
        );
    }

    Ok(expected_source_key)
}

fn file_size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn db_size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
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

            let csv_size = file_size_bytes(&args.csv);

            // Full resume preflight: compute source_key before importing
            let preflight_source_key = if args.mode == ImportMode::Full && args.resume {
                match validate_full_resume_source(&mut conn, &args.csv) {
                    Ok(k) => Some(k),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                None
            };

            let mode_label = match &args.mode {
                ImportMode::Limited { .. } => "limited",
                ImportMode::Full => "full",
            };

            let start = std::time::Instant::now();
            let result: PuzzleFileImportResult = match &args.mode {
                ImportMode::Limited { max_rows } => {
                    match puzzle_import::import_puzzles_from_file_chunked_limited(
                        &mut conn,
                        &args.csv,
                        args.chunk_size,
                        *max_rows,
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("Import error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ImportMode::Full => {
                    match puzzle_import::import_puzzles_from_file_chunked(
                        &mut conn,
                        &args.csv,
                        args.chunk_size,
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("Import error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            };
            let elapsed = start.elapsed();

            // Verify preflight source_key matches (full + resume)
            if let Some(ref expected) = preflight_source_key {
                if &result.source_key != expected {
                    eprintln!(
                        "Cannot resume full import: CSV source identity does not match checkpoint"
                    );
                    std::process::exit(1);
                }
            }

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

            let starting_checkpoint = final_checkpoint - result.inserted_rows as i64;
            if starting_checkpoint < 0 {
                eprintln!(
                    "Invariant violation: starting_checkpoint ({}) is negative",
                    starting_checkpoint
                );
                std::process::exit(1);
            }

            // Post-import invariants
            match &args.mode {
                ImportMode::Limited { max_rows } => {
                    if final_checkpoint > *max_rows as i64 {
                        eprintln!(
                            "Invariant violation: final_checkpoint ({}) > max_rows ({})",
                            final_checkpoint, max_rows
                        );
                        std::process::exit(1);
                    }
                    if final_rows > *max_rows as i64 {
                        eprintln!(
                            "Invariant violation: final_rows ({}) > max_rows ({})",
                            final_rows, max_rows
                        );
                        std::process::exit(1);
                    }
                }
                ImportMode::Full => {
                    if final_rows != final_checkpoint {
                        eprintln!(
                            "Invariant violation: final_rows ({}) != final_checkpoint ({})",
                            final_rows, final_checkpoint
                        );
                        std::process::exit(1);
                    }
                    if result.inserted_rows as i64 != final_checkpoint - starting_checkpoint {
                        eprintln!(
                            "Invariant violation: inserted_rows ({}) != final_checkpoint - starting_checkpoint ({})",
                            result.inserted_rows, final_checkpoint - starting_checkpoint
                        );
                        std::process::exit(1);
                    }
                }
            }

            let elapsed_ms = elapsed.as_millis();
            let rows_per_second = if result.inserted_rows > 0 && elapsed.as_secs_f64() > 0.0 {
                result.inserted_rows as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            let db_size = db_size_bytes(&args.db);

            let max_rows_display = match &args.mode {
                ImportMode::Limited { max_rows } => max_rows.to_string(),
                ImportMode::Full => "EOF".to_string(),
            };

            println!("CMS IMPORT");
            println!("mode: {}", mode_label);
            println!("CSV: {}", args.csv.display());
            println!("DB: {}", args.db.display());
            println!("chunk_size: {}", args.chunk_size);
            println!("source_key: {}", result.source_key);
            println!("starting_rows: {}", starting_rows);
            println!("starting_checkpoint: {}", starting_checkpoint);
            println!("inserted_rows: {}", result.inserted_rows);
            println!("final_rows: {}", final_rows);
            println!("final_checkpoint: {}", final_checkpoint);
            println!("max_rows: {}", max_rows_display);
            println!("elapsed_ms: {}", elapsed_ms);
            println!("rows_per_second: {:.0}", rows_per_second);
            println!("db_size_bytes: {}", db_size);
            println!("csv_size_bytes: {}", csv_size);

            if args.mode == ImportMode::Full {
                let difference_from_6m = final_rows as i64 - 6_000_000;
                println!("actual_full_rows: {}", final_rows);
                println!("difference_from_6m: {}", difference_from_6m);
            }
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
    fn test_parse_limited_valid() {
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
                assert_eq!(parsed.mode, ImportMode::Limited { max_rows: 5000 });
                assert_eq!(parsed.chunk_size, 500);
                assert!(parsed.resume);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    #[test]
    fn test_parse_full_valid_with_confirm() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "target/cms_full_import/lichess_full.sqlite".into(),
            "--full".into(),
            "--confirm-full-import".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.mode, ImportMode::Full);
                assert!(!parsed.resume);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    #[test]
    fn test_parse_full_without_confirm_fails() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "target/cms_full_import/lichess_full.sqlite".into(),
            "--full".into(),
        ];
        let err = parse_args_from(&args).unwrap_err();
        assert!(err.contains("--confirm-full-import"), "error: {}", err);
    }

    #[test]
    fn test_parse_confirm_without_full_fails() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--confirm-full-import".into(),
        ];
        let err = parse_args_from(&args).unwrap_err();
        assert!(err.contains("--full"), "error: {}", err);
    }

    #[test]
    fn test_parse_full_plus_max_rows_fails() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--full".into(),
            "--confirm-full-import".into(),
            "--max-rows".into(),
            "100".into(),
        ];
        let err = parse_args_from(&args).unwrap_err();
        assert!(err.contains("exactly one"), "error: {}", err);
    }

    #[test]
    fn test_parse_no_mode_fails() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
        ];
        let err = parse_args_from(&args).unwrap_err();
        assert!(err.contains("exactly one"), "error: {}", err);
    }

    #[test]
    fn test_parse_max_rows_zero_fails() {
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
    fn test_parse_chunk_zero_fails() {
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
    fn test_parse_chunk_exceeds_max_fails() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
            "--chunk-size".into(),
            "100001".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    #[test]
    fn test_parse_chunk_size_greater_than_max_rows_allowed() {
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
                assert_eq!(parsed.mode, ImportMode::Limited { max_rows: 100 });
                assert_eq!(parsed.chunk_size, 200);
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    #[test]
    fn test_parse_default_chunk_size() {
        let args = vec![
            "--csv".into(),
            "data.csv".into(),
            "--db".into(),
            "test.sqlite".into(),
            "--max-rows".into(),
            "100".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.chunk_size, 50_000, "default chunk should be 50000");
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

    // ── Path validation tests ──

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
    fn test_full_import_path_within_target_allowed() {
        let p = PathBuf::from("target/cms_full_import/test.sqlite");
        assert!(is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_path_outside_rejected() {
        let p = PathBuf::from("test.sqlite");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_path_other_subdir_rejected() {
        let p = PathBuf::from("target/other/test.sqlite");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_ocp_db_rejected() {
        let p = PathBuf::from("ocp.db");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_path_traversal_rejected() {
        let p = PathBuf::from("target/cms_full_import/../../ocp.db");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_sibling_prefix_rejected() {
        let p = PathBuf::from("target/cms_full_import_evil/test.sqlite");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_absolute_path_within_allowed() {
        let allowed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_full_import")
            .join("test.sqlite");
        assert!(is_path_within_full_import_dir(&allowed).unwrap());
    }

    // ── Validation tests ──

    #[test]
    fn test_validate_ocp_db_rejected() {
        let args = Args {
            csv: PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv"),
            db: PathBuf::from("ocp.db"),
            mode: ImportMode::Limited { max_rows: 100 },
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
            mode: ImportMode::Limited { max_rows: 100 },
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
            mode: ImportMode::Limited { max_rows: 100 },
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
            mode: ImportMode::Limited { max_rows: 100 },
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
            mode: ImportMode::Limited { max_rows: 100 },
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
            mode: ImportMode::Limited { max_rows: 100 },
            chunk_size: 10,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_validate_full_mode_outside_dir_rejected() {
        let csv = PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv");
        let args = Args {
            csv,
            db: PathBuf::from("cms011_forbidden.sqlite"),
            mode: ImportMode::Full,
            chunk_size: 50_000,
            resume: false,
        };
        assert!(validate_args(&args).is_err());
    }

    // ── Full resume source identity tests ──

    fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        let p = dir.join(format!("resume_{}_{}_{}", name, std::process::id(), id));
        std::fs::write(&p, content).unwrap();
        p
    }

    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
    }

    #[test]
    fn test_full_resume_preflight_ok() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("resume_ok", fixture.as_bytes());
        let mut conn = setup_test_db();

        // Seed: import all 4 puzzles from the fixture
        let result = puzzle_import::import_puzzles_from_file_chunked(&mut conn, &csv_path, 2)
            .expect("seed import");
        assert_eq!(result.inserted_rows, 4);

        // Now validate preflight — should succeed with matching source_key
        let key = validate_full_resume_source(&mut conn, &csv_path).expect("preflight should pass");
        assert_eq!(key, result.source_key);

        std::fs::remove_file(&csv_path).ok();
    }

    #[test]
    fn test_full_resume_preflight_no_checkpoint() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("resume_none", fixture.as_bytes());
        let mut conn = setup_test_db();

        // Empty DB — no checkpoints
        let err = validate_full_resume_source(&mut conn, &csv_path).unwrap_err();
        assert!(err.contains("no checkpoint"), "error: {}", err);

        std::fs::remove_file(&csv_path).ok();
    }

    #[test]
    fn test_full_resume_preflight_wrong_source_key() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("resume_wrong", fixture.as_bytes());
        let mut conn = setup_test_db();

        // Import with a fake source_key that doesn't match the CSV
        let fake_key = "cms-source-v1:999999:0000000000000000";

        // Manually insert a checkpoint with wrong key
        diesel::insert_into(chess_material_studio::schema::puzzle_import_progress::table)
            .values((
                chess_material_studio::schema::puzzle_import_progress::dsl::source_key
                    .eq(fake_key),
                chess_material_studio::schema::puzzle_import_progress::dsl::completed_rows
                    .eq(4i64),
            ))
            .execute(&mut conn)
            .expect("insert fake checkpoint");

        // Also insert 4 puzzles to make the DB non-empty
        let count = puzzle_import::import_puzzles_from_reader(&mut conn, fixture.as_bytes())
            .expect("seed puzzles");
        assert_eq!(count, 4);

        // Preflight should fail: key mismatch — explicit message
        let err = validate_full_resume_source(&mut conn, &csv_path).unwrap_err();
        assert!(
            err.contains("does not match checkpoint"),
            "error: {}",
            err
        );

        std::fs::remove_file(&csv_path).ok();
    }

    #[test]
    fn test_full_resume_preflight_multiple_checkpoints() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("resume_multi", fixture.as_bytes());
        let mut conn = setup_test_db();

        // Insert two different source_keys
        diesel::insert_into(chess_material_studio::schema::puzzle_import_progress::table)
            .values((
                chess_material_studio::schema::puzzle_import_progress::dsl::source_key
                    .eq("key-alpha"),
                chess_material_studio::schema::puzzle_import_progress::dsl::completed_rows
                    .eq(2i64),
            ))
            .execute(&mut conn)
            .expect("insert alpha");

        diesel::insert_into(chess_material_studio::schema::puzzle_import_progress::table)
            .values((
                chess_material_studio::schema::puzzle_import_progress::dsl::source_key
                    .eq("key-beta"),
                chess_material_studio::schema::puzzle_import_progress::dsl::completed_rows
                    .eq(3i64),
            ))
            .execute(&mut conn)
            .expect("insert beta");

        let err = validate_full_resume_source(&mut conn, &csv_path).unwrap_err();
        assert!(
            err.contains("multiple source checkpoints"),
            "error: {}",
            err
        );

        std::fs::remove_file(&csv_path).ok();
    }

    // ── Full import small functional test ──

    #[test]
    fn test_full_import_small_to_eof() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("full_small", fixture.as_bytes());
        let db_path = tmp_path("full_small_db");

        let mut conn = establish_connection(&db_path).expect("open db");
        run_migrations(&mut conn).expect("migrations");

        let starting_rows = row_count(&mut conn).expect("count");
        assert_eq!(starting_rows, 0);

        // First run: full import
        let result = puzzle_import::import_puzzles_from_file_chunked(&mut conn, &csv_path, 2)
            .expect("full import");
        assert_eq!(result.inserted_rows, 4, "fixture has 4 puzzles");

        let final_rows = row_count(&mut conn).expect("count");
        assert_eq!(final_rows, 4);

        let final_cp = checkpoint_value(&mut conn, &result.source_key).expect("checkpoint");
        assert_eq!(final_cp, 4);

        // Second run: resume — should insert 0
        let result2 = puzzle_import::import_puzzles_from_file_chunked(&mut conn, &csv_path, 2)
            .expect("second import");
        assert_eq!(result2.inserted_rows, 0, "second run should insert 0");
        assert_eq!(result.source_key, result2.source_key, "source_key must match");

        let final_rows2 = row_count(&mut conn).expect("count");
        assert_eq!(final_rows2, 4, "rows unchanged");

        let final_cp2 = checkpoint_value(&mut conn, &result2.source_key).expect("checkpoint");
        assert_eq!(final_cp2, 4, "checkpoint unchanged");

        std::fs::remove_file(&csv_path).ok();
        std::fs::remove_file(&db_path).ok();
    }
}
