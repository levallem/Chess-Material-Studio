// CMS-012 — SQLite Puzzle Search Core
//
// Reusable search engine for the puzzles table.
// No dependency on UI, Iced, SearchTab, config, or Openings.

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

use crate::models::Puzzle;
use crate::schema::puzzles;

diesel::define_sql_function! {
    fn instr(haystack: diesel::sql_types::Text, needle: diesel::sql_types::Text) -> diesel::sql_types::Integer;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSide {
    Any,
    White,
    Black,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleSearchFilters {
    pub min_rating: i32,
    pub max_rating: i32,
    pub min_popularity: i32,
    pub theme_tag: Option<String>,
    pub opening_tag: Option<String>,
    pub side: SearchSide,
    pub limit: usize,
}

#[derive(QueryableByName)]
struct PuzzleSchemaColumn {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    r#type: String,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    notnull: i32,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    pk: i32,
}

const PUZZLES_SCHEMA: [(&str, &str, i32, i32); 10] = [
    ("puzzle_id", "TEXT", 1, 1),
    ("fen", "TEXT", 1, 0),
    ("moves", "TEXT", 1, 0),
    ("rating", "INTEGER", 1, 0),
    ("rating_deviation", "INTEGER", 1, 0),
    ("popularity", "INTEGER", 1, 0),
    ("nb_plays", "INTEGER", 1, 0),
    ("themes", "TEXT", 1, 0),
    ("game_url", "TEXT", 1, 0),
    ("opening_tags", "TEXT", 1, 0),
];

pub fn validate_puzzle_sqlite_db(path: &std::path::Path) -> Result<(), String> {
    validate_puzzle_sqlite_db_with_project_database(path, std::path::Path::new("ocp.db"))
}

fn validate_puzzle_sqlite_db_with_project_database(
    path: &std::path::Path,
    project_database: &std::path::Path,
) -> Result<(), String> {
    if path.file_name().is_some_and(|name| name == "ocp.db")
        || matches!(
            (path.canonicalize(), project_database.canonicalize()),
            (Ok(selected), Ok(project_database)) if selected == project_database
        )
    {
        return Err("ocp.db cannot be used as a puzzle database".into());
    }

    let metadata =
        std::fs::metadata(path).map_err(|e| format!("database file cannot be read: {}", e))?;
    if !metadata.is_file() {
        return Err("database path is not a file".into());
    }

    let path_str = path
        .to_str()
        .ok_or_else(|| "database path is not valid UTF-8".to_string())?;
    let mut conn = SqliteConnection::establish(path_str)
        .map_err(|e| format!("cannot open SQLite database: {}", e))?;
    let columns = diesel::sql_query("PRAGMA table_info(puzzles)")
        .load::<PuzzleSchemaColumn>(&mut conn)
        .map_err(|e| format!("cannot inspect puzzle database schema: {}", e))?;

    if columns.len() != PUZZLES_SCHEMA.len()
        || columns
            .iter()
            .zip(PUZZLES_SCHEMA)
            .any(|(column, expected)| {
                (
                    column.name.as_str(),
                    column.r#type.as_str(),
                    column.notnull,
                    column.pk,
                ) != expected
            })
    {
        return Err("database does not have the expected puzzles schema".into());
    }

    Ok(())
}

pub fn search_puzzles(
    conn: &mut SqliteConnection,
    filters: &PuzzleSearchFilters,
) -> Result<Vec<Puzzle>, String> {
    if filters.min_rating > filters.max_rating {
        return Err("min_rating must be <= max_rating".into());
    }
    if filters.limit == 0 {
        return Err("limit must be greater than 0".into());
    }
    let limit = i64::try_from(filters.limit)
        .map_err(|_| "limit overflow converting to i64".to_string())?;

    if filters.side != SearchSide::Any && filters.opening_tag.is_none() {
        return Err("side filter requires an opening filter".into());
    }

    let mut query = puzzles::table
        .into_boxed::<diesel::sqlite::Sqlite>()
        .filter(puzzles::dsl::rating.ge(filters.min_rating))
        .filter(puzzles::dsl::rating.le(filters.max_rating))
        .filter(puzzles::dsl::popularity.ge(filters.min_popularity))
        .limit(limit);

    if let Some(ref theme) = filters.theme_tag {
        query = query.filter(instr(puzzles::dsl::themes, theme).gt(0));
    }

    if let Some(ref opening) = filters.opening_tag {
        query = query.filter(instr(puzzles::dsl::opening_tags, opening).gt(0));
    }

    // Preserves legacy SearchTab semantics.
    // Do not change without a separate behavior decision.
    match filters.side {
        SearchSide::Any => {}
        SearchSide::White => {
            query = query.filter(instr(puzzles::dsl::game_url, "black").gt(0));
        }
        SearchSide::Black => {
            query = query.filter(instr(puzzles::dsl::game_url, "black").eq(0));
        }
    }

    query
        .load::<Puzzle>(conn)
        .map_err(|e| format!("query failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::puzzle_import::MIGRATIONS;
    use diesel_migrations::MigrationHarness;

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        crate::puzzle_import::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("fixture import should succeed");
        conn
    }

    fn setup_empty_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
    }

    fn default_filters() -> PuzzleSearchFilters {
        PuzzleSearchFilters {
            min_rating: 0,
            max_rating: 4000,
            min_popularity: -100,
            theme_tag: None,
            opening_tag: None,
            side: SearchSide::Any,
            limit: 10,
        }
    }

    fn ids(results: &[Puzzle]) -> Vec<&str> {
        results.iter().map(|p| p.puzzle_id.as_str()).collect()
    }

    // 20. ALL — no filters
    #[test]
    fn test_all_puzzles() {
        let mut conn = setup_test_db();
        let results = search_puzzles(&mut conn, &default_filters()).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00008", "00009", "00010", "00011"]);
    }

    // 21. RATING
    #[test]
    fn test_rating_filter() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            min_rating: 1500,
            max_rating: 1700,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00008", "00009", "00010"]);
    }

    // 22. POPULARITY
    #[test]
    fn test_popularity_filter() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            min_popularity: 90,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00008", "00010"]);
    }

    // 23. THEME FORK
    #[test]
    fn test_theme_fork() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            theme_tag: Some("fork".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00008", "00010"]);
    }

    // 24. THEME SACRIFICE
    #[test]
    fn test_theme_sacrifice() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            theme_tag: Some("sacrifice".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00010"]);
    }

    // 25. OPENING
    #[test]
    fn test_opening_filter() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00008", "00010"]);
    }

    // 26. COMBINED
    #[test]
    fn test_combined_filters() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            min_rating: 1600,
            max_rating: 1800,
            min_popularity: 0,
            theme_tag: Some("fork".into()),
            opening_tag: Some("Italian_Game".into()),
            side: SearchSide::Any,
            limit: 10,
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["00010"]);
    }

    // 27. SIDE LEGACY SEMANTICS
    #[test]
    fn test_side_legacy_black() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            side: SearchSide::Black,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        // Fixture has no "black" in game_url, so Black (NOT LIKE %black%) returns all
        assert_eq!(got, vec!["00008", "00010"]);
    }

    #[test]
    fn test_side_legacy_white() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            side: SearchSide::White,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        // Fixture has no "black" in game_url, so White (LIKE %black%) returns none
        assert!(results.is_empty());
    }

    // 28. LIMIT
    #[test]
    fn test_limit() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            limit: 1,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        assert_eq!(results.len(), 1);
    }

    // 29. NO MATCH
    #[test]
    fn test_no_match() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            min_rating: 3900,
            max_rating: 4000,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        assert!(results.is_empty());
    }

    // 30. INVALID RANGE
    #[test]
    fn test_invalid_range() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            min_rating: 2000,
            max_rating: 1000,
            ..default_filters()
        };
        assert!(search_puzzles(&mut conn, &filters).is_err());
    }

    // 31. LIMIT ZERO
    #[test]
    fn test_limit_zero() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            limit: 0,
            ..default_filters()
        };
        assert!(search_puzzles(&mut conn, &filters).is_err());
    }

    // 32. SIDE WITHOUT OPENING
    #[test]
    fn test_side_without_opening_white() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            side: SearchSide::White,
            ..default_filters()
        };
        assert!(search_puzzles(&mut conn, &filters).is_err());
    }

    #[test]
    fn test_side_without_opening_black() {
        let mut conn = setup_test_db();
        let filters = PuzzleSearchFilters {
            side: SearchSide::Black,
            ..default_filters()
        };
        assert!(search_puzzles(&mut conn, &filters).is_err());
    }

    // CMS-013: instr literal semantics — underscore is NOT wildcard
    #[test]
    fn test_underscore_is_literal() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,fork,https://url.com,Italian_Game,\nB,fen,e1e8,1500,70,95,10000,fork,https://url.com,ItalianXGame,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["A"]);
    }

    // CMS-013: instr literal semantics — percent is NOT wildcard
    #[test]
    fn test_percent_is_literal() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,fork,https://url.com,Test%Line,\nB,fen,e1e8,1500,70,95,10000,fork,https://url.com,TestXLine,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Test%Line".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["A"]);
    }

    // CMS-013: instr is case-sensitive — reproduces String::contains
    #[test]
    fn test_case_sensitivity() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,fork,https://url.com,Italian_Game,\nB,fen,e1e8,1500,70,95,10000,fork,https://url.com,italian_game,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["A"]);
    }

    // CMS-013: theme also uses instr
    #[test]
    fn test_theme_uses_instr() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,myfork_x,https://url.com,,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            theme_tag: Some("fork".into()),
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["A"]);
    }

    // CMS-013: side White is case-sensitive — only lowercase "black" matches
    #[test]
    fn test_side_white_case_sensitive() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,fork,https://example/black,Italian_Game,\nB,fen,e1e8,1500,70,95,10000,fork,https://example/Black,Italian_Game,\nC,fen,e1e8,1500,70,95,10000,fork,https://example/white,Italian_Game,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            side: SearchSide::White,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["A"], "only lowercase 'black' should match (case-sensitive)");
    }

    // CMS-013: side Black excludes only lowercase "black"
    #[test]
    fn test_side_black_case_sensitive() {
        let mut conn = setup_empty_db();
        crate::puzzle_import::import_puzzles_from_reader(
            &mut conn,
            "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\nA,fen,e1e8,1500,70,95,10000,fork,https://example/black,Italian_Game,\nB,fen,e1e8,1500,70,95,10000,fork,https://example/Black,Italian_Game,\nC,fen,e1e8,1500,70,95,10000,fork,https://example/white,Italian_Game,\n".as_bytes(),
        ).unwrap();
        let filters = PuzzleSearchFilters {
            opening_tag: Some("Italian_Game".into()),
            side: SearchSide::Black,
            ..default_filters()
        };
        let results = search_puzzles(&mut conn, &filters).unwrap();
        let mut got = ids(&results);
        got.sort();
        assert_eq!(got, vec!["B", "C"], "Black side excludes only lowercase 'black'");
    }

    static TEMP_DB_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct TempPuzzleDb {
        path: std::path::PathBuf,
        directory: Option<std::path::PathBuf>,
    }

    impl TempPuzzleDb {
        fn new(label: &str) -> Self {
            Self {
                path: unique_temp_db_path(label, ".sqlite"),
                directory: None,
            }
        }

        fn ocp_db(label: &str) -> Self {
            let directory = unique_temp_db_path(label, "");
            std::fs::create_dir(&directory).expect("test directory should be created");
            Self {
                path: directory.join("ocp.db"),
                directory: Some(directory),
            }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempPuzzleDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            if let Some(directory) = &self.directory {
                let _ = std::fs::remove_dir(directory);
            }
        }
    }

    fn unique_temp_db_path(label: &str, suffix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after UNIX epoch")
            .as_nanos();
        let sequence = TEMP_DB_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "cms-puzzle-db-validation-{label}-{}-{nanos}-{sequence}{suffix}",
            std::process::id()
        ))
    }

    fn create_puzzle_db(path: &std::path::Path) {
        let mut conn =
            SqliteConnection::establish(path.to_str().unwrap()).expect("test database should open");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("test database schema should migrate");
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_accepts_valid_db() {
        let database = TempPuzzleDb::new("valid");
        create_puzzle_db(database.path());
        {
            let mut conn = SqliteConnection::establish(database.path().to_str().unwrap())
                .expect("test database should reopen");
            crate::puzzle_import::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
                .expect("test puzzle should import");
        }

        validate_puzzle_sqlite_db(database.path())
            .expect("valid puzzle database should pass validation");
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_accepts_valid_empty_db() {
        let database = TempPuzzleDb::new("empty");
        create_puzzle_db(database.path());

        validate_puzzle_sqlite_db(database.path())
            .expect("valid empty puzzle database should pass validation");
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_rejects_missing_file() {
        let database = TempPuzzleDb::new("missing");
        assert!(validate_puzzle_sqlite_db(database.path()).is_err());
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_rejects_non_sqlite_file() {
        let database = TempPuzzleDb::new("non-sqlite");
        std::fs::write(database.path(), b"not a SQLite database")
            .expect("test file should be written");

        assert!(validate_puzzle_sqlite_db(database.path()).is_err());
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_rejects_missing_schema() {
        let database = TempPuzzleDb::new("missing-schema");
        {
            let mut conn = SqliteConnection::establish(database.path().to_str().unwrap())
                .expect("test database should open");
            diesel::sql_query("CREATE TABLE puzzles (puzzle_id TEXT NOT NULL PRIMARY KEY)")
                .execute(&mut conn)
                .expect("incomplete schema should be created");
        }

        assert!(validate_puzzle_sqlite_db(database.path()).is_err());
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_rejects_ocp_db_basename() {
        let database = TempPuzzleDb::ocp_db("ocp-basename");
        create_puzzle_db(database.path());

        assert!(validate_puzzle_sqlite_db(database.path()).is_err());
    }

    #[test]
    fn test_validate_puzzle_sqlite_db_rejects_project_database_equivalent_path() {
        let project_db = TempPuzzleDb::new("project-database");
        create_puzzle_db(project_db.path());
        let equivalent_path = project_db
            .path()
            .parent()
            .expect("temporary database should have a parent")
            .join(".")
            .join(
                project_db
                    .path()
                    .file_name()
                    .expect("temporary database should have a name"),
            );

        assert!(
            validate_puzzle_sqlite_db_with_project_database(&equivalent_path, project_db.path(),)
                .is_err()
        );
    }
}
