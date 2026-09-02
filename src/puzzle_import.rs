use crate::models::{NewPuzzle, Puzzle};
use crate::schema::puzzle_import_progress;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, embed_migrations};
use std::io::{Read, Seek, SeekFrom};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

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

pub fn upsert_checkpoint(
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

// ── CMS-007 — File wrapper & stable source identity ────────────────

const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
const FNV_PRIME: u64 = 1099511628211;

/// FNV-1a 64-bit hash — streaming, one byte at a time.
/// XOR first, then multiply (the "a" variant).
fn fnv1a_update(hash: u64, byte: u8) -> u64 {
    (hash ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// Compute FNV-1a 64-bit over an arbitrary `Read` source.
fn fnv1a_hash_reader<R: Read>(reader: &mut R) -> Result<u64, Box<dyn std::error::Error>> {
    let mut hash = FNV_OFFSET_BASIS;
    let mut buf = [0u8; 65536]; // 64 KiB buffer
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            hash = fnv1a_update(hash, byte);
        }
    }
    Ok(hash)
}

/// Result of a file-based puzzle import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleFileImportResult {
    pub source_key: String,
    pub inserted_rows: usize,
}

/// Compute a stable `source_key` from an already-open `File`.
///
/// Reads the entire file to compute FNV-1a 64-bit, then rewinds
/// the cursor to byte 0 so the caller can re-read for import.
fn puzzle_source_key_from_open_file(
    file: &mut std::fs::File,
) -> Result<String, Box<dyn std::error::Error>> {
    let file_size = file.metadata()?.len();

    file.seek(SeekFrom::Start(0))?;
    let hash = {
        let mut reader = std::io::BufReader::new(&mut *file);
        fnv1a_hash_reader(&mut reader)?
    };

    file.seek(SeekFrom::Start(0))?;

    Ok(format!("cms-source-v1:{}:{:016x}", file_size, hash))
}

/// Compute a stable `source_key` from a file path.
///
/// Format: `cms-source-v1:<file_size>:<fnv64_hex>`
///
/// The hash is FNV-1a 64-bit over the **entire** file content.
/// Two files with identical content always produce the same key,
/// regardless of path, name, or modification time.
///
/// **Cost**: reads the full file once. Intentional for CMS-007;
/// caching can be added later if needed.
pub fn puzzle_source_key_from_file<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(path)?;
    puzzle_source_key_from_open_file(&mut file)
}

