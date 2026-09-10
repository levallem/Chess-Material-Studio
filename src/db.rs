use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::config::{self, Puzzle};
use crate::models::NewFavorite;
use crate::schema::favs;
use crate::schema::favs::dsl::*;

use crate::openings::{Openings, Variation};
use crate::search_tab::{OpeningSide, TacticalThemes};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub fn establish_connection() -> Result<SqliteConnection, String> {
    establish_connection_at(config::DATABASE_URL)
}

fn establish_connection_at(database_url: &str) -> Result<SqliteConnection, String> {
    let mut connection = SqliteConnection::establish(database_url)
        .map_err(|error| format!("cannot open favorites database {database_url}: {error}"))?;
    connection
        .run_pending_migrations(MIGRATIONS)
        .map_err(|error| format!("cannot migrate favorites database {database_url}: {error}"))?;

    Ok(connection)
}

pub fn get_favorites(
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: TacticalThemes,
    opening: Openings,
    variation: Variation,
    op_side: Option<OpeningSide>,
    result_limit: usize,
) -> Result<Vec<Puzzle>, String> {
    let mut conn = establish_connection()?;
    get_favorites_with_connection(
        &mut conn,
        min_rating,
        max_rating,
        min_popularity,
        theme,
        opening,
        variation,
        op_side,
        result_limit,
    )
}

fn get_favorites_with_connection(
    conn: &mut SqliteConnection,
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: TacticalThemes,
    opening: Openings,
    variation: Variation,
    op_side: Option<OpeningSide>,
    result_limit: usize,
) -> Result<Vec<Puzzle>, String> {
    let results;
    let theme_filter = String::from("%") + theme.get_tag_name() + "%";
    let limit = result_limit as i64;
    if opening == Openings::Any {
        results = favs
            .filter(rating.between(min_rating, max_rating))
            .filter(popularity.ge(min_popularity))
            .filter(themes.like(theme_filter))
            .limit(limit)
            .load::<Puzzle>(conn);
    } else {
        let opening_tag: &str = if variation.name != Variation::ANY_STR {
            &variation.name
        } else {
            opening.get_field_name()
        };
        let opening_filter = opening_tags.like(String::from("%") + opening_tag + "%");
        let side = match op_side {
            None => OpeningSide::Any,
            Some(x) => x,
        };
        if side == OpeningSide::White {
            results = favs
                .filter(rating.between(min_rating, max_rating))
                .filter(popularity.ge(min_popularity))
                .filter(themes.like(theme_filter))
                .filter(opening_filter)
                .filter(game_url.like("%black%"))
                .limit(limit)
                .load::<Puzzle>(conn);
        } else if side == OpeningSide::Black {
            results = favs
                .filter(rating.between(min_rating, max_rating))
                .filter(popularity.ge(min_popularity))
                .filter(themes.like(theme_filter))
                .filter(opening_filter)
                .filter(game_url.not_like("%black%"))
                .limit(limit)
                .load::<Puzzle>(conn);
        } else {
            results = favs
                .filter(rating.between(min_rating, max_rating))
                .filter(popularity.ge(min_popularity))
                .filter(themes.like(theme_filter))
                .filter(opening_filter)
                .limit(limit)
                .load::<Puzzle>(conn);
        }
    }
    results.map_err(|error| format!("cannot query favorites: {error}"))
}

pub fn is_favorite(id: &str) -> Result<bool, String> {
    let mut conn = establish_connection()?;
    is_favorite_with_connection(&mut conn, id)
}

fn is_favorite_with_connection(conn: &mut SqliteConnection, id: &str) -> Result<bool, String> {
    match favs.filter(puzzle_id.eq(id)).first::<Puzzle>(conn) {
        Ok(_) => Ok(true),
        Err(diesel::result::Error::NotFound) => Ok(false),
        Err(error) => Err(format!("cannot determine whether favorite {id} exists: {error}")),
    }
}

pub fn toggle_favorite(puzzle: Puzzle) -> Result<bool, String> {
    let mut conn = establish_connection()?;
    toggle_favorite_with_connection(&mut conn, puzzle)
}

