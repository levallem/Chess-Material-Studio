use crate::models::Puzzle;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};
use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::collections::HashSet;
use std::path::Path;

pub const PROJECT_APPLICATION_ID: &str = "chess-material-studio-project";
pub const PROJECT_SCHEMA_VERSION: i32 = 3;
pub const PROJECT_MIGRATIONS: EmbeddedMigrations = embed_migrations!("project_migrations");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMetadata {
    pub project_name: String,
    pub schema_version: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChapter {
    pub id: i32,
    pub name: String,
    pub position: i32,
    pub target_puzzle_count: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectPuzzleDecision {
    Selected,
    Discarded,
}

impl ProjectPuzzleDecision {
    fn as_sql_value(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Discarded => "discarded",
        }
    }

    fn from_sql_value(value: &str) -> Result<Self, String> {
        match value {
            "selected" => Ok(Self::Selected),
            "discarded" => Ok(Self::Discarded),
            _ => Err("project puzzle review has an invalid decision".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectPuzzleReview {
    pub chapter_id: i32,
    pub puzzle: Puzzle,
    pub decision: ProjectPuzzleDecision,
    pub reviewed_at: String,
}

#[derive(Debug, Clone)]
pub struct ProjectChapterSelectedPuzzles {
    pub chapter_id: i32,
    pub chapter_name: String,
    pub chapter_position: i32,
    pub puzzles: Vec<Puzzle>,
}

#[derive(QueryableByName)]
struct ProjectMetadataRow {
    #[diesel(sql_type = Text)]
    application_id: String,
    #[diesel(sql_type = Integer)]
    schema_version: i32,
    #[diesel(sql_type = Text)]
    project_name: String,
    #[diesel(sql_type = Text)]
    created_at: String,
}

#[derive(QueryableByName)]
struct ProjectChapterRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Integer)]
    position: i32,
    #[diesel(sql_type = Nullable<Integer>)]
    target_puzzle_count: Option<i32>,
    #[diesel(sql_type = Text)]
    created_at: String,
}

#[derive(QueryableByName)]
struct ProjectPuzzleReviewRow {
    #[diesel(sql_type = Integer)]
    chapter_id: i32,
    #[diesel(sql_type = Text)]
    puzzle_id: String,
    #[diesel(sql_type = Text)]
    decision: String,
    #[diesel(sql_type = Text)]
    fen: String,
    #[diesel(sql_type = Text)]
    moves: String,
    #[diesel(sql_type = Integer)]
    rating: i32,
    #[diesel(sql_type = Integer)]
    rating_deviation: i32,
    #[diesel(sql_type = Integer)]
    popularity: i32,
    #[diesel(sql_type = Integer)]
    nb_plays: i32,
    #[diesel(sql_type = Text)]
    themes: String,
    #[diesel(sql_type = Text)]
    game_url: String,
    #[diesel(sql_type = Text)]
    opening_tags: String,
    #[diesel(sql_type = Text)]
    reviewed_at: String,
}

#[derive(QueryableByName)]
struct ProjectChapterSelectedPuzzleRow {
    #[diesel(sql_type = Integer)]
    chapter_id: i32,
    #[diesel(sql_type = Text)]
    chapter_name: String,
    #[diesel(sql_type = Integer)]
    chapter_position: i32,
    #[diesel(sql_type = Text)]
    puzzle_id: String,
    #[diesel(sql_type = Text)]
    fen: String,
    #[diesel(sql_type = Text)]
    moves: String,
    #[diesel(sql_type = Integer)]
    rating: i32,
    #[diesel(sql_type = Integer)]
    rating_deviation: i32,
    #[diesel(sql_type = Integer)]
    popularity: i32,
    #[diesel(sql_type = Integer)]
    nb_plays: i32,
    #[diesel(sql_type = Text)]
    themes: String,
    #[diesel(sql_type = Text)]
    game_url: String,
    #[diesel(sql_type = Text)]
    opening_tags: String,
}

#[derive(QueryableByName)]
struct ProjectPuzzleDecisionRow {
    #[diesel(sql_type = Text)]
    decision: String,
}

#[derive(QueryableByName)]
struct PuzzleIdRow {
    #[diesel(sql_type = Text)]
    puzzle_id: String,
}

#[derive(QueryableByName)]
struct ChapterExistsRow {
    #[diesel(sql_type = Integer)]
    chapter_exists: i32,
}

impl From<ProjectChapterRow> for ProjectChapter {
    fn from(row: ProjectChapterRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            position: row.position,
            target_puzzle_count: row.target_puzzle_count,
            created_at: row.created_at,
        }
    }
}

impl TryFrom<ProjectPuzzleReviewRow> for ProjectPuzzleReview {
    type Error = String;

    fn try_from(row: ProjectPuzzleReviewRow) -> Result<Self, Self::Error> {
        Ok(Self {
            chapter_id: row.chapter_id,
            puzzle: Puzzle {
                puzzle_id: row.puzzle_id,
                fen: row.fen,
                moves: row.moves,
                rating: row.rating,
                rating_deviation: row.rating_deviation,
                popularity: row.popularity,
                nb_plays: row.nb_plays,
                themes: row.themes,
                game_url: row.game_url,
                opening: row.opening_tags,
            },
            decision: ProjectPuzzleDecision::from_sql_value(&row.decision)?,
            reviewed_at: row.reviewed_at,
        })
    }
}

impl ProjectChapterSelectedPuzzleRow {
    fn into_puzzle(self) -> Puzzle {
        Puzzle {
            puzzle_id: self.puzzle_id,
            fen: self.fen,
            moves: self.moves,
            rating: self.rating,
            rating_deviation: self.rating_deviation,
            popularity: self.popularity,
            nb_plays: self.nb_plays,
            themes: self.themes,
            game_url: self.game_url,
            opening: self.opening_tags,
        }
    }
}

pub fn create_project(path: &Path, project_name: &str) -> Result<ProjectMetadata, String> {
    validate_name(project_name, "project name")?;

    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err("project file already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect project path: {error}")),
    }

    let path_string = project_path_string(path)?;
    let result = (|| {
        let mut connection = SqliteConnection::establish(&path_string)
            .map_err(|error| format!("cannot create SQLite project: {error}"))?;
        connection
            .run_pending_migrations(PROJECT_MIGRATIONS)
            .map_err(|error| format!("cannot run project migrations: {error}"))?;

        let metadata = ProjectMetadata {
            project_name: project_name.to_owned(),
            schema_version: PROJECT_SCHEMA_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        diesel::sql_query(
            "INSERT INTO project_metadata \
             (id, application_id, schema_version, project_name, created_at) \
             VALUES (1, ?, ?, ?, ?)",
        )
        .bind::<Text, _>(PROJECT_APPLICATION_ID)
        .bind::<Integer, _>(metadata.schema_version)
        .bind::<Text, _>(&metadata.project_name)
        .bind::<Text, _>(&metadata.created_at)
        .execute(&mut connection)
        .map_err(|error| format!("cannot write project metadata: {error}"))?;

        Ok(metadata)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }

    result
}

pub fn open_project(path: &Path) -> Result<ProjectMetadata, String> {
    let mut connection = open_validated_project_connection(path)?;
    let row = read_project_metadata(&mut connection)?;
    Ok(project_metadata_from_row(&row))
}

pub fn create_chapter(
    path: &Path,
    name: &str,
    target_puzzle_count: Option<i32>,
) -> Result<ProjectChapter, String> {
    validate_name(name, "chapter name")?;
    validate_target_puzzle_count(target_puzzle_count)?;

    let mut connection = open_validated_project_connection(path)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    diesel::sql_query(
        "INSERT INTO chapters (name, position, target_puzzle_count, created_at) \
         VALUES (?, (SELECT COALESCE(MAX(position), 0) + 1 FROM chapters), ?, ?)",
    )
    .bind::<Text, _>(name)
    .bind::<Nullable<Integer>, _>(target_puzzle_count)
    .bind::<Text, _>(&created_at)
    .execute(&mut connection)
    .map_err(|error| format!("cannot create chapter: {error}"))?;

    read_last_inserted_chapter(&mut connection)
}

pub fn list_chapters(path: &Path) -> Result<Vec<ProjectChapter>, String> {
    let mut connection = open_validated_project_connection(path)?;
    list_chapters_from_connection(&mut connection)
}

pub fn rename_chapter(path: &Path, chapter_id: i32, name: &str) -> Result<(), String> {
    validate_name(name, "chapter name")?;

    let mut connection = open_validated_project_connection(path)?;
    let updated = diesel::sql_query("UPDATE chapters SET name = ? WHERE id = ?")
        .bind::<Text, _>(name)
        .bind::<Integer, _>(chapter_id)
        .execute(&mut connection)
        .map_err(|error| format!("cannot rename chapter: {error}"))?;
    if updated == 0 {
        return Err("chapter not found".into());
    }
    Ok(())
}

pub fn set_chapter_target(
    path: &Path,
    chapter_id: i32,
    target_puzzle_count: Option<i32>,
) -> Result<(), String> {
    validate_target_puzzle_count(target_puzzle_count)?;

    let mut connection = open_validated_project_connection(path)?;
    let updated = diesel::sql_query("UPDATE chapters SET target_puzzle_count = ? WHERE id = ?")
        .bind::<Nullable<Integer>, _>(target_puzzle_count)
        .bind::<Integer, _>(chapter_id)
        .execute(&mut connection)
        .map_err(|error| format!("cannot update chapter target: {error}"))?;
    if updated == 0 {
        return Err("chapter not found".into());
    }
    Ok(())
}

pub fn reorder_chapters(path: &Path, ordered_ids: &[i32]) -> Result<(), String> {
    let mut connection = open_validated_project_connection(path)?;
    let existing_chapters = list_chapters_from_connection(&mut connection)?;
    validate_chapter_order(&existing_chapters, ordered_ids)?;
    let chapter_count = i32::try_from(existing_chapters.len())
        .map_err(|_| "too many chapters to reorder".to_string())?;
    let maximum_position = existing_chapters
        .iter()
        .map(|chapter| chapter.position)
        .max()
        .unwrap_or(0);
    let temporary_start = maximum_position
        .checked_add(1)
        .ok_or_else(|| "chapter positions cannot be staged safely".to_string())?;
    maximum_position
        .checked_add(chapter_count)
        .ok_or_else(|| "chapter positions cannot be staged safely".to_string())?;

    connection
        .transaction::<(), diesel::result::Error, _>(|connection| {
            for (index, chapter) in existing_chapters.iter().enumerate() {
                diesel::sql_query("UPDATE chapters SET position = ? WHERE id = ?")
                    .bind::<Integer, _>(temporary_start + i32::try_from(index).unwrap())
                    .bind::<Integer, _>(chapter.id)
                    .execute(connection)?;
            }
            for (index, chapter_id) in ordered_ids.iter().enumerate() {
                diesel::sql_query("UPDATE chapters SET position = ? WHERE id = ?")
                    .bind::<Integer, _>(i32::try_from(index + 1).unwrap())
                    .bind::<Integer, _>(chapter_id)
                    .execute(connection)?;
            }
            Ok(())
        })
        .map_err(|error| format!("cannot reorder chapters: {error}"))
}

pub fn set_puzzle_decision(
    path: &Path,
    chapter_id: i32,
    puzzle: &Puzzle,
    decision: ProjectPuzzleDecision,
) -> Result<(), String> {
    let mut connection = open_validated_project_connection(path)?;
    ensure_chapter_exists(&mut connection, chapter_id)?;
    let reviewed_at = chrono::Utc::now().to_rfc3339();

    connection
        .transaction::<(), diesel::result::Error, _>(|connection| {
            diesel::sql_query(
                "INSERT INTO chapter_puzzle_reviews \
                 (chapter_id, puzzle_id, decision, fen, moves, rating, rating_deviation, \
                  popularity, nb_plays, themes, game_url, opening_tags, reviewed_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(chapter_id, puzzle_id) DO UPDATE SET \
                 decision = excluded.decision, fen = excluded.fen, moves = excluded.moves, \
                 rating = excluded.rating, rating_deviation = excluded.rating_deviation, \
                 popularity = excluded.popularity, nb_plays = excluded.nb_plays, \
                 themes = excluded.themes, game_url = excluded.game_url, \
                 opening_tags = excluded.opening_tags, reviewed_at = excluded.reviewed_at",
            )
            .bind::<Integer, _>(chapter_id)
            .bind::<Text, _>(&puzzle.puzzle_id)
            .bind::<Text, _>(decision.as_sql_value())
            .bind::<Text, _>(&puzzle.fen)
            .bind::<Text, _>(&puzzle.moves)
            .bind::<Integer, _>(puzzle.rating)
            .bind::<Integer, _>(puzzle.rating_deviation)
            .bind::<Integer, _>(puzzle.popularity)
            .bind::<Integer, _>(puzzle.nb_plays)
            .bind::<Text, _>(&puzzle.themes)
            .bind::<Text, _>(&puzzle.game_url)
            .bind::<Text, _>(&puzzle.opening)
            .bind::<Text, _>(&reviewed_at)
            .execute(connection)?;
            Ok(())
        })
        .map_err(|error| match error {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) if decision == ProjectPuzzleDecision::Selected => {
                "puzzle is already selected in another chapter".into()
            }
            error => format!("cannot set puzzle decision: {error}"),
        })
}

