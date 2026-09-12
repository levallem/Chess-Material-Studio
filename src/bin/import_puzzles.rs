use std::path::{Component, Path, PathBuf};

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

fn canonicalize_if_exists(path: &Path) -> Result<PathBuf, String> {
    normalize_path(path)
}

/// Check whether a given path resolves to the project's `ocp.db`.
///
/// Anchored to `CARGO_MANIFEST_DIR`, not the current working directory.
fn is_ocp_db(path: &Path) -> Result<bool, String> {
    let project_ocp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ocp.db");
    let protected = normalize_path(&project_ocp)
        .map_err(|e| format!("cannot normalize protected database: {}", e))?;
    let target = normalize_path(path)
        .map_err(|e| format!("cannot normalize {}: {}", path.display(), e))?;

    Ok(target == protected)
}

/// Normalize a path by resolving existing components with `canonicalize` and
/// lexically resolving any remaining components.
///
/// Existing components are canonicalized as they are encountered, so symlinks
/// cannot escape a later containment check. Missing suffix components are
/// normalized one at a time: `.` is ignored, `..` pops one component, and
/// normal components are appended without requiring them to exist.
fn normalize_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot get cwd: {}", e))?
            .join(path)
    };

    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(name) => {
                result.push(name);
                if result.try_exists().map_err(|e| {
                    format!("cannot inspect path component {}: {}", result.display(), e)
                })? {
                    result = std::fs::canonicalize(&result).map_err(|e| {
                        format!("cannot canonicalize {}: {}", result.display(), e)
                    })?;
                }
            }
        }
    }

    if result.as_os_str().is_empty() {
        std::fs::canonicalize(".")
            .map_err(|e| format!("cannot canonicalize current dir: {}", e))
    } else {
        Ok(result)
    }
}
/// Check whether a DB path is within `project_root/target/cms_full_import/`.
///
/// The canonical target and allowed directories must remain physically within
/// their parent security boundaries before either can be used for containment.
fn is_path_within_full_import_dir_from_root(
    path: &Path,
    project_root: &Path,
) -> Result<bool, String> {
    let project_root_canon = normalize_path(project_root)
        .map_err(|e| format!("cannot normalize project root: {}", e))?;
    let target_dir = project_root.join("target");
    let target_canon = normalize_path(&target_dir)
        .map_err(|e| format!("cannot normalize target dir: {}", e))?;
    if !target_canon.starts_with(&project_root_canon) {
        return Ok(false);
    }

    let allowed_dir = target_dir.join("cms_full_import");
    let allowed_canon = normalize_path(&allowed_dir)
        .map_err(|e| format!("cannot normalize allowed dir: {}", e))?;
    if !allowed_canon.starts_with(&target_canon) {
        return Ok(false);
    }

    let target_canon = normalize_path(path)
        .map_err(|e| format!("cannot normalize {}: {}", path.display(), e))?;

    Ok(target_canon.starts_with(&allowed_canon))
}

