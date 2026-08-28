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

pub fn establish_connection() -> SqliteConnection {
    let mut connection = SqliteConnection::establish(config::DATABASE_URL)
        .unwrap_or_else(|_| panic!("Error connecting to {}", config::DATABASE_URL));
    let _ = connection.run_pending_migrations(MIGRATIONS);

    connection
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
) -> Option<Vec<Puzzle>> {
    let mut conn = establish_connection();
    let results;
    let theme_filter = String::from("%") + theme.get_tag_name() + "%";
    let limit = result_limit as i64;
    if opening == Openings::Any {
        results = favs
            .filter(rating.between(min_rating, max_rating))
            .filter(popularity.ge(min_popularity))
            .filter(themes.like(theme_filter))
            .limit(limit)
            .load::<Puzzle>(&mut conn);
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
                .load::<Puzzle>(&mut conn);
        } else if side == OpeningSide::Black {
            results = favs
                .filter(rating.between(min_rating, max_rating))
                .filter(popularity.ge(min_popularity))
                .filter(themes.like(theme_filter))
                .filter(opening_filter)
                .filter(game_url.not_like("%black%"))
                .limit(limit)
                .load::<Puzzle>(&mut conn);
        } else {
            results = favs
                .filter(rating.between(min_rating, max_rating))
                .filter(popularity.ge(min_popularity))
                .filter(themes.like(theme_filter))
                .filter(opening_filter)
                .limit(limit)
                .load::<Puzzle>(&mut conn);
        }
    }
    results.ok()
}

pub fn is_favorite(id: &str) -> bool {
    let mut conn = establish_connection();
    let results = favs.filter(puzzle_id.eq(id)).first::<Puzzle>(&mut conn);

    results.is_ok()
}

pub fn toggle_favorite(puzzle: Puzzle) {
    let mut conn = establish_connection();
    let is_fav = favs
        .filter(puzzle_id.eq(&puzzle.puzzle_id))
        .first::<Puzzle>(&mut conn)
        .is_ok();

    if is_fav {
        diesel::delete(favs::table)
            .filter(puzzle_id.eq(&puzzle.puzzle_id))
            .execute(&mut conn)
            .expect("Error removing favorite");
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
            .execute(&mut conn)
            .expect("Error saving new favorite");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewPuzzle;

    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
    }

    fn test_puzzle<'a>() -> NewPuzzle<'a> {
        NewPuzzle {
            puzzle_id: "test_puzzle_001",
            fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            moves: "e7e5 g1f3",
            rating: 1500,
            rating_deviation: 70,
            popularity: 95,
            nb_plays: 10000,
            themes: "fork opening",
            game_url: "https://lichess.org/training/abc123",
            opening_tags: "Italian_Game",
        }
    }

    #[test]
    fn test_migration_creates_puzzles_table() {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        let result = conn.run_pending_migrations(MIGRATIONS);
        assert!(
            result.is_ok(),
            "Migrations should succeed on empty database"
        );
    }

    #[test]
    fn test_puzzles_table_exists_after_migration() {
        let mut conn = setup_test_db();
        let count: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("puzzles table should exist after migration");
        assert_eq!(count, 0, "puzzles table should be empty after migration");
    }

    #[test]
    fn test_insert_and_query_puzzle() {
        let mut conn = setup_test_db();
        let puzzle = test_puzzle();

        let inserted = diesel::insert_into(crate::schema::puzzles::table)
            .values(&puzzle)
            .execute(&mut conn);
        assert!(inserted.is_ok(), "Should insert puzzle successfully");
        assert_eq!(inserted.unwrap(), 1, "Should insert exactly one row");

        let result: Puzzle = crate::schema::puzzles::table
            .filter(crate::schema::puzzles::dsl::puzzle_id.eq("test_puzzle_001"))
            .first::<Puzzle>(&mut conn)
            .expect("Should retrieve puzzle");

        assert_eq!(result.puzzle_id, "test_puzzle_001");
        assert_eq!(
            result.fen,
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        assert_eq!(result.moves, "e7e5 g1f3");
        assert_eq!(result.rating, 1500);
        assert_eq!(result.rating_deviation, 70);
        assert_eq!(result.popularity, 95);
        assert_eq!(result.nb_plays, 10000);
        assert_eq!(result.themes, "fork opening");
        assert_eq!(result.game_url, "https://lichess.org/training/abc123");
        assert_eq!(result.opening, "Italian_Game");
    }

    #[test]
    fn test_puzzle_id_is_unique() {
        let mut conn = setup_test_db();
        let puzzle = test_puzzle();

        diesel::insert_into(crate::schema::puzzles::table)
            .values(&puzzle)
            .execute(&mut conn)
            .expect("First insert should succeed");

        let duplicate = NewPuzzle {
            puzzle_id: "test_puzzle_001",
            fen: "some/other/fen",
            moves: "a1a2",
            rating: 2000,
            rating_deviation: 50,
            popularity: 80,
            nb_plays: 5000,
            themes: "mate",
            game_url: "https://lichess.org/training/xyz789",
            opening_tags: "Sicilian_Defense",
        };

        let result = diesel::insert_into(crate::schema::puzzles::table)
            .values(&duplicate)
            .execute(&mut conn);
        assert!(result.is_err(), "Duplicate puzzle_id should fail");
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
}