pub fn get_puzzle_decision(
    path: &Path,
    chapter_id: i32,
    puzzle_id: &str,
) -> Result<Option<ProjectPuzzleDecision>, String> {
    let mut connection = open_validated_project_connection(path)?;
    ensure_chapter_exists(&mut connection, chapter_id)?;
    diesel::sql_query(
        "SELECT decision FROM chapter_puzzle_reviews WHERE chapter_id = ? AND puzzle_id = ?",
    )
    .bind::<Integer, _>(chapter_id)
    .bind::<Text, _>(puzzle_id)
    .get_result::<ProjectPuzzleDecisionRow>(&mut connection)
    .optional()
    .map_err(|error| format!("cannot get puzzle decision: {error}"))?
    .map(|row| ProjectPuzzleDecision::from_sql_value(&row.decision))
    .transpose()
}

pub fn clear_puzzle_decision(path: &Path, chapter_id: i32, puzzle_id: &str) -> Result<(), String> {
    let mut connection = open_validated_project_connection(path)?;
    ensure_chapter_exists(&mut connection, chapter_id)?;
    diesel::sql_query("DELETE FROM chapter_puzzle_reviews WHERE chapter_id = ? AND puzzle_id = ?")
        .bind::<Integer, _>(chapter_id)
        .bind::<Text, _>(puzzle_id)
        .execute(&mut connection)
        .map_err(|error| format!("cannot clear puzzle decision: {error}"))?;
    Ok(())
}

