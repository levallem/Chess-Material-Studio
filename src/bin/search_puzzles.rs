// CMS-012 — SQLite Puzzle Search Benchmark Runner
//
// Validation tool: opens an existing SQLite DB, runs searches,
// measures performance. Read-only — no writes, no migrations, no PRAGMA.

use std::path::PathBuf;
use std::time::Instant;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

use offline_chess_puzzles::puzzle_search::{search_puzzles, PuzzleSearchFilters, SearchSide};
use offline_chess_puzzles::schema;

const MAX_RESULT_LIMIT: usize = 10_000;
const MAX_REPEAT: usize = 20;

// ── CLI ────────────────────────────────────────────────────────────────

struct Args {
    db: PathBuf,
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: Option<String>,
    opening: Option<String>,
    side: SearchSide,
    limit: usize,
    repeat: usize,
}

enum ParseOutcome {
    Run(Args),
    Help,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  search_puzzles --db <PATH> --min-rating <N> --max-rating <N> --min-popularity <N> --limit <N> [OPTIONS]");
    eprintln!();
    eprintln!("Required arguments:");
    eprintln!("  --db <PATH>              Path to SQLite database file");
    eprintln!("  --min-rating <N>         Minimum puzzle rating");
    eprintln!("  --max-rating <N>         Maximum puzzle rating");
    eprintln!("  --min-popularity <N>     Minimum popularity");
    eprintln!("  --limit <N>              Maximum results (1..=10000)");
    eprintln!();
    eprintln!("Optional arguments:");
    eprintln!("  --theme <TAG>            Filter by theme tag");
    eprintln!("  --opening <TAG>          Filter by opening tag");
    eprintln!("  --side <any|white|black> Filter by side (default: any)");
    eprintln!("  --repeat <N>             Repeat search N times (1..=20, default: 1)");
    eprintln!("  --help, -h               Show this help");
}

fn parse_args_from(args: &[String]) -> Result<ParseOutcome, String> {
    if args.is_empty() {
        return Err("no arguments provided".into());
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }

    let mut db: Option<PathBuf> = None;
    let mut min_rating: Option<i32> = None;
    let mut max_rating: Option<i32> = None;
    let mut min_popularity: Option<i32> = None;
    let mut theme: Option<String> = None;
    let mut opening: Option<String> = None;
    let mut side_str: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut repeat: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                db = Some(PathBuf::from(args.get(i).ok_or("--db requires a value")?));
            }
            "--min-rating" => {
                i += 1;
                let val = args.get(i).ok_or("--min-rating requires a value")?;
                min_rating = Some(val.parse().map_err(|_| format!("invalid --min-rating: {}", val))?);
            }
            "--max-rating" => {
                i += 1;
                let val = args.get(i).ok_or("--max-rating requires a value")?;
                max_rating = Some(val.parse().map_err(|_| format!("invalid --max-rating: {}", val))?);
            }
            "--min-popularity" => {
                i += 1;
                let val = args.get(i).ok_or("--min-popularity requires a value")?;
                min_popularity = Some(val.parse().map_err(|_| format!("invalid --min-popularity: {}", val))?);
            }
            "--theme" => {
                i += 1;
                theme = Some(args.get(i).ok_or("--theme requires a value")?.clone());
            }
            "--opening" => {
                i += 1;
                opening = Some(args.get(i).ok_or("--opening requires a value")?.clone());
            }
            "--side" => {
                i += 1;
                side_str = Some(args.get(i).ok_or("--side requires a value")?.clone());
            }
            "--limit" => {
                i += 1;
                let val = args.get(i).ok_or("--limit requires a value")?;
                limit = Some(val.parse().map_err(|_| format!("invalid --limit: {}", val))?);
            }
            "--repeat" => {
                i += 1;
                let val = args.get(i).ok_or("--repeat requires a value")?;
                repeat = Some(val.parse().map_err(|_| format!("invalid --repeat: {}", val))?);
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }

    let db = db.ok_or("missing required argument: --db")?;
    let min_rating = min_rating.ok_or("missing required argument: --min-rating")?;
    let max_rating = max_rating.ok_or("missing required argument: --max-rating")?;
    let min_popularity = min_popularity.ok_or("missing required argument: --min-popularity")?;
    let limit = limit.ok_or("missing required argument: --limit")?;

    if limit == 0 {
        return Err("limit must be greater than 0".into());
    }
    if limit > MAX_RESULT_LIMIT {
        return Err(format!("limit must be <= {} (got {})", MAX_RESULT_LIMIT, limit));
    }

    let side = match side_str.as_deref() {
        None | Some("any") => SearchSide::Any,
        Some("white") => SearchSide::White,
        Some("black") => SearchSide::Black,
        Some(other) => return Err(format!("invalid --side: {} (expected any, white, or black)", other)),
    };

    let repeat = repeat.unwrap_or(1);
    if repeat == 0 {
        return Err("repeat must be greater than 0".into());
    }
    if repeat > MAX_REPEAT {
        return Err(format!("repeat must be <= {} (got {})", MAX_REPEAT, repeat));
    }

    Ok(ParseOutcome::Run(Args {
        db,
        min_rating,
        max_rating,
        min_popularity,
        theme,
        opening,
        side,
        limit,
        repeat,
    }))
}

