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

    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to open in-memory database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
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