pub fn list_chapter_puzzle_reviews(
    path: &Path,
    chapter_id: i32,
) -> Result<Vec<ProjectPuzzleReview>, String> {
    list_puzzle_reviews(path, chapter_id, None)
}

pub fn list_reviewed_puzzle_ids_for_chapter(
    path: &Path,
    chapter_id: i32,
) -> Result<HashSet<String>, String> {
    let mut connection = open_validated_project_connection(path)?;
    ensure_chapter_exists(&mut connection, chapter_id)?;
    diesel::sql_query("SELECT puzzle_id FROM chapter_puzzle_reviews WHERE chapter_id = ?")
        .bind::<Integer, _>(chapter_id)
        .load::<PuzzleIdRow>(&mut connection)
        .map(|rows| rows.into_iter().map(|row| row.puzzle_id).collect())
        .map_err(|error| format!("cannot list reviewed puzzle IDs: {error}"))
}

pub fn list_selected_puzzles_for_chapter(
    path: &Path,
    chapter_id: i32,
) -> Result<Vec<Puzzle>, String> {
    list_puzzle_reviews(path, chapter_id, Some(ProjectPuzzleDecision::Selected))
        .map(|reviews| reviews.into_iter().map(|review| review.puzzle).collect())
}

pub fn list_selected_puzzles_by_chapter(
    path: &Path,
) -> Result<Vec<ProjectChapterSelectedPuzzles>, String> {
    let mut connection = open_read_only_project_connection(path)?;
    let rows = diesel::sql_query(
        "SELECT chapters.id AS chapter_id, chapters.name AS chapter_name, \
         chapters.position AS chapter_position, chapter_puzzle_reviews.puzzle_id, \
         chapter_puzzle_reviews.fen, chapter_puzzle_reviews.moves, \
         chapter_puzzle_reviews.rating, chapter_puzzle_reviews.rating_deviation, \
         chapter_puzzle_reviews.popularity, chapter_puzzle_reviews.nb_plays, \
         chapter_puzzle_reviews.themes, chapter_puzzle_reviews.game_url, \
         chapter_puzzle_reviews.opening_tags \
         FROM chapters INNER JOIN chapter_puzzle_reviews \
         ON chapter_puzzle_reviews.chapter_id = chapters.id \
         WHERE chapter_puzzle_reviews.decision = 'selected' \
         ORDER BY chapters.position ASC, chapter_puzzle_reviews.reviewed_at ASC, \
         chapter_puzzle_reviews.puzzle_id ASC",
    )
    .load::<ProjectChapterSelectedPuzzleRow>(&mut connection)
    .map_err(|error| format!("cannot list selected project puzzles: {error}"))?;

    let mut chapters: Vec<ProjectChapterSelectedPuzzles> = Vec::new();
    for row in rows {
        let is_current_chapter = chapters
            .last()
            .is_some_and(|chapter| chapter.chapter_id == row.chapter_id);
        if !is_current_chapter {
            chapters.push(ProjectChapterSelectedPuzzles {
                chapter_id: row.chapter_id,
                chapter_name: row.chapter_name.clone(),
                chapter_position: row.chapter_position,
                puzzles: Vec::new(),
            });
        }
        chapters
            .last_mut()
            .expect("selected puzzle row always creates a chapter")
            .puzzles
            .push(row.into_puzzle());
    }
    Ok(chapters)
}

pub fn find_selected_puzzle_chapter(
    path: &Path,
    puzzle_id: &str,
) -> Result<Option<ProjectChapter>, String> {
    let mut connection = open_validated_project_connection(path)?;
    diesel::sql_query(
        "SELECT chapters.id, chapters.name, chapters.position, chapters.target_puzzle_count, \
         chapters.created_at FROM chapters \
         INNER JOIN chapter_puzzle_reviews ON chapter_puzzle_reviews.chapter_id = chapters.id \
         WHERE chapter_puzzle_reviews.puzzle_id = ? \
         AND chapter_puzzle_reviews.decision = 'selected'",
    )
    .bind::<Text, _>(puzzle_id)
    .get_result::<ProjectChapterRow>(&mut connection)
    .optional()
    .map(|chapter| chapter.map(ProjectChapter::from))
    .map_err(|error| format!("cannot find selected puzzle chapter: {error}"))
}

fn open_validated_project_connection(path: &Path) -> Result<SqliteConnection, String> {
    let file_metadata =
        std::fs::metadata(path).map_err(|error| format!("project file cannot be read: {error}"))?;
    if !file_metadata.is_file() {
        return Err("project path is not a regular file".into());
    }

    let path_string = project_path_string(path)?;
    let mut connection = SqliteConnection::establish(&path_string)
        .map_err(|error| format!("cannot open SQLite project: {error}"))?;
    let row = read_project_metadata(&mut connection)?;
    if row.application_id != PROJECT_APPLICATION_ID {
        return Err("SQLite file is not a Chess Material Studio project".into());
    }
    if row.project_name.trim().is_empty() || row.created_at.trim().is_empty() {
        return Err("project metadata is missing required values".into());
    }
    if row.schema_version < 1 || row.schema_version > PROJECT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported project schema version: {}",
            row.schema_version
        ));
    }
    if row.schema_version < PROJECT_SCHEMA_VERSION {
        connection
            .transaction::<(), Box<dyn std::error::Error + Send + Sync>, _>(|connection| {
                connection.run_pending_migrations(PROJECT_MIGRATIONS)?;
                diesel::sql_query("UPDATE project_metadata SET schema_version = ? WHERE id = 1")
                    .bind::<Integer, _>(PROJECT_SCHEMA_VERSION)
                    .execute(connection)?;
                Ok(())
            })
            .map_err(|error| format!("cannot upgrade project: {error}"))?;
    }

    Ok(connection)
}