// ── Helpers ────────────────────────────────────────────────────────────

fn open_existing_db(path: &std::path::Path) -> Result<SqliteConnection, String> {
    if !path.is_file() {
        return Err(format!("database file not found: {}", path.display()));
    }
    let path_str = path.to_str().ok_or("invalid db path (non-UTF-8)")?;
    SqliteConnection::establish(path_str).map_err(|e| format!("cannot open DB: {}", e))
}

fn row_count(conn: &mut SqliteConnection) -> Result<i64, String> {
    schema::puzzles::table
        .count()
        .get_result::<i64>(conn)
        .map_err(|e| format!("count query failed: {}", e))
}

// ── Main ───────────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let args = match parse_args_from(&std::env::args().skip(1).collect::<Vec<String>>())? {
        ParseOutcome::Help => {
            print_usage();
            return Ok(());
        }
        ParseOutcome::Run(a) => a,
    };

    // ── DB integrity before ──────────────────────────────────────────

    let db_metadata_before = std::fs::metadata(&args.db)
        .map_err(|e| format!("cannot stat DB: {}", e))?;
    let db_size_before = db_metadata_before.len();
    let db_mtime_before = db_metadata_before.modified()
        .map_err(|e| format!("cannot read mtime: {}", e))?;

    // ── Open DB (read-only intent) ──────────────────────────────────

    let mut conn = open_existing_db(&args.db)?;
    let db_total_rows = row_count(&mut conn)?;

    // ── Print config ─────────────────────────────────────────────────

    eprintln!("CMS-012 Search Benchmark");
    eprintln!();
    eprintln!("db: {}", args.db.display());
    eprintln!("db_total_rows: {}", db_total_rows);
    eprintln!("db_size_bytes_before: {}", db_size_before);
    eprintln!("min_rating: {}", args.min_rating);
    eprintln!("max_rating: {}", args.max_rating);
    eprintln!("min_popularity: {}", args.min_popularity);
    eprintln!("theme: {}", args.theme.as_deref().unwrap_or("-"));
    eprintln!("opening: {}", args.opening.as_deref().unwrap_or("-"));
    eprintln!("side: {:?}", args.side);
    eprintln!("limit: {}", args.limit);
    eprintln!("repeat: {}", args.repeat);
    eprintln!();

    // ── Run searches ─────────────────────────────────────────────────

    let filters = PuzzleSearchFilters {
        min_rating: args.min_rating,
        max_rating: args.max_rating,
        min_popularity: args.min_popularity,
        theme_tag: args.theme.clone(),
        opening_tag: args.opening.clone(),
        side: args.side,
        limit: args.limit,
    };

    let mut run_times: Vec<u128> = Vec::with_capacity(args.repeat);
    let mut result_count: usize = 0;

    for run_idx in 0..args.repeat {
        let start = Instant::now();
        let results = search_puzzles(&mut conn, &filters)?;
        let elapsed_ms = start.elapsed().as_millis();
        run_times.push(elapsed_ms);
        result_count = results.len();

        if run_idx == 0 {
            let sample_count = results.len().min(10);
            if sample_count > 0 {
                eprintln!("sample_ids:");
                for p in &results[..sample_count] {
                    eprintln!("  {}", p.puzzle_id);
                }
                eprintln!();
            }
        }

        eprintln!("run_{}_{}ms", run_idx + 1, elapsed_ms);
    }

    // ── Summary metrics ──────────────────────────────────────────────

    let min_ms = run_times.iter().min().copied().unwrap_or(0);
    let max_ms = run_times.iter().max().copied().unwrap_or(0);
    let average_ms = if run_times.is_empty() {
        0.0
    } else {
        run_times.iter().sum::<u128>() as f64 / run_times.len() as f64
    };

    // ── DB integrity after ───────────────────────────────────────────

    let db_metadata_after = std::fs::metadata(&args.db)
        .map_err(|e| format!("cannot stat DB after: {}", e))?;
    let db_size_after = db_metadata_after.len();
    let db_mtime_after = db_metadata_after.modified()
        .map_err(|e| format!("cannot read mtime after: {}", e))?;

    let size_unchanged = db_size_before == db_size_after;
    let mtime_unchanged = db_mtime_before == db_mtime_after;

    // ── Print results ────────────────────────────────────────────────

    println!();
    println!("CMS-012 SEARCH BENCHMARK RESULTS");
    println!();
    println!("result_count: {}", result_count);
    println!();
    println!("run_times:");
    for (i, t) in run_times.iter().enumerate() {
        println!("  run_{}: {}ms", i + 1, t);
    }
    println!();
    println!("min_ms: {}", min_ms);
    println!("max_ms: {}", max_ms);
    println!("average_ms: {:.2}", average_ms);
    println!();
    println!("db_size_bytes_before: {}", db_size_before);
    println!("db_size_bytes_after: {}", db_size_after);
    println!("db_size_unchanged: {}", if size_unchanged { "YES" } else { "NO" });
    println!("db_modified_unchanged: {}", if mtime_unchanged { "YES" } else { "NO" });

    if !size_unchanged {
        eprintln!("ERROR: DB size changed during read-only benchmark!");
        return Err("DB integrity violation: size changed".into());
    }
    if !mtime_unchanged {
        eprintln!("ERROR: DB modification time changed during read-only benchmark!");
        return Err("DB integrity violation: mtime changed".into());
    }

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

    // 1. Valid args (minimal)
    #[test]
    fn test_parse_minimal_args() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.db, PathBuf::from("test.sqlite"));
                assert_eq!(parsed.min_rating, 0);
                assert_eq!(parsed.max_rating, 4000);
                assert_eq!(parsed.min_popularity, -100);
                assert_eq!(parsed.limit, 100);
                assert_eq!(parsed.side, SearchSide::Any);
                assert_eq!(parsed.repeat, 1);
                assert!(parsed.theme.is_none());
                assert!(parsed.opening.is_none());
            }
            ParseOutcome::Help => panic!("expected Run, got Help"),
        }
    }

    // 2. All filters
    #[test]
    fn test_parse_all_filters() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "1500".into(),
            "--max-rating".into(), "2000".into(),
            "--min-popularity".into(), "50".into(),
            "--theme".into(), "fork".into(),
            "--opening".into(), "Italian_Game".into(),
            "--side".into(), "white".into(),
            "--limit".into(), "50".into(),
            "--repeat".into(), "5".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => {
                assert_eq!(parsed.theme.as_deref(), Some("fork"));
                assert_eq!(parsed.opening.as_deref(), Some("Italian_Game"));
                assert_eq!(parsed.side, SearchSide::White);
                assert_eq!(parsed.repeat, 5);
            }
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 3. Missing db
    #[test]
    fn test_parse_missing_db() {
        let args = vec![
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 4. Missing min-rating
    #[test]
    fn test_parse_missing_min_rating() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 5. Missing max-rating
    #[test]
    fn test_parse_missing_max_rating() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 6. Missing popularity
    #[test]
    fn test_parse_missing_popularity() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--limit".into(), "100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 7. Missing limit
    #[test]
    fn test_parse_missing_limit() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 8. Side: any
    #[test]
    fn test_parse_side_any() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--side".into(), "any".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => assert_eq!(parsed.side, SearchSide::Any),
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 9. Side: white
    #[test]
    fn test_parse_side_white() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--side".into(), "white".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => assert_eq!(parsed.side, SearchSide::White),
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 10. Side: black
    #[test]
    fn test_parse_side_black() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--side".into(), "black".into(),
        ];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Run(parsed) => assert_eq!(parsed.side, SearchSide::Black),
            ParseOutcome::Help => panic!("expected Run"),
        }
    }

    // 11. Side: invalid
    #[test]
    fn test_parse_side_invalid() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--side".into(), "invalid".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 12. Limit 0
    #[test]
    fn test_parse_limit_zero() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "0".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 13. Limit 10001
    #[test]
    fn test_parse_limit_over_max() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "10001".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 14. Repeat 0
    #[test]
    fn test_parse_repeat_zero() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--repeat".into(), "0".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 15. Repeat 21
    #[test]
    fn test_parse_repeat_over_max() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--repeat".into(), "21".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 16. Unknown argument
    #[test]
    fn test_parse_unknown_argument() {
        let args = vec![
            "--db".into(), "test.sqlite".into(),
            "--min-rating".into(), "0".into(),
            "--max-rating".into(), "4000".into(),
            "--min-popularity".into(), "-100".into(),
            "--limit".into(), "100".into(),
            "--bogus".into(),
        ];
        assert!(parse_args_from(&args).is_err());
    }

    // 17. --help
    #[test]
    fn test_parse_help() {
        let args = vec!["--help".into()];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Help => {}
            ParseOutcome::Run(_) => panic!("expected Help"),
        }
    }

    // 18. -h
    #[test]
    fn test_parse_help_short() {
        let args = vec!["-h".into()];
        match parse_args_from(&args).unwrap() {
            ParseOutcome::Help => {}
            ParseOutcome::Run(_) => panic!("expected Help"),
        }
    }
}