fn toggle_favorite_with_connection(
    conn: &mut SqliteConnection,
    puzzle: Puzzle,
) -> Result<bool, String> {
    let is_fav = is_favorite_with_connection(conn, &puzzle.puzzle_id)?;

    if is_fav {
        diesel::delete(favs::table)
            .filter(puzzle_id.eq(&puzzle.puzzle_id))
            .execute(conn)
            .map_err(|error| format!("cannot remove favorite {}: {error}", puzzle.puzzle_id))?;
        Ok(false)
    } else {
        let new_fav = NewFavorite {
            puzzle_id: &puzzle.puzzle_id,
            fen: &puzzle.fen,
            moves: &puzzle.moves,
            rating: puzzle.rating,
            rd: puzzle.rating_deviation,
            popularity: puzzle.popularity,
            nb_plays: puzzle.nb_plays,
            themes: &puzzle.themes,
            game_url: &puzzle.game_url,
            opening_tags: &puzzle.opening,
        };

        diesel::insert_into(favs::table)
            .values(&new_fav)
            .execute(conn)
            .map_err(|error| format!("cannot save favorite {}: {error}", puzzle.puzzle_id))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        let sequence = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&directory).expect("test directory should be created");
        directory.join(format!("favorites_{label}_{}_{}.sqlite", std::process::id(), sequence))
    }

    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
    }

    fn test_puzzle(id: &str) -> Puzzle {
        Puzzle {
            puzzle_id: id.to_string(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".to_string(),
            moves: "a1a2".to_string(),
            rating: 1500,
            rating_deviation: 80,
            popularity: 50,
            nb_plays: 10,
            themes: "hangingPiece".to_string(),
            game_url: "https://lichess.org/game".to_string(),
            opening: String::new(),
        }
    }

    fn all_favorites(conn: &mut SqliteConnection) -> Result<Vec<Puzzle>, String> {
        get_favorites_with_connection(
            conn,
            0,
            4000,
            -100,
            TacticalThemes::All,
            Openings::Any,
            Variation::ANY.clone(),
            Some(OpeningSide::Any),
            100,
        )
    }

    #[test]
    fn test_favs_table_still_exists() {
        let mut conn = setup_test_db();
        let count: i64 = crate::schema::favs::table
            .count()
            .get_result(&mut conn)
            .expect("favs table should still exist after new migration");
        assert_eq!(count, 0, "favs table should be empty in test DB");
    }

    #[test]
    fn test_migrations_are_idempotent() {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");

        conn.run_pending_migrations(MIGRATIONS)
            .expect("First migration run should succeed");

        conn.run_pending_migrations(MIGRATIONS)
            .expect("Second migration run should succeed (idempotent)");
    }

    #[test]
    fn establish_connection_migrates_a_new_controlled_database() {
        let path = temp_db_path("new");
        let mut conn = establish_connection_at(path.to_str().expect("test path should be UTF-8"))
            .expect("new controlled database should migrate");
        let count: i64 = favs::table.count().get_result(&mut conn).expect("count favorites");
        assert_eq!(count, 0);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn establish_connection_propagates_open_failure() {
        let path = temp_db_path("missing_parent").join("ocp.db");
        let result = establish_connection_at(path.to_str().expect("test path should be UTF-8"));
        match result {
            Ok(_) => panic!("opening a database below a missing parent should fail"),
            Err(error) => assert!(error.contains("cannot open favorites database")),
        }
    }

    #[test]
    fn establish_connection_propagates_migration_failure() {
        let path = temp_db_path("migration_failure");
        let mut conn = SqliteConnection::establish(path.to_str().expect("test path should be UTF-8"))
            .expect("controlled database should open");
        diesel::sql_query("CREATE TABLE favs (puzzle_id TEXT PRIMARY KEY)")
            .execute(&mut conn)
            .expect("incompatible table should be created");
        drop(conn);

        let result = establish_connection_at(path.to_str().expect("test path should be UTF-8"));
        match result {
            Ok(_) => panic!("migrating an incompatible table should fail"),
            Err(error) => assert!(error.contains("cannot migrate favorites database")),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_favorites_distinguishes_empty_and_populated_results() {
        let mut conn = setup_test_db();
        assert!(all_favorites(&mut conn).expect("empty query should succeed").is_empty());

        toggle_favorite_with_connection(&mut conn, test_puzzle("favorite-one"))
            .expect("insert should succeed");
        let results = all_favorites(&mut conn).expect("populated query should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].puzzle_id, "favorite-one");
    }

    #[test]
    fn get_favorites_propagates_query_failure() {
        let mut conn = setup_test_db();
        diesel::sql_query("DROP TABLE favs")
            .execute(&mut conn)
            .expect("favorites table should drop");

        let result = all_favorites(&mut conn);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot query favorites"));
    }

    #[test]
    fn is_favorite_distinguishes_absent_and_existing_puzzles() {
        let mut conn = setup_test_db();
        assert!(!is_favorite_with_connection(&mut conn, "absent").expect("absence should not fail"));

        toggle_favorite_with_connection(&mut conn, test_puzzle("present"))
            .expect("insert should succeed");
        assert!(is_favorite_with_connection(&mut conn, "present").expect("presence should not fail"));
    }

    #[test]
    fn toggle_favorite_inserts_then_removes() {
        let mut conn = setup_test_db();
        let puzzle = test_puzzle("toggle");

        assert!(toggle_favorite_with_connection(&mut conn, puzzle.clone()).expect("insert should succeed"));
        assert!(!toggle_favorite_with_connection(&mut conn, puzzle).expect("delete should succeed"));
        assert!(!is_favorite_with_connection(&mut conn, "toggle").expect("absence should not fail"));
    }

    #[test]
    fn toggle_favorite_propagates_insert_failure() {
        let mut conn = setup_test_db();
        diesel::sql_query(
            "CREATE TRIGGER fail_favorite_insert BEFORE INSERT ON favs BEGIN SELECT RAISE(ABORT, 'insert failure'); END",
        )
        .execute(&mut conn)
        .expect("failure trigger should be created");

        let result = toggle_favorite_with_connection(&mut conn, test_puzzle("insert_failure"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot save favorite insert_failure"));
    }

    #[test]
    fn toggle_favorite_propagates_delete_failure() {
        let mut conn = setup_test_db();
        let puzzle = test_puzzle("delete_failure");
        toggle_favorite_with_connection(&mut conn, puzzle.clone()).expect("insert should succeed");
        diesel::sql_query(
            "CREATE TRIGGER fail_favorite_delete BEFORE DELETE ON favs BEGIN SELECT RAISE(ABORT, 'delete failure'); END",
        )
        .execute(&mut conn)
        .expect("failure trigger should be created");

        let result = toggle_favorite_with_connection(&mut conn, puzzle);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot remove favorite delete_failure"));
    }
}
