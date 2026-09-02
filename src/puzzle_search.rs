// CMS-012 — SQLite Puzzle Search Core
//
// Reusable search engine for the puzzles table.
// No dependency on UI, Iced, SearchTab, config, or Openings.

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

use crate::models::Puzzle;
use crate::schema::puzzles;

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
        let pattern = format!("%{}%", theme);
        query = query.filter(puzzles::dsl::themes.like(pattern));
    }

    if let Some(ref opening) = filters.opening_tag {
        let pattern = format!("%{}%", opening);
        query = query.filter(puzzles::dsl::opening_tags.like(pattern));
    }

    // Preserves legacy SearchTab semantics.
    // Do not change without a separate behavior decision.
    match filters.side {
        SearchSide::Any => {}
        SearchSide::White => {
            query = query.filter(puzzles::dsl::game_url.like("%black%"));
        }
        SearchSide::Black => {
            query = query.filter(puzzles::dsl::game_url.not_like("%black%"));
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
}