/// Import puzzles from a physical CSV file.
///
/// Opens the file once: the same handle is used for fingerprinting
/// and for the chunked import — no TOCTOU window.
pub fn import_puzzles_from_file_chunked<P: AsRef<std::path::Path>>(
    conn: &mut SqliteConnection,
    path: P,
    chunk_size: usize,
) -> Result<PuzzleFileImportResult, Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(&path)?;

    // fingerprint the same handle we'll import from
    let source_key = puzzle_source_key_from_open_file(&mut file)?;

    let inserted = import_puzzles_from_reader_chunked(conn, file, &source_key, chunk_size)?;

    Ok(PuzzleFileImportResult {
        source_key,
        inserted_rows: inserted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewPuzzle;
    use diesel_migrations::MigrationHarness;

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

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

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

    // ── CMS-007 tests ──────────────────────────────────────────────

    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}_{}_{}", name, std::process::id(), id))
    }

    fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = tmp_path(name);
        // Write and explicitly drop the file to release any handles before reading
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&p).expect("create tmp file");
            f.write_all(content).expect("write tmp file");
            f.sync_all().expect("sync tmp file");
        }
        p
    }

    // FNV-1a 64 test vector: empty string = offset basis
    #[test]
    fn test_cms007_fnv_empty_vector() {
        let hash = fnv1a_hash_reader(&mut std::io::Cursor::new(b"")).unwrap();
        assert_eq!(hash, 0xcbf29ce484222325, "FNV-1a 64-bit of empty string");
    }

    // FNV-1a test vector: "a" = known value
    #[test]
    fn test_cms007_fnv_single_byte() {
        let hash = fnv1a_hash_reader(&mut std::io::Cursor::new(b"a")).unwrap();
        // FNV-1a 64("a") = af63dc4c8601ec8c
        assert_eq!(hash, 0xaf63dc4c8601ec8c, "FNV-1a 64-bit of 'a'");
    }

    #[test]
    fn test_cms007_deterministic_source_key() {
        let p = write_tmp("det", b"abc\n123\n");
        let k1 = super::puzzle_source_key_from_file(&p).unwrap();
        let k2 = super::puzzle_source_key_from_file(&p).unwrap();
        assert_eq!(k1, k2, "same file must produce same key");
        assert!(k1.starts_with("cms-source-v1:"), "format prefix");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_cms007_same_content_different_path() {
        let a = write_tmp("path_a", b"abc\n123\n");
        let b = write_tmp("path_b", b"abc\n123\n");
        let ka = super::puzzle_source_key_from_file(&a).unwrap();
        let kb = super::puzzle_source_key_from_file(&b).unwrap();
        assert_eq!(ka, kb, "same content must produce same key regardless of path");
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn test_cms007_different_content_different_key() {
        let a = write_tmp("diff_a", b"abc\n");
        let b = write_tmp("diff_b", b"abd\n");
        let ka = super::puzzle_source_key_from_file(&a).unwrap();
        let kb = super::puzzle_source_key_from_file(&b).unwrap();
        assert_ne!(ka, kb, "different content must produce different key");
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn test_cms007_size_in_source_key() {
        let content = b"hello world";
        let p = write_tmp("size", content);
        let key = super::puzzle_source_key_from_file(&p).unwrap();
        let expected_size = content.len();
        let prefix = format!("cms-source-v1:{}:", expected_size);
        assert!(key.starts_with(&prefix), "key should contain file size, got: {}", key);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_cms007_missing_file_returns_error() {
        let p = tmp_path("nonexistent_xyz");
        let result = super::puzzle_source_key_from_file(&p);
        assert!(result.is_err(), "missing file should return Err");
    }

    #[test]
    fn test_cms007_modified_content_changes_source_key() {
        let p = write_tmp("mod_a", b"abc\n");
        let ka = super::puzzle_source_key_from_file(&p).unwrap();
        std::fs::write(&p, b"abd\n").unwrap();
        let kb = super::puzzle_source_key_from_file(&p).unwrap();
        assert_ne!(ka, kb, "modified content must change key");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_cms007_wrapper_imports_fixture() {
        let p = write_tmp("fixture", FIXTURE.as_bytes());
        let mut conn = setup_test_db();
        let result = super::import_puzzles_from_file_chunked(&mut conn, &p, 2)
            .expect("wrapper import should succeed");
        assert_eq!(result.inserted_rows, 4, "fixture has 4 puzzles");
        assert!(!result.source_key.is_empty(), "source_key must not be empty");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 4);

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq(&result.source_key))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint");
        assert_eq!(cp, 4, "checkpoint should equal total rows");

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn test_cms007_second_run_inserts_zero() {
        let p = write_tmp("fixture2", FIXTURE.as_bytes());
        let mut conn = setup_test_db();

        let r1 = super::import_puzzles_from_file_chunked(&mut conn, &p, 2)
            .expect("first import");
        assert_eq!(r1.inserted_rows, 4);

        let r2 = super::import_puzzles_from_file_chunked(&mut conn, &p, 2)
            .expect("second import");
        assert_eq!(r2.inserted_rows, 0, "second run should insert 0");
        assert_eq!(r1.source_key, r2.source_key, "source_key must be stable");

        let rows: i64 = crate::schema::puzzles::table
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, 4, "still 4 rows");

        let cp: i64 = crate::schema::puzzle_import_progress::table
            .filter(crate::schema::puzzle_import_progress::dsl::source_key.eq(&r1.source_key))
            .select(crate::schema::puzzle_import_progress::dsl::completed_rows)
            .first::<i64>(&mut conn)
            .expect("checkpoint");
        assert_eq!(cp, 4);

        std::fs::remove_file(&p).ok();
    }
}