fn open_read_only_project_connection(path: &Path) -> Result<SqliteConnection, String> {
    let file_metadata =
        std::fs::metadata(path).map_err(|error| format!("project file cannot be read: {error}"))?;
    if !file_metadata.is_file() {
        return Err("project path is not a regular file".into());
    }

    let path_string = project_path_string(path)?;
    let mut connection = SqliteConnection::establish(&path_string)
        .map_err(|error| format!("cannot open SQLite project: {error}"))?;
    let row = read_project_metadata(&mut connection)?;
    if row.application_id != PROJECT_APPLICATION_ID {
        return Err("SQLite file is not a Chess Material Studio project".into());
    }
    if row.project_name.trim().is_empty() || row.created_at.trim().is_empty() {
        return Err("project metadata is missing required values".into());
    }
    if row.schema_version != PROJECT_SCHEMA_VERSION {
        return Err(format!(
            "project schema version {} is not readable without an upgrade",
            row.schema_version
        ));
    }
    Ok(connection)
}

fn list_puzzle_reviews(
    path: &Path,
    chapter_id: i32,
    decision: Option<ProjectPuzzleDecision>,
) -> Result<Vec<ProjectPuzzleReview>, String> {
    let mut connection = open_validated_project_connection(path)?;
    ensure_chapter_exists(&mut connection, chapter_id)?;
    let rows = match decision {
        Some(decision) => diesel::sql_query(
            "SELECT chapter_id, puzzle_id, decision, fen, moves, rating, rating_deviation, \
             popularity, nb_plays, themes, game_url, opening_tags, reviewed_at \
             FROM chapter_puzzle_reviews WHERE chapter_id = ? AND decision = ? \
             ORDER BY reviewed_at ASC, puzzle_id ASC",
        )
        .bind::<Integer, _>(chapter_id)
        .bind::<Text, _>(decision.as_sql_value())
        .load::<ProjectPuzzleReviewRow>(&mut connection),
        None => diesel::sql_query(
            "SELECT chapter_id, puzzle_id, decision, fen, moves, rating, rating_deviation, \
             popularity, nb_plays, themes, game_url, opening_tags, reviewed_at \
             FROM chapter_puzzle_reviews WHERE chapter_id = ? \
             ORDER BY reviewed_at ASC, puzzle_id ASC",
        )
        .bind::<Integer, _>(chapter_id)
        .load::<ProjectPuzzleReviewRow>(&mut connection),
    }
    .map_err(|error| format!("cannot list puzzle reviews: {error}"))?;

    rows.into_iter()
        .map(ProjectPuzzleReview::try_from)
        .collect()
}

fn ensure_chapter_exists(connection: &mut SqliteConnection, chapter_id: i32) -> Result<(), String> {
    let row =
        diesel::sql_query("SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ?) AS chapter_exists")
            .bind::<Integer, _>(chapter_id)
            .get_result::<ChapterExistsRow>(connection)
            .map_err(|error| format!("cannot validate chapter: {error}"))?;
    if row.chapter_exists == 0 {
        return Err("chapter not found".into());
    }
    Ok(())
}

fn read_project_metadata(connection: &mut SqliteConnection) -> Result<ProjectMetadataRow, String> {
    let rows = diesel::sql_query(
        "SELECT application_id, schema_version, project_name, created_at \
         FROM project_metadata WHERE id = 1",
    )
    .load::<ProjectMetadataRow>(connection)
    .map_err(|error| format!("cannot read project metadata: {error}"))?;
    let [row] = rows.as_slice() else {
        return Err("project metadata is missing or invalid".into());
    };
    Ok(ProjectMetadataRow {
        application_id: row.application_id.clone(),
        schema_version: row.schema_version,
        project_name: row.project_name.clone(),
        created_at: row.created_at.clone(),
    })
}

fn project_metadata_from_row(row: &ProjectMetadataRow) -> ProjectMetadata {
    ProjectMetadata {
        project_name: row.project_name.clone(),
        schema_version: row.schema_version,
        created_at: row.created_at.clone(),
    }
}

fn read_last_inserted_chapter(connection: &mut SqliteConnection) -> Result<ProjectChapter, String> {
    diesel::sql_query(
        "SELECT id, name, position, target_puzzle_count, created_at \
         FROM chapters WHERE id = last_insert_rowid()",
    )
    .get_result::<ProjectChapterRow>(connection)
    .map(ProjectChapter::from)
    .map_err(|error| format!("cannot read created chapter: {error}"))
}

fn list_chapters_from_connection(
    connection: &mut SqliteConnection,
) -> Result<Vec<ProjectChapter>, String> {
    diesel::sql_query(
        "SELECT id, name, position, target_puzzle_count, created_at \
         FROM chapters ORDER BY position ASC",
    )
    .load::<ProjectChapterRow>(connection)
    .map(|rows| rows.into_iter().map(ProjectChapter::from).collect())
    .map_err(|error| format!("cannot list chapters: {error}"))
}

fn validate_name(name: &str, field: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    Ok(())
}

fn validate_target_puzzle_count(target_puzzle_count: Option<i32>) -> Result<(), String> {
    if target_puzzle_count.is_some_and(|target| target <= 0) {
        return Err("target puzzle count must be greater than zero".into());
    }
    Ok(())
}

fn validate_chapter_order(
    existing_chapters: &[ProjectChapter],
    ordered_ids: &[i32],
) -> Result<(), String> {
    let existing_ids: HashSet<i32> = existing_chapters.iter().map(|chapter| chapter.id).collect();
    let ordered_id_set: HashSet<i32> = ordered_ids.iter().copied().collect();
    if ordered_id_set.len() != ordered_ids.len() {
        return Err("chapter order contains duplicate IDs".into());
    }
    if ordered_ids
        .iter()
        .any(|chapter_id| !existing_ids.contains(chapter_id))
    {
        return Err("chapter order contains an unknown ID".into());
    }
    if ordered_id_set.len() != existing_ids.len() {
        return Err("chapter order is missing an existing ID".into());
    }
    Ok(())
}

