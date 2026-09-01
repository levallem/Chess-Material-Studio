use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::config::{self, Puzzle};
use crate::models::{NewFavorite, NewPuzzle};
use crate::schema::favs;
use crate::schema::favs::dsl::*;
use crate::schema::puzzle_import_progress;

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

pub fn import_puzzles_from_reader<R: std::io::Read>(
    conn: &mut SqliteConnection,
    source: R,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(source);

    let mut count = 0usize;
    for result in reader.deserialize::<Puzzle>() {
        let puzzle = result?;
        let new_puzzle = NewPuzzle {
            puzzle_id: &puzzle.puzzle_id,
            fen: &puzzle.fen,
            moves: &puzzle.moves,
            rating: puzzle.rating,
            rating_deviation: puzzle.rating_deviation,
            popularity: puzzle.popularity,
            nb_plays: puzzle.nb_plays,
            themes: &puzzle.themes,
            game_url: &puzzle.game_url,
            opening_tags: &puzzle.opening,
        };
        diesel::insert_into(crate::schema::puzzles::table)
            .values(&new_puzzle)
            .execute(conn)?;
        count += 1;
    }
    Ok(count)
}

pub fn import_puzzles_from_reader_transactional<R: std::io::Read>(
    conn: &mut SqliteConnection,
    source: R,
) -> Result<usize, Box<dyn std::error::Error>> {
    conn.transaction(|conn| {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(source);

        let mut count = 0usize;
        for result in reader.deserialize::<Puzzle>() {
            let puzzle = result?;
            let new_puzzle = NewPuzzle {
                puzzle_id: &puzzle.puzzle_id,
                fen: &puzzle.fen,
                moves: &puzzle.moves,
                rating: puzzle.rating,
                rating_deviation: puzzle.rating_deviation,
                popularity: puzzle.popularity,
                nb_plays: puzzle.nb_plays,
                themes: &puzzle.themes,
                game_url: &puzzle.game_url,
                opening_tags: &puzzle.opening,
            };
            diesel::insert_into(crate::schema::puzzles::table)
                .values(&new_puzzle)
                .execute(conn)?;
            count += 1;
        }
        Ok(count)
    })
}

// CMS-006 — Chunked import with checkpoint resume
//
// Resume is safe ONLY when the same source_key represents
// exactly the same source file with the same row order.
// Automatic source identity validation is out of scope.

fn get_checkpoint(
    conn: &mut SqliteConnection,
    source_key: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    let result = puzzle_import_progress::table
        .filter(puzzle_import_progress::dsl::source_key.eq(source_key))
        .select(puzzle_import_progress::dsl::completed_rows)
        .first::<i64>(conn);
    match result {
        Ok(rows) => Ok(rows),
        Err(diesel::result::Error::NotFound) => Ok(0),
        Err(e) => Err(Box::new(e)),
    }
}

fn upsert_checkpoint(
    conn: &mut SqliteConnection,
    source_key: &str,
    completed_rows: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let existing = puzzle_import_progress::table
        .filter(puzzle_import_progress::dsl::source_key.eq(source_key))
        .first::<(String, i64)>(conn);
    match existing {
        Ok(_) => {
            diesel::update(puzzle_import_progress::table)
                .filter(puzzle_import_progress::dsl::source_key.eq(source_key))
                .set(puzzle_import_progress::dsl::completed_rows.eq(completed_rows))
                .execute(conn)?;
        }
        Err(diesel::result::Error::NotFound) => {
            diesel::insert_into(puzzle_import_progress::table)
                .values((
                    puzzle_import_progress::dsl::source_key.eq(source_key),
                    puzzle_import_progress::dsl::completed_rows.eq(completed_rows),
                ))
                .execute(conn)?;
        }
        Err(e) => return Err(Box::new(e)),
    }
    Ok(())
}