fn is_path_within_full_import_dir(path: &Path) -> Result<bool, String> {
    is_path_within_full_import_dir_from_root(path, Path::new(env!("CARGO_MANIFEST_DIR")))
}
fn validate_args(args: &Args) -> Result<(), String> {
    let csv_canon = canonicalize_if_exists(&args.csv)?;
    let db_canon = canonicalize_if_exists(&args.db)?;

    if csv_canon == db_canon {
        return Err("--csv and --db cannot resolve to the same file".into());
    }

    if is_ocp_db(&args.db)? {
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

/// Resume preflight: verify that the existing DB has exactly one checkpoint
/// whose source_key matches the expected one from the CSV.
fn validate_resume_source(
    conn: &mut SqliteConnection,
    csv_path: &Path,
    mode: &ImportMode,
) -> Result<String, String> {
    let import_kind = match mode {
        ImportMode::Limited { .. } => "limited import",
        ImportMode::Full => "full import",
    };
    let expected_source_key = puzzle_import::puzzle_source_key_from_file(csv_path)
        .map_err(|e| format!("failed to compute CSV source key: {}", e))?;

    let count = checkpoint_count(conn)?;
    if count == 0 {
        return Err(format!("Cannot resume {}: no checkpoint found", import_kind));
    }
    if count > 1 {
        return Err(format!(
            "Cannot resume {}: multiple source checkpoints found",
            import_kind
        ));
    }

    // Query the single stored source_key directly
    let stored_key = single_checkpoint_source_key(conn)?;
    if stored_key != expected_source_key {
        return Err(format!(
            "Cannot resume {}: CSV source identity does not match checkpoint",
            import_kind
        ));
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

            // Resume preflight: compute and validate the source key before importing.
            let preflight_source_key = if args.resume {
                match validate_resume_source(&mut conn, &args.csv, &args.mode) {
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

            // Verify the source key did not change between preflight and import.
            if let Some(ref expected) = preflight_source_key {
                if &result.source_key != expected {
                    let import_kind = match &args.mode {
                        ImportMode::Limited { .. } => "limited import",
                        ImportMode::Full => "full import",
                    };
                    eprintln!(
                        "Cannot resume {}: CSV source identity does not match checkpoint",
                        import_kind
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
    fn test_full_import_path_internal_normalization_allowed() {
        let p = PathBuf::from("target/cms_full_import/sub/../test.sqlite");
        assert!(is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_full_import_path_multiple_parent_dirs_rejected() {
        let p = PathBuf::from("target/cms_full_import/sub/../../../outside.sqlite");
        assert!(!is_path_within_full_import_dir(&p).unwrap());
    }

    #[test]
    fn test_normalize_path_lexically_resolves_missing_suffix_components() {
        let missing_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("cms_normalize_missing_{}", std::process::id()));
        assert!(!missing_dir.exists(), "test directory must not exist");

        let normalized = normalize_path(&missing_dir.join("sub").join("..").join("test.sqlite"))
            .unwrap();
        let expected = normalize_path(&missing_dir.join("test.sqlite")).unwrap();

        assert_eq!(normalized, expected);
    }

    #[cfg(unix)]
    struct TestDirCleanup(Vec<PathBuf>);

    #[cfg(unix)]
    impl Drop for TestDirCleanup {
        fn drop(&mut self) {
            for path in &self.0 {
                std::fs::remove_dir_all(path).ok();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_full_import_allowed_dir_symlink_outside_target_rejected() {
        use std::os::unix::fs::symlink;

        let project_root = tmp_path("full_import_symlink_project");
        let external_dir = tmp_path("full_import_symlink_external");
        let _cleanup = TestDirCleanup(vec![project_root.clone(), external_dir.clone()]);
        let target_dir = project_root.join("target");
        let allowed_dir = target_dir.join("cms_full_import");

        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::create_dir_all(&external_dir).unwrap();
        symlink(&external_dir, &allowed_dir).unwrap();

        assert!(!is_path_within_full_import_dir_from_root(
            &allowed_dir.join("external.sqlite"),
            &project_root,
        )
        .unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn test_full_import_candidate_symlink_outside_target_rejected() {
        use std::os::unix::fs::symlink;

        let project_root = tmp_path("full_import_candidate_symlink_project");
        let external_dir = tmp_path("full_import_candidate_symlink_external");
        let _cleanup = TestDirCleanup(vec![project_root.clone(), external_dir.clone()]);
        let allowed_dir = project_root.join("target").join("cms_full_import");

        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&external_dir).unwrap();
        symlink(&external_dir, allowed_dir.join("outside")).unwrap();

        assert!(!is_path_within_full_import_dir_from_root(
            &allowed_dir.join("outside").join("external.sqlite"),
            &project_root,
        )
        .unwrap());
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
    fn test_validate_ocp_db_through_missing_parent_rejected() {
        let missing_parent = format!("cms_ocp_missing_{}", std::process::id());
        assert!(!Path::new(&missing_parent).exists());
        let args = Args {
            csv: PathBuf::from("tests/fixtures/lichess_puzzles_sample.csv"),
            db: PathBuf::from(missing_parent).join("..").join("ocp.db"),
            mode: ImportMode::Limited { max_rows: 100 },
            chunk_size: 10,
            resume: false,
        };

        assert_eq!(
            validate_args(&args).unwrap_err(),
            "Refusing to use protected database: ocp.db"
        );
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

    // ── Resume source identity tests ──

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
        let key = validate_resume_source(&mut conn, &csv_path, &ImportMode::Full)
            .expect("preflight should pass");
        assert_eq!(key, result.source_key);

        std::fs::remove_file(&csv_path).ok();
    }

    #[test]
    fn test_full_resume_preflight_no_checkpoint() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("resume_none", fixture.as_bytes());
        let mut conn = setup_test_db();

        // Empty DB — no checkpoints
        let err = validate_resume_source(&mut conn, &csv_path, &ImportMode::Full).unwrap_err();
        assert_eq!(err, "Cannot resume full import: no checkpoint found");

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
        let err = validate_resume_source(&mut conn, &csv_path, &ImportMode::Full).unwrap_err();
        assert_eq!(
            err,
            "Cannot resume full import: CSV source identity does not match checkpoint"
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

        let err = validate_resume_source(&mut conn, &csv_path, &ImportMode::Full).unwrap_err();
        assert_eq!(err, "Cannot resume full import: multiple source checkpoints found");

        std::fs::remove_file(&csv_path).ok();
    }

    #[test]
    fn test_limited_resume_preflight_wrong_source_preserves_database() {
        let source_a = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let source_b = format!("{}\n", source_a.replace("00001", "99999"));
        let csv_a_path = write_tmp("limited_resume_a", source_a.as_bytes());
        let csv_b_path = write_tmp("limited_resume_b", source_b.as_bytes());
        let mut conn = setup_test_db();

        let source_a_result =
            puzzle_import::import_puzzles_from_file_chunked_limited(&mut conn, &csv_a_path, 1, 2)
                .expect("seed limited import");
        let source_b_key = puzzle_import::puzzle_source_key_from_file(&csv_b_path)
            .expect("compute source B key");
        assert_ne!(source_a_result.source_key, source_b_key);
        let rows_before = row_count(&mut conn).expect("count rows before preflight");
        let checkpoint_a_before = checkpoint_value(&mut conn, &source_a_result.source_key)
            .expect("read source A checkpoint before preflight");
        let checkpoints_before = checkpoint_count(&mut conn).expect("count checkpoints before preflight");
        let source_b_rows_before: i64 = chess_material_studio::schema::puzzles::table
            .filter(chess_material_studio::schema::puzzles::dsl::puzzle_id.eq("99999"))
            .count()
            .get_result(&mut conn)
            .expect("count source B-exclusive rows before preflight");
        assert_eq!(source_b_rows_before, 0);

        let err = validate_resume_source(
            &mut conn,
            &csv_b_path,
            &ImportMode::Limited { max_rows: 2 },
        )
        .unwrap_err();
        assert!(err.contains("does not match checkpoint"), "error: {}", err);

        assert_eq!(
            row_count(&mut conn).expect("count rows after preflight"),
            rows_before,
            "mismatched limited resume must not insert rows"
        );
        assert_eq!(
            checkpoint_count(&mut conn).expect("count checkpoints after preflight"),
            checkpoints_before,
            "mismatched limited resume must not add checkpoints"
        );
        assert_eq!(
            checkpoint_value(&mut conn, &source_a_result.source_key)
                .expect("read source A checkpoint after preflight"),
            checkpoint_a_before,
            "mismatched limited resume must not update source A's checkpoint"
        );
        assert!(
            checkpoint_value(&mut conn, &source_b_key).is_err(),
            "mismatched limited resume must not create source B's checkpoint"
        );
        let source_b_rows_after: i64 = chess_material_studio::schema::puzzles::table
            .filter(chess_material_studio::schema::puzzles::dsl::puzzle_id.eq("99999"))
            .count()
            .get_result(&mut conn)
            .expect("count source B-exclusive rows after preflight");
        assert_eq!(
            source_b_rows_after, source_b_rows_before,
            "mismatched limited resume must not insert source B-exclusive rows"
        );

        std::fs::remove_file(&csv_a_path).ok();
        std::fs::remove_file(&csv_b_path).ok();
    }

    #[test]
    fn test_limited_resume_same_source_preflight_allows_continuation() {
        let fixture = include_str!("../../tests/fixtures/lichess_puzzles_sample.csv");
        let csv_path = write_tmp("limited_resume_same", fixture.as_bytes());
        let mut conn = setup_test_db();

        let initial =
            puzzle_import::import_puzzles_from_file_chunked_limited(&mut conn, &csv_path, 1, 2)
                .expect("initial limited import");
        assert_eq!(initial.inserted_rows, 2);

        let source_key = validate_resume_source(
            &mut conn,
            &csv_path,
            &ImportMode::Limited { max_rows: 4 },
        )
        .expect("same source preflight");
        assert_eq!(source_key, initial.source_key);

        let resumed =
            puzzle_import::import_puzzles_from_file_chunked_limited(&mut conn, &csv_path, 1, 4)
                .expect("resume limited import");
        assert_eq!(resumed.inserted_rows, 2);
        assert_eq!(row_count(&mut conn).expect("count final rows"), 4);
        assert_eq!(
            checkpoint_value(&mut conn, &source_key).expect("read final checkpoint"),
            4
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