fn project_path_string(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "project path is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::Connection;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempProjectDb {
        path: std::path::PathBuf,
        directory: std::path::PathBuf,
    }

    impl TempProjectDb {
        fn new(label: &str) -> Self {
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_023b_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("test directory should be created");
            Self {
                path: directory.join("project.cms.sqlite"),
                directory,
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn connection(&self) -> SqliteConnection {
            SqliteConnection::establish(self.path().to_str().unwrap()).unwrap()
        }
    }

    impl Drop for TempProjectDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(&self.directory);
        }
    }

    #[derive(QueryableByName)]
    struct TableName {
        #[diesel(sql_type = Text)]
        name: String,
    }

    #[derive(QueryableByName)]
    struct MigrationVersion {
        #[diesel(sql_type = Text)]
        version: String,
    }

    #[derive(QueryableByName)]
    struct ExistingValueRow {
        #[diesel(sql_type = Text)]
        existing_value: String,
    }

    #[test]
    fn creating_a_project_creates_a_valid_sqlite_project() {
        let project = TempProjectDb::new("create");
        let metadata =
            create_project(project.path(), "Libro táctico").expect("project should be created");

        assert!(project.path().is_file());
        assert_eq!(metadata.project_name, "Libro táctico");
        assert_eq!(metadata.schema_version, PROJECT_SCHEMA_VERSION);
        assert_eq!(open_project(project.path()).unwrap(), metadata);
    }

    #[test]
    fn reopening_a_project_returns_the_same_name() {
        let project = TempProjectDb::new("reopen");
        create_project(project.path(), "Patrones de ataque").unwrap();

        assert_eq!(
            open_project(project.path()).unwrap().project_name,
            "Patrones de ataque"
        );
    }

    #[test]
    fn project_schema_version_starts_at_three() {
        let project = TempProjectDb::new("schema-version");

        assert_eq!(
            create_project(project.path(), "Versión inicial")
                .unwrap()
                .schema_version,
            3
        );
    }

    #[test]
    fn opening_rejects_an_invalid_application_identifier() {
        let project = TempProjectDb::new("invalid-identifier");
        create_project(project.path(), "Identificador").unwrap();
        {
            let mut connection =
                SqliteConnection::establish(project.path().to_str().unwrap()).unwrap();
            diesel::sql_query("UPDATE project_metadata SET application_id = 'other-app'")
                .execute(&mut connection)
                .unwrap();
        }

        assert!(open_project(project.path()).is_err());
    }

    #[test]
    fn creating_rejects_empty_or_whitespace_only_project_names() {
        for name in ["", "   ", "\t\n"] {
            let project = TempProjectDb::new("empty-name");
            assert!(create_project(project.path(), name).is_err());
            assert!(!project.path().exists());
        }
    }

    #[test]
    fn creating_rejects_an_existing_file_without_overwriting_it() {
        let project = TempProjectDb::new("existing-file");
        std::fs::write(project.path(), b"existing content").unwrap();

        assert!(create_project(project.path(), "No reemplazar").is_err());
        assert_eq!(std::fs::read(project.path()).unwrap(), b"existing content");
    }

    #[test]
    fn opening_rejects_a_nonexistent_file() {
        let project = TempProjectDb::new("missing-file");

        assert!(open_project(project.path()).is_err());
    }

    #[test]
    fn opening_rejects_an_unrelated_sqlite_database() {
        let project = TempProjectDb::new("unrelated-sqlite");
        {
            let mut connection =
                SqliteConnection::establish(project.path().to_str().unwrap()).unwrap();
            diesel::sql_query("CREATE TABLE unrelated_data (value TEXT NOT NULL)")
                .execute(&mut connection)
                .unwrap();
        }

        assert!(open_project(project.path()).is_err());
        let mut connection = project.connection();
        let tables = table_names(&mut connection);
        assert!(tables.contains("unrelated_data"));
        assert!(!tables.contains("project_metadata"));
        assert!(!tables.contains("chapters"));
        assert!(!tables.contains("__diesel_schema_migrations"));
    }

    #[test]
    fn creating_a_project_does_not_depend_on_ocp_db() {
        let project = TempProjectDb::new("no-ocp-db");
        let ocp_db = project.directory.join("ocp.db");
        assert!(!ocp_db.exists());

        create_project(project.path(), "Independiente").unwrap();

        assert!(!ocp_db.exists());
        assert!(open_project(project.path()).is_ok());
    }

    #[test]
    fn project_migrations_are_isolated_from_normal_migrations() {
        let project = TempProjectDb::new("migration-isolation");
        create_project(project.path(), "Migraciones aisladas").unwrap();
        let mut connection = project.connection();
        let tables = table_names(&mut connection);
        assert!(tables.contains("project_metadata"));
        assert!(tables.contains("chapters"));
        assert!(!tables.contains("favs"));
        assert!(!tables.contains("puzzles"));
        assert!(!tables.contains("puzzle_import_progress"));

        assert_eq!(
            migration_versions(&mut connection),
            [
                "20260908000000".to_string(),
                "20260908010000".to_string(),
                "20260908020000".to_string(),
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn opening_a_valid_v1_project_upgrades_it_to_version_three() {
        let project = TempProjectDb::new("upgrade-v1");
        create_valid_v1_project(&project);

        assert_eq!(open_project(project.path()).unwrap().schema_version, 3);
        let mut connection = project.connection();
        assert!(table_names(&mut connection).contains("chapters"));
        assert_eq!(
            migration_versions(&mut connection),
            [
                "20260908000000".to_string(),
                "20260908010000".to_string(),
                "20260908020000".to_string(),
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn opening_a_future_project_version_rejects_without_modifying_it() {
        let project = create_test_project("future-version");
        let mut connection = project.connection();
        diesel::sql_query("UPDATE project_metadata SET schema_version = 4")
            .execute(&mut connection)
            .unwrap();
        let migration_versions_before = migration_versions(&mut connection);

        assert!(open_project(project.path()).is_err());

        let metadata = read_project_metadata(&mut connection).unwrap();
        assert_eq!(metadata.schema_version, 4);
        assert_eq!(
            migration_versions(&mut connection),
            migration_versions_before
        );
    }

    #[test]
    fn opening_a_v1_shaped_non_cms_database_does_not_migrate_it() {
        let project = TempProjectDb::new("wrong-v1-identity");
        create_v1_project_with_application_id(&project, "other-app");

        assert!(open_project(project.path()).is_err());

        let mut connection = project.connection();
        assert!(!table_names(&mut connection).contains("chapters"));
        let metadata = read_project_metadata(&mut connection).unwrap();
        assert_eq!(metadata.application_id, "other-app");
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(
            migration_versions(&mut connection),
            ["20260908000000".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn creating_chapters_uses_one_based_insertion_order() {
        let project = create_test_project("chapter-insertion");
        let first = create_chapter(project.path(), "Pieza colgante", None).unwrap();
        let second = create_chapter(project.path(), "Ataque doble", Some(20)).unwrap();

        assert_eq!(first.position, 1);
        assert_eq!(second.position, 2);
        assert_eq!(second.target_puzzle_count, Some(20));
        assert_eq!(
            chapter_names(&list_chapters(project.path()).unwrap()),
            ["Pieza colgante", "Ataque doble"]
        );
    }

    #[test]
    fn chapter_names_and_targets_are_validated() {
        let project = create_test_project("chapter-validation");
        for name in ["", "   ", "\t\n"] {
            assert!(create_chapter(project.path(), name, None).is_err());
        }
        let chapter = create_chapter(project.path(), "Válido", None).unwrap();
        for name in ["", "   ", "\t\n"] {
            assert!(rename_chapter(project.path(), chapter.id, name).is_err());
        }
        for target in [Some(0), Some(-1)] {
            assert!(create_chapter(project.path(), "Inválido", target).is_err());
            assert!(set_chapter_target(project.path(), chapter.id, target).is_err());
        }
    }

    #[test]
    fn chapter_target_none_and_updates_persist_after_reopening() {
        let project = create_test_project("chapter-target-persist");
        let chapter = create_chapter(project.path(), "Ataque doble", Some(20)).unwrap();

        set_chapter_target(project.path(), chapter.id, None).unwrap();
        assert_eq!(
            list_chapters(project.path()).unwrap()[0].target_puzzle_count,
            None
        );
        set_chapter_target(project.path(), chapter.id, Some(13)).unwrap();
        rename_chapter(project.path(), chapter.id, "Ataque doble renovado").unwrap();
        open_project(project.path()).unwrap();

        let reopened = &list_chapters(project.path()).unwrap()[0];
        assert_eq!(reopened.name, "Ataque doble renovado");
        assert_eq!(reopened.target_puzzle_count, Some(13));
    }

    #[test]
    fn creating_a_chapter_preserves_created_at_when_listed() {
        let project = create_test_project("chapter-created-at");
        let chapter = create_chapter(project.path(), "Persistencia", None).unwrap();

        let listed = list_chapters(project.path()).unwrap();
        assert_eq!(listed[0].id, chapter.id);
        assert_eq!(listed[0].created_at, chapter.created_at);
    }

    #[test]
    fn rename_and_target_reject_unknown_ids_without_changes() {
        let project = create_test_project("unknown-update-id");
        create_chapter(project.path(), "Original", Some(10)).unwrap();
        let before = list_chapters(project.path()).unwrap();

        assert!(rename_chapter(project.path(), 999, "No existe").is_err());
        assert!(set_chapter_target(project.path(), 999, None).is_err());

        assert_eq!(list_chapters(project.path()).unwrap(), before);
    }

    #[test]
    fn reordering_chapters_persists_contiguous_positions() {
        let project = create_test_project("reorder-persist");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let third = create_chapter(project.path(), "Tercero", None).unwrap();

        reorder_chapters(project.path(), &[third.id, first.id, second.id]).unwrap();
        open_project(project.path()).unwrap();
        let chapters = list_chapters(project.path()).unwrap();

        assert_eq!(chapter_names(&chapters), ["Tercero", "Primero", "Segundo"]);
        assert_eq!(
            chapters
                .iter()
                .map(|chapter| chapter.position)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn reordering_sparse_positions_uses_collision_free_staging() {
        let project = create_test_project("reorder-sparse");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let third = create_chapter(project.path(), "Tercero", None).unwrap();
        let mut connection = project.connection();
        diesel::sql_query("UPDATE chapters SET position = 7 WHERE id = ?")
            .bind::<Integer, _>(third.id)
            .execute(&mut connection)
            .unwrap();
        diesel::sql_query("UPDATE chapters SET position = 3 WHERE id = ?")
            .bind::<Integer, _>(second.id)
            .execute(&mut connection)
            .unwrap();

        reorder_chapters(project.path(), &[third.id, first.id, second.id]).unwrap();

        let chapters = list_chapters(project.path()).unwrap();
        assert_eq!(chapter_names(&chapters), ["Tercero", "Primero", "Segundo"]);
        assert_eq!(
            chapters
                .iter()
                .map(|chapter| chapter.position)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn reorder_rejects_duplicate_unknown_and_missing_ids_without_changes() {
        let project = create_test_project("reorder-validation");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let before = list_chapters(project.path()).unwrap();

        assert!(reorder_chapters(project.path(), &[first.id, first.id]).is_err());
        assert!(reorder_chapters(project.path(), &[first.id, 999]).is_err());
        assert!(reorder_chapters(project.path(), &[first.id]).is_err());
        assert_eq!(list_chapters(project.path()).unwrap(), before);
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn opening_a_valid_v2_project_upgrades_it_to_version_three() {
        let project = TempProjectDb::new("upgrade-v2");
        create_valid_v2_project(&project);

        assert_eq!(open_project(project.path()).unwrap().schema_version, 3);
        let mut connection = project.connection();
        assert!(table_names(&mut connection).contains("chapter_puzzle_reviews"));
        assert_eq!(
            migration_versions(&mut connection),
            [
                "20260908000000".to_string(),
                "20260908010000".to_string(),
                "20260908020000".to_string(),
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn failed_v1_upgrade_rolls_back_every_pending_project_migration() {
        let project = TempProjectDb::new("upgrade-v1-rollback");
        create_valid_v1_project(&project);
        let mut connection = project.connection();
        diesel::sql_query("CREATE TABLE chapter_puzzle_reviews (existing_value TEXT NOT NULL)")
            .execute(&mut connection)
            .unwrap();
        diesel::sql_query("INSERT INTO chapter_puzzle_reviews VALUES ('preserve me')")
            .execute(&mut connection)
            .unwrap();
        let tables_before = table_names(&mut connection);
        let migrations_before = migration_versions(&mut connection);

        assert!(open_project(project.path()).is_err());

        assert_eq!(
            read_project_metadata(&mut connection)
                .unwrap()
                .schema_version,
            1
        );
        assert_eq!(migration_versions(&mut connection), migrations_before);
        assert_eq!(table_names(&mut connection), tables_before);
        assert!(!table_names(&mut connection).contains("chapters"));
        assert_eq!(
            diesel::sql_query("SELECT existing_value FROM chapter_puzzle_reviews")
                .get_result::<ExistingValueRow>(&mut connection)
                .unwrap()
                .existing_value,
            "preserve me"
        );
    }

    #[test]
    fn opening_rejects_schema_versions_below_one_without_migrating() {
        let project = TempProjectDb::new("invalid-version");
        let mut connection = project.connection();
        diesel::sql_query(
            "CREATE TABLE project_metadata (\
             id INTEGER PRIMARY KEY, application_id TEXT NOT NULL, schema_version INTEGER NOT NULL, \
             project_name TEXT NOT NULL, created_at TEXT NOT NULL)",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query(
            "INSERT INTO project_metadata VALUES \
             (1, 'chess-material-studio-project', 0, 'Proyecto inválido', '2026-09-08T00:00:00Z')",
        )
        .execute(&mut connection)
        .unwrap();

        assert!(open_project(project.path()).is_err());
        assert!(!table_names(&mut connection).contains("__diesel_schema_migrations"));
        assert_eq!(
            read_project_metadata(&mut connection)
                .unwrap()
                .schema_version,
            0
        );
    }

    #[test]
    fn puzzle_reviews_persist_complete_snapshots_and_decisions() {
        let project = create_test_project("review-persistence");
        let chapter = create_chapter(project.path(), "Ataques", None).unwrap();
        let selected = puzzle("selected");
        let discarded = puzzle("discarded");

        set_puzzle_decision(
            project.path(),
            chapter.id,
            &selected,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            project.path(),
            chapter.id,
            &discarded,
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();

        assert_eq!(
            get_puzzle_decision(project.path(), chapter.id, &selected.puzzle_id).unwrap(),
            Some(ProjectPuzzleDecision::Selected)
        );
        assert_eq!(
            get_puzzle_decision(project.path(), chapter.id, &discarded.puzzle_id).unwrap(),
            Some(ProjectPuzzleDecision::Discarded)
        );
        assert_eq!(
            get_puzzle_decision(project.path(), chapter.id, "unreviewed").unwrap(),
            None
        );

        let reviews = list_chapter_puzzle_reviews(project.path(), chapter.id).unwrap();
        let selected_review = reviews
            .iter()
            .find(|review| review.puzzle.puzzle_id == selected.puzzle_id)
            .unwrap();
        assert_eq!(selected_review.decision, ProjectPuzzleDecision::Selected);
        assert_puzzle_matches(&selected_review.puzzle, &selected);
        assert!(chrono::DateTime::parse_from_rfc3339(&selected_review.reviewed_at).is_ok());

        let selected_puzzles =
            list_selected_puzzles_for_chapter(project.path(), chapter.id).unwrap();
        assert_eq!(selected_puzzles.len(), 1);
        assert_puzzle_matches(&selected_puzzles[0], &selected);
    }

    #[test]
    fn reviewed_puzzle_ids_are_chapter_scoped_and_read_only() {
        let project = create_test_project("reviewed-puzzle-ids");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let selected = puzzle("selected");
        let discarded = puzzle("discarded");
        let other_chapter = puzzle("other-chapter");

        set_puzzle_decision(project.path(), first.id, &selected, ProjectPuzzleDecision::Selected)
            .unwrap();
        set_puzzle_decision(project.path(), first.id, &discarded, ProjectPuzzleDecision::Discarded)
            .unwrap();
        set_puzzle_decision(
            project.path(),
            second.id,
            &other_chapter,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let before = list_chapter_puzzle_reviews(project.path(), first.id)
            .unwrap()
            .into_iter()
            .map(|review| (review.puzzle.puzzle_id, review.decision, review.reviewed_at))
            .collect::<Vec<_>>();

        let reviewed = list_reviewed_puzzle_ids_for_chapter(project.path(), first.id).unwrap();

        assert_eq!(reviewed, HashSet::from([selected.puzzle_id.clone(), discarded.puzzle_id]));
        assert!(!reviewed.contains(&other_chapter.puzzle_id));
        let after = list_chapter_puzzle_reviews(project.path(), first.id)
            .unwrap()
            .into_iter()
            .map(|review| (review.puzzle.puzzle_id, review.decision, review.reviewed_at))
            .collect::<Vec<_>>();
        assert_eq!(after, before);
        assert!(list_reviewed_puzzle_ids_for_chapter(project.path(), 999).is_err());

        clear_puzzle_decision(project.path(), first.id, &selected.puzzle_id).unwrap();
        assert!(!list_reviewed_puzzle_ids_for_chapter(project.path(), first.id)
            .unwrap()
            .contains(&selected.puzzle_id));
    }

    #[test]
    fn puzzle_decisions_validate_chapters_change_and_clear_idempotently() {
        let project = create_test_project("review-changes");
        let chapter = create_chapter(project.path(), "Cambios", None).unwrap();
        let value = puzzle("changeable");

        assert!(
            set_puzzle_decision(project.path(), 999, &value, ProjectPuzzleDecision::Selected)
                .is_err()
        );
        assert!(get_puzzle_decision(project.path(), 999, &value.puzzle_id).is_err());
        assert!(clear_puzzle_decision(project.path(), 999, &value.puzzle_id).is_err());
        assert!(list_chapter_puzzle_reviews(project.path(), 999).is_err());

        set_puzzle_decision(
            project.path(),
            chapter.id,
            &value,
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        set_puzzle_decision(
            project.path(),
            chapter.id,
            &value,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        assert_eq!(
            get_puzzle_decision(project.path(), chapter.id, &value.puzzle_id).unwrap(),
            Some(ProjectPuzzleDecision::Selected)
        );
        set_puzzle_decision(
            project.path(),
            chapter.id,
            &value,
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        clear_puzzle_decision(project.path(), chapter.id, &value.puzzle_id).unwrap();
        clear_puzzle_decision(project.path(), chapter.id, &value.puzzle_id).unwrap();
        assert_eq!(
            get_puzzle_decision(project.path(), chapter.id, &value.puzzle_id).unwrap(),
            None
        );
    }

    #[test]
    fn chapter_puzzle_reviews_are_ordered_by_timestamp_then_puzzle_id() {
        let project = create_test_project("review-order");
        let chapter = create_chapter(project.path(), "Orden", None).unwrap();
        for value in [puzzle("B"), puzzle("A")] {
            set_puzzle_decision(
                project.path(),
                chapter.id,
                &value,
                ProjectPuzzleDecision::Discarded,
            )
            .unwrap();
        }
        let mut connection = project.connection();
        diesel::sql_query("UPDATE chapter_puzzle_reviews SET reviewed_at = '2026-09-08T00:00:00Z'")
            .execute(&mut connection)
            .unwrap();

        assert_eq!(
            list_chapter_puzzle_reviews(project.path(), chapter.id)
                .unwrap()
                .into_iter()
                .map(|review| review.puzzle.puzzle_id)
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
    }

    #[test]
    fn project_selected_puzzles_are_grouped_ordered_and_read_only() {
        let project = create_test_project("project-selected-puzzles");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let discarded = puzzle("discarded");
        let first_b = puzzle("first-b");
        let first_a = puzzle("first-a");
        let second_a = puzzle("second-a");

        for (chapter_id, puzzle, decision) in [
            (first.id, &discarded, ProjectPuzzleDecision::Discarded),
            (first.id, &first_b, ProjectPuzzleDecision::Selected),
            (first.id, &first_a, ProjectPuzzleDecision::Selected),
            (second.id, &second_a, ProjectPuzzleDecision::Selected),
        ] {
            set_puzzle_decision(project.path(), chapter_id, puzzle, decision).unwrap();
        }
        let mut connection = project.connection();
        diesel::sql_query("UPDATE chapter_puzzle_reviews SET reviewed_at = '2026-09-09T00:00:00Z'")
            .execute(&mut connection)
            .unwrap();
        let before = list_chapter_puzzle_reviews(project.path(), first.id)
            .unwrap()
            .into_iter()
            .map(|review| (review.puzzle.puzzle_id, review.decision, review.reviewed_at))
            .collect::<Vec<_>>();

        let chapters = list_selected_puzzles_by_chapter(project.path()).unwrap();

        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].chapter_id, first.id);
        assert_eq!(chapters[0].chapter_name, "Primero");
        assert_eq!(chapters[0].chapter_position, first.position);
        assert_eq!(
            chapters[0]
                .puzzles
                .iter()
                .map(|puzzle| puzzle.puzzle_id.as_str())
                .collect::<Vec<_>>(),
            ["first-a", "first-b"]
        );
        assert_puzzle_matches(&chapters[0].puzzles[0], &first_a);
        assert_eq!(chapters[1].chapter_id, second.id);
        assert_eq!(chapters[1].chapter_name, "Segundo");
        assert_eq!(chapters[1].puzzles[0].puzzle_id, "second-a");
        let after = list_chapter_puzzle_reviews(project.path(), first.id)
            .unwrap()
            .into_iter()
            .map(|review| (review.puzzle.puzzle_id, review.decision, review.reviewed_at))
            .collect::<Vec<_>>();
        assert_eq!(after, before);
    }

    #[test]
    fn project_selected_puzzles_omit_empty_chapters_and_report_unavailable_projects() {
        let project = create_test_project("project-selected-empty");
        let chapter = create_chapter(project.path(), "Vacío", None).unwrap();
        set_puzzle_decision(
            project.path(),
            chapter.id,
            &puzzle("discarded-only"),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();

        assert!(
            list_selected_puzzles_by_chapter(project.path())
                .unwrap()
                .is_empty()
        );

        std::fs::remove_file(project.path()).unwrap();
        assert!(list_selected_puzzles_by_chapter(project.path()).is_err());

        std::fs::write(project.path(), "not a project").unwrap();
        assert!(list_selected_puzzles_by_chapter(project.path()).is_err());
    }

    #[test]
    fn project_selected_puzzles_do_not_upgrade_an_older_project() {
        let project = TempProjectDb::new("project-selected-read-only-schema");
        create_valid_v2_project(&project);

        assert!(list_selected_puzzles_by_chapter(project.path()).is_err());

        let mut connection = project.connection();
        assert_eq!(
            read_project_metadata(&mut connection)
                .unwrap()
                .schema_version,
            2
        );
        assert!(!table_names(&mut connection).contains("chapter_puzzle_reviews"));
    }

    #[test]
    fn duplicate_selected_puzzles_are_protected_across_chapters() {
        let project = create_test_project("duplicate-selections");
        let first = create_chapter(project.path(), "Primero", None).unwrap();
        let second = create_chapter(project.path(), "Segundo", None).unwrap();
        let value = puzzle("shared");

        set_puzzle_decision(
            project.path(),
            first.id,
            &value,
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        set_puzzle_decision(
            project.path(),
            second.id,
            &value,
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        set_puzzle_decision(
            project.path(),
            first.id,
            &value,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();

        let error = set_puzzle_decision(
            project.path(),
            second.id,
            &value,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap_err();
        assert!(error.contains("already selected"));
        assert_eq!(
            get_puzzle_decision(project.path(), first.id, &value.puzzle_id).unwrap(),
            Some(ProjectPuzzleDecision::Selected)
        );
        assert_eq!(
            get_puzzle_decision(project.path(), second.id, &value.puzzle_id).unwrap(),
            Some(ProjectPuzzleDecision::Discarded)
        );
        assert_eq!(
            find_selected_puzzle_chapter(project.path(), &value.puzzle_id)
                .unwrap()
                .unwrap()
                .id,
            first.id
        );

        clear_puzzle_decision(project.path(), first.id, &value.puzzle_id).unwrap();
        set_puzzle_decision(
            project.path(),
            second.id,
            &value,
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        assert_eq!(
            find_selected_puzzle_chapter(project.path(), &value.puzzle_id)
                .unwrap()
                .unwrap()
                .id,
            second.id
        );
    }

    fn create_test_project(label: &str) -> TempProjectDb {
        let project = TempProjectDb::new(label);
        create_project(project.path(), "Proyecto de prueba").unwrap();
        project
    }

    fn create_valid_v1_project(project: &TempProjectDb) {
        create_v1_project_with_application_id(project, PROJECT_APPLICATION_ID);
    }

    fn create_valid_v2_project(project: &TempProjectDb) {
        create_valid_v1_project(project);
        let mut connection = project.connection();
        diesel::sql_query(
            "CREATE TABLE chapters (\
             id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL, \
             target_puzzle_count INTEGER NULL, created_at TEXT NOT NULL, UNIQUE (position))",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query(
            "INSERT INTO __diesel_schema_migrations (version) VALUES ('20260908010000')",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query("UPDATE project_metadata SET schema_version = 2")
            .execute(&mut connection)
            .unwrap();
    }

    fn create_v1_project_with_application_id(project: &TempProjectDb, application_id: &str) {
        let mut connection = project.connection();
        diesel::sql_query(
            "CREATE TABLE project_metadata (\
             id INTEGER PRIMARY KEY CHECK (id = 1), \
             application_id TEXT NOT NULL, \
             schema_version INTEGER NOT NULL CHECK (schema_version >= 1), \
             project_name TEXT NOT NULL, \
             created_at TEXT NOT NULL)",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE __diesel_schema_migrations (\
             version VARCHAR(50) PRIMARY KEY NOT NULL, \
             run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query(
            "INSERT INTO __diesel_schema_migrations (version) VALUES ('20260908000000')",
        )
        .execute(&mut connection)
        .unwrap();
        diesel::sql_query(
            "INSERT INTO project_metadata \
             (id, application_id, schema_version, project_name, created_at) \
             VALUES (1, ?, 1, 'Proyecto v1', '2026-09-08T00:00:00Z')",
        )
        .bind::<Text, _>(application_id)
        .execute(&mut connection)
        .unwrap();
    }

    fn puzzle(puzzle_id: &str) -> Puzzle {
        Puzzle {
            puzzle_id: puzzle_id.into(),
            fen: format!("fen-{puzzle_id}"),
            moves: format!("e2e4 e7e5 {puzzle_id}"),
            rating: 1800,
            rating_deviation: 75,
            popularity: 92,
            nb_plays: 1234,
            themes: format!("fork {puzzle_id}"),
            game_url: format!("https://lichess.org/{puzzle_id}"),
            opening: format!("Italian_Game {puzzle_id}"),
        }
    }

    fn assert_puzzle_matches(actual: &Puzzle, expected: &Puzzle) {
        assert_eq!(actual.puzzle_id, expected.puzzle_id);
        assert_eq!(actual.fen, expected.fen);
        assert_eq!(actual.moves, expected.moves);
        assert_eq!(actual.rating, expected.rating);
        assert_eq!(actual.rating_deviation, expected.rating_deviation);
        assert_eq!(actual.popularity, expected.popularity);
        assert_eq!(actual.nb_plays, expected.nb_plays);
        assert_eq!(actual.themes, expected.themes);
        assert_eq!(actual.game_url, expected.game_url);
        assert_eq!(actual.opening, expected.opening);
    }

    fn chapter_names(chapters: &[ProjectChapter]) -> Vec<&str> {
        chapters
            .iter()
            .map(|chapter| chapter.name.as_str())
            .collect()
    }

    fn table_names(connection: &mut SqliteConnection) -> HashSet<String> {
        diesel::sql_query("SELECT name FROM sqlite_master WHERE type = 'table'")
            .load::<TableName>(connection)
            .unwrap()
            .into_iter()
            .map(|table| table.name)
            .collect()
    }

    fn migration_versions(connection: &mut SqliteConnection) -> HashSet<String> {
        diesel::sql_query("SELECT version FROM __diesel_schema_migrations")
            .load::<MigrationVersion>(connection)
            .unwrap()
            .into_iter()
            .map(|migration| migration.version)
            .collect()
    }
}