pub fn import_puzzles_from_reader_chunked<R: std::io::Read>(
    conn: &mut SqliteConnection,
    source: R,
    source_key: &str,
    chunk_size: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    if chunk_size == 0 {
        return Err("chunk_size must be greater than 0".into());
    }
    if source_key.is_empty() {
        return Err("source_key must not be empty".into());
    }

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(source);

    let completed_rows = get_checkpoint(conn, source_key)?;
    if completed_rows < 0 {
        return Err("completed_rows is negative".into());
    }

    // Deserialize using header-based mapping (Puzzle's serde rename attributes).
    // DailyDate is an unknown CSV field for Puzzle and is ignored
    // during header-based Serde deserialization.
    let mut iter = reader.deserialize::<Puzzle>();

    // Skip already-confirmed rows, validating deserialization on each.
    for _ in 0..completed_rows {
        iter.next()
            .ok_or("checkpoint exceeds source rows — \
                     source_key may refer to a different source")??;
    }

    let mut total_inserted = 0usize;
    let mut previous_completed = completed_rows;

    loop {
        let mut chunk: Vec<Puzzle> = Vec::with_capacity(chunk_size);
        for _ in 0..chunk_size {
            match iter.next() {
                Some(Ok(puzzle)) => chunk.push(puzzle),
                Some(Err(e)) => return Err(e.into()),
                None => break,
            }
        }

        if chunk.is_empty() {
            break;
        }

        let new_completed = previous_completed + chunk.len() as i64;

        conn.transaction(|conn| {
            for puzzle in &chunk {
                let new_puzzle = NewPuzzle {
                    puzzle_id: &puzzle.puzzle_id,
                    fen: &puzzle.fen,
                    moves: &puzzle.moves,
                    rating: puzzle.rating,
                    rating_deviation: puzzle.rating_deviation,
                    popularity: puzzle.popularity,
                    nb_plays: puzzle.nb_plays,
                    themes: &puzzle.themes,
                    game_url: &puzzle.game_url,
                    opening_tags: &puzzle.opening,
                };
                diesel::insert_into(crate::schema::puzzles::table)
                    .values(&new_puzzle)
                    .execute(conn)?;
            }
            upsert_checkpoint(conn, source_key, new_completed)?;
            Ok::<_, Box<dyn std::error::Error>>(())
        })?;

        total_inserted += chunk.len();
        previous_completed = new_completed;
    }

    Ok(total_inserted)
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

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

    #[test]
    fn test_importer_inserts_all_fixture_puzzles() {
        let mut conn = setup_test_db();
        let count = super::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("import should succeed");
        assert_eq!(count, 4, "fixture contains 4 puzzles");
    }

    #[test]
    fn test_importer_puzzle_count_matches_sqlite() {
        let mut conn = setup_test_db();
        super::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("import should succeed");

        let count: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count query should succeed");
        assert_eq!(count, 4, "SQLite should contain exactly 4 rows");
    }

    #[test]
    fn test_importer_round_trip_all_fields() {
        let mut conn = setup_test_db();
        super::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("import should succeed");

        let p: Puzzle = crate::schema::puzzles::table
            .filter(crate::schema::puzzles::dsl::puzzle_id.eq("00010"))
            .first::<Puzzle>(&mut conn)
            .expect("puzzle 00010 should exist");

        assert_eq!(p.puzzle_id, "00010");
        assert_eq!(p.fen, "r1bqkb1r/pppppppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 2 3");
        assert_eq!(p.moves, "f3g5 e7e6 g5f7");
        assert_eq!(p.rating, 1700);
        assert_eq!(p.rating_deviation, 80);
        assert_eq!(p.popularity, 92);
        assert_eq!(p.nb_plays, 7500);
        assert_eq!(p.themes, "fork sacrifice middlegame");
        assert_eq!(p.game_url, "https://lichess.org/training/ghi789");
        assert_eq!(p.opening, "Italian_Game");
    }

    #[test]
    fn test_importer_empty_opening_tags_preserved() {
        let mut conn = setup_test_db();
        super::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("import should succeed");

        let p: Puzzle = crate::schema::puzzles::table
            .filter(crate::schema::puzzles::dsl::puzzle_id.eq("00009"))
            .first::<Puzzle>(&mut conn)
            .expect("puzzle 00009 should exist");
        assert_eq!(p.opening, "", "empty OpeningTags should be empty string, not NULL");
    }

    #[test]
    fn test_importer_daily_date_ignored() {
        let mut conn = setup_test_db();
        let result = super::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes());
        assert!(result.is_ok(), "importing CSV with DailyDate column should succeed");

        let p: Puzzle = crate::schema::puzzles::table
            .filter(crate::schema::puzzles::dsl::puzzle_id.eq("00010"))
            .first::<Puzzle>(&mut conn)
            .expect("puzzle 00010 should exist");
        assert_eq!(p.rating, 1700, "DailyDate should be ignored, puzzle parsed correctly");
    }

    #[test]
    fn test_importer_invalid_csv_returns_error() {
        let mut conn = setup_test_db();
        let bad_csv = b"PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\n00008,fen,e1e8,invalid,70,95,10000,fork,https://url.com,,\n";
        let result = super::import_puzzles_from_reader(&mut conn, &bad_csv[..]);
        assert!(result.is_err(), "invalid Rating should produce an error");
    }

    #[test]
    fn test_transactional_importer_inserts_all_fixture_puzzles() {
        let mut conn = setup_test_db();
        let count =
            super::import_puzzles_from_reader_transactional(&mut conn, FIXTURE.as_bytes())
                .expect("transactional import should succeed");
        assert_eq!(count, 4, "fixture contains 4 puzzles");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count query should succeed");
        assert_eq!(rows, 4, "SQLite should contain exactly 4 rows");
    }

    #[test]
    fn test_transactional_importer_rolls_back_on_invalid_row() {
        let mut conn = setup_test_db();
        let csv_with_invalid =
            b"PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate\n00008,N4k3/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1,e1e8,1500,70,95,10000,fork,https://lichess.org/training/abc123,Italian_Game,\n00009,fen,other,not_a_number,70,95,10000,mate,https://lichess.org/training/xyz789,,\n";

        let result =
            super::import_puzzles_from_reader_transactional(&mut conn, &csv_with_invalid[..]);
        assert!(
            result.is_err(),
            "a valid row followed by an invalid row should fail the transaction"
        );

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count query should succeed");
        assert_eq!(rows, 0, "entire transaction should roll back, no partial rows");
    }

    // ── CMS-006 tests ──────────────────────────────────────────────

    #[test]
    fn test_cms006_migration_creates_checkpoint_table() {
        let mut conn = setup_test_db();
        let count: i64 = crate::schema::puzzle_import_progress::table
            .count()
            .get_result(&mut conn)
            .expect("puzzle_import_progress table should exist");
        assert_eq!(count, 0, "checkpoint table should start empty");
    }

    #[test]
    fn test_cms006_normal_chunked_import() {
        let mut conn = setup_test_db();
        let inserted = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "fixture-v1",
            2,
        )
        .expect("chunked import should succeed");
        assert_eq!(inserted, 4, "fixture contains 4 puzzles");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count query should succeed");
        assert_eq!(rows, 4, "SQLite should contain exactly 4 rows");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq("fixture-v1"))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint should exist");
        assert_eq!(cp, 4, "checkpoint should equal total rows");
    }

    #[test]
    fn test_cms006_second_run_does_not_duplicate() {
        let mut conn = setup_test_db();

        super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "fixture-v1",
            2,
        )
        .expect("first import");

        let inserted = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "fixture-v1",
            2,
        )
        .expect("second import");
        assert_eq!(inserted, 0, "no new puzzles should be inserted");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count query should succeed");
        assert_eq!(rows, 4, "still exactly 4 rows");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq("fixture-v1"))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint should exist");
        assert_eq!(cp, 4);
    }

    #[test]
    fn test_cms006_resume_from_checkpoint() {
        let mut conn = setup_test_db();

        // Simulate a partially-completed import: insert first 2 puzzles
        // using the non-chunked importer, then set checkpoint=2 manually.
        // This respects source_key semantics: same key = same full source.
        let partial_csv = "\
PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate
00008,N4k3/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1,e1e8,1500,70,95,10000,fork,https://lichess.org/training/abc123,Italian_Game,
00009,r1bqkbnr/pppppppp/2n5/4P3/8/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2,d2d4,1600,50,88,5000,opening,https://lichess.org/training/def456,,
";

        // Step 1: insert 2 puzzles via non-chunked importer
        super::import_puzzles_from_reader(&mut conn, partial_csv.as_bytes())
            .expect("seed partial state");

        // Step 2: set checkpoint for fixture-v1 = 2
        super::upsert_checkpoint(&mut conn, "fixture-v1", 2)
            .expect("set checkpoint");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 2, "should have 2 puzzles after seeding");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq("fixture-v1"))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint");
        assert_eq!(cp, 2, "checkpoint should be 2 before resume");

        // Step 3: resume — same source_key, full CSV, chunk_size=2
        let inserted = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "fixture-v1",
            2,
        )
        .expect("resume import");
        assert_eq!(inserted, 2, "resume should insert remaining 2 puzzles");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 4, "should have all 4 puzzles");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq("fixture-v1"))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint");
        assert_eq!(cp, 4);
    }

    #[test]
    fn test_cms006_failed_chunk_rollback_preserves_previous() {
        let mut conn = setup_test_db();

        // CSV with 4 valid puzzles then 1 duplicate to trigger rollback in chunk 2
        let csv_with_dup = "\
PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags,DailyDate
00008,N4k3/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1,e1e8,1500,70,95,10000,fork,https://lichess.org/training/abc123,Italian_Game,
00009,r1bqkbnr/pppppppp/2n5/4P3/8/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2,d2d4,1600,50,88,5000,opening,https://lichess.org/training/def456,,
00010,r1bqkb1r/pppppppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 2 3,f3g5 e7e6 g5f7,1700,80,92,7500,fork sacrifice middlegame,https://lichess.org/training/ghi789,Italian_Game,
00008,N4k3/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1,e1e8,1500,70,95,10000,fork,https://lichess.org/training/dup,,
";

        let result = super::import_puzzles_from_reader_chunked(
            &mut conn,
            csv_with_dup.as_bytes(),
            "dup-test",
            2,
        );
        assert!(result.is_err(), "duplicate in chunk 2 should cause error");

        // Chunk 1 (puzzles 00008, 00009) should be committed
        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 2, "only chunk 1 should be present");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq("dup-test"))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint");
        assert_eq!(cp, 2, "checkpoint should not advance past chunk 1");
    }

    #[test]
    fn test_cms006_zero_chunk_size_returns_error() {
        let mut conn = setup_test_db();
        let result = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "fixture-v1",
            0,
        );
        assert!(result.is_err(), "chunk_size=0 should return Err");
    }

    #[test]
    fn test_cms006_empty_source_key_returns_error() {
        let mut conn = setup_test_db();
        let result = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "",
            2,
        );
        assert!(result.is_err(), "empty source_key should return Err");
    }

    #[test]
    fn test_cms006_checkpoint_greater_than_source_returns_error() {
        let mut conn = setup_test_db();

        // Manually insert a checkpoint claiming 10 rows
        diesel::insert_into(crate::schema::puzzle_import_progress::table)
            .values((
                crate::schema::puzzle_import_progress::dsl::source_key.eq("big-cp"),
                crate::schema::puzzle_import_progress::dsl::completed_rows.eq(10i64),
            ))
            .execute(&mut conn)
            .expect("insert checkpoint");

        let result = super::import_puzzles_from_reader_chunked(
            &mut conn,
            FIXTURE.as_bytes(),
            "big-cp",
            2,
        );
        assert!(result.is_err(), "checkpoint > source rows should return Err");

        // Verify no puzzles were inserted
        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 0, "no puzzles should be inserted when checkpoint is incompatible");
    }

    #[test]
    fn test_cms006_header_mapping_uses_names_not_positions() {
        let mut conn = setup_test_db();

        // CSV with columns reordered and DailyDate included.
        // This should FAIL with positional deserialize(None) but PASS
        // with header-based reader.deserialize::<Puzzle>().
        let reordered_csv = "\
Rating,GameUrl,PuzzleId,OpeningTags,FEN,Moves,RatingDeviation,Popularity,NbPlays,Themes,DailyDate
2100,https://lichess.org/training/abc999,00099,Sicilian_Defense,rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2,e2e4,100,85,3000,opening,2026-01-15
";

        let inserted = super::import_puzzles_from_reader_chunked(
            &mut conn,
            reordered_csv.as_bytes(),
            "header-test",
            10,
        )
        .expect("chunked import with reordered headers should succeed");
        assert_eq!(inserted, 1);

        let p: Puzzle = crate::schema::puzzles::table
            .filter(crate::schema::puzzles::dsl::puzzle_id.eq("00099"))
            .first::<Puzzle>(&mut conn)
            .expect("puzzle 00099 should exist");

        assert_eq!(p.puzzle_id, "00099");
        assert_eq!(p.rating, 2100);
        assert_eq!(p.fen, "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
        assert_eq!(p.opening, "Sicilian_Defense");
    }
}
