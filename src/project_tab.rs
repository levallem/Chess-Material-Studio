use iced::widget::{Button, Column, Container, Scrollable, Text, TextInput, checkbox};
use iced::{Alignment, Element, Length, Task, alignment};
use iced_aw::TabLabel;
use rfd::AsyncFileDialog;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chess_material_studio::project::{
    ProjectChapter, ProjectMetadata, ProjectPuzzleDecision, clear_puzzle_decision, create_chapter,
    create_project, find_selected_puzzle_chapter, get_puzzle_decision, list_chapters,
    list_reviewed_puzzle_ids_for_chapter,
    list_selected_puzzles_by_chapter, list_selected_puzzles_for_chapter, open_project,
    set_puzzle_decision,
};

use crate::lang;
use crate::models::Puzzle;
use crate::styles::btn_style_simple;
use crate::{Message, Tab};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveProject {
    path: PathBuf,
    metadata: ProjectMetadata,
    chapters: Vec<ProjectChapter>,
    active_chapter_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PuzzleReviewContext {
    path: PathBuf,
    chapter_id: i32,
    chapter_name: String,
    puzzle_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CachedPuzzleReviewDecision {
    Loaded(Option<ProjectPuzzleDecision>),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedPuzzleReview {
    context: PuzzleReviewContext,
    decision: CachedPuzzleReviewDecision,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedPuzzlesContext {
    path: PathBuf,
    chapter_id: i32,
}

#[derive(Debug, Clone)]
enum CachedSelectedPuzzles {
    Loaded(Vec<chess_material_studio::models::Puzzle>),
    Failed { status: String },
}

#[derive(Debug, Clone)]
struct SelectedPuzzlesCache {
    context: SelectedPuzzlesContext,
    state: CachedSelectedPuzzles,
}

#[derive(Debug, Clone)]
struct ProjectPgnExportSnapshot {
    project_name: String,
    chapters: Vec<crate::export::ProjectPgnChapter>,
}

#[derive(Debug, Clone)]
pub enum SelectedPuzzlesPgnExportResult {
    Cancelled,
    Finished(Result<(), String>),
}

#[derive(Debug, Clone)]
pub enum ProjectPgnExportResult {
    Cancelled,
    Finished(Result<(), String>),
}

#[derive(Debug, Clone)]
pub enum SelectedPuzzlesPdfExportResult {
    Cancelled,
    Finished(Result<(), String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleReviewView {
    pub chapter_name: String,
    pub decision: Option<ProjectPuzzleDecision>,
    pub decision_loaded: bool,
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum ProjectMessage {
    ProjectNameChanged(String),
    ChooseNewProjectPath,
    NewProjectPathChosen(Option<PathBuf>),
    CreateProject,
    ChooseProjectToOpen,
    ProjectToOpenChosen(Option<PathBuf>),
    CloseProject,
    ChapterNameChanged(String),
    TargetPuzzleCountChanged(String),
    CreateChapter,
    SelectChapter(i32),
    SetChapterExportSelected { chapter_id: i32, selected: bool },
    LoadSelectedPuzzles,
    ExportProjectPgn,
    ProjectPgnExportFinished(ProjectPgnExportResult),
    ExportSelectedPuzzlesPgn,
    SelectedPuzzlesPgnExportFinished(SelectedPuzzlesPgnExportResult),
    ExportSelectedPuzzlesPdf,
    SelectedPuzzlesPdfExportFinished(SelectedPuzzlesPdfExportResult),
}

pub struct ProjectTab {
    pub lang: lang::Language,
    active_project: Option<ActiveProject>,
    project_name: String,
    new_project_path: Option<PathBuf>,
    chapter_name: String,
    target_puzzle_count: String,
    status: String,
    review_cache: Option<CachedPuzzleReview>,
    selected_puzzles_cache: Option<SelectedPuzzlesCache>,
    export_selected_chapter_ids: HashSet<i32>,
}

impl ProjectTab {
    pub fn new() -> Self {
        Self {
            lang: crate::config::SETTINGS.lang,
            active_project: None,
            project_name: String::new(),
            new_project_path: None,
            chapter_name: String::new(),
            target_puzzle_count: String::new(),
            status: String::new(),
            review_cache: None,
            selected_puzzles_cache: None,
            export_selected_chapter_ids: HashSet::new(),
        }
    }

    pub fn update(&mut self, message: ProjectMessage) -> Task<Message> {
        match message {
            ProjectMessage::ProjectNameChanged(value) => {
                self.project_name = value;
                self.status.clear();
                Task::none()
            }
            ProjectMessage::ChooseNewProjectPath => Task::perform(
                Self::choose_new_project_path(self.project_name.clone()),
                |path| Message::Project(ProjectMessage::NewProjectPathChosen(path)),
            ),
            ProjectMessage::NewProjectPathChosen(path) => {
                self.new_project_path = path;
                self.status.clear();
                Task::none()
            }
            ProjectMessage::CreateProject => {
                self.create_project_from_draft();
                Task::none()
            }
            ProjectMessage::ChooseProjectToOpen => {
                Task::perform(Self::choose_project_to_open(), |path| {
                    Message::Project(ProjectMessage::ProjectToOpenChosen(path))
                })
            }
            ProjectMessage::ProjectToOpenChosen(Some(path)) => {
                self.open_project_path(&path);
                Task::none()
            }
            ProjectMessage::ProjectToOpenChosen(None) => Task::none(),
            ProjectMessage::CloseProject => {
                self.active_project = None;
                self.export_selected_chapter_ids.clear();
                self.chapter_name.clear();
                self.target_puzzle_count.clear();
                self.status.clear();
                self.clear_puzzle_review_cache();
                self.clear_selected_puzzles_cache();
                Task::none()
            }
            ProjectMessage::ChapterNameChanged(value) => {
                self.chapter_name = value;
                self.status.clear();
                Task::none()
            }
            ProjectMessage::TargetPuzzleCountChanged(value) => {
                self.target_puzzle_count = value;
                self.status.clear();
                Task::none()
            }
            ProjectMessage::CreateChapter => {
                self.create_chapter_from_draft();
                Task::none()
            }
            ProjectMessage::SelectChapter(chapter_id) => {
                self.select_chapter(chapter_id);
                Task::none()
            }
            ProjectMessage::SetChapterExportSelected {
                chapter_id,
                selected,
            } => {
                self.set_chapter_export_selected(chapter_id, selected);
                Task::none()
            }
            ProjectMessage::LoadSelectedPuzzles => self
                .selected_puzzle_load_batch()
                .map(Message::LoadProjectPuzzles)
                .map(Task::done)
                .unwrap_or_else(Task::none),
            ProjectMessage::ExportProjectPgn => match self.project_pgn_export_snapshot() {
                Ok(Some(snapshot)) => Task::perform(Self::export_project_pgn(snapshot), |result| {
                    Message::Project(ProjectMessage::ProjectPgnExportFinished(result))
                }),
                Ok(None) => {
                    self.status = lang::tr(&self.lang, "no_selected_puzzles_in_project");
                    Task::none()
                }
                Err(error) => {
                    self.status = format!(
                        "{}: {error}",
                        lang::tr(&self.lang, "project_pgn_export_failed")
                    );
                    Task::none()
                }
            },
            ProjectMessage::ProjectPgnExportFinished(result) => {
                self.apply_project_pgn_export_result(result);
                Task::none()
            }
            ProjectMessage::ExportSelectedPuzzlesPgn => self
                .selected_puzzle_export_batch()
                .map(|puzzles| {
                    Task::perform(Self::export_selected_puzzles_pgn(puzzles), |result| {
                        Message::Project(ProjectMessage::SelectedPuzzlesPgnExportFinished(result))
                    })
                })
                .unwrap_or_else(Task::none),
            ProjectMessage::SelectedPuzzlesPgnExportFinished(result) => {
                self.apply_selected_puzzles_pgn_export_result(result);
                Task::none()
            }
            ProjectMessage::ExportSelectedPuzzlesPdf => self
                .selected_puzzle_export_batch()
                .map(|puzzles| {
                    let lang = self.lang;
                    Task::perform(Self::export_selected_puzzles_pdf(puzzles, lang), |result| {
                        Message::Project(ProjectMessage::SelectedPuzzlesPdfExportFinished(result))
                    })
                })
                .unwrap_or_else(Task::none),
            ProjectMessage::SelectedPuzzlesPdfExportFinished(result) => {
                self.apply_selected_puzzles_pdf_export_result(result);
                Task::none()
            }
        }
    }

    async fn choose_new_project_path(project_name: String) -> Option<PathBuf> {
        let stem = project_name.trim();
        let default_name = if stem.is_empty() {
            "project.cms.sqlite".to_owned()
        } else {
            format!("{stem}.cms.sqlite")
        };

        AsyncFileDialog::new()
            .add_filter("Chess Material Studio project", &["cms.sqlite", "sqlite"])
            .set_file_name(default_name)
            .save_file()
            .await
            .map(|file| file.path().to_path_buf())
    }

    async fn choose_project_to_open() -> Option<PathBuf> {
        AsyncFileDialog::new()
            .add_filter("Chess Material Studio project", &["cms.sqlite", "sqlite"])
            .add_filter("SQLite database", &["sqlite", "db"])
            .pick_file()
            .await
            .map(|file| file.path().to_path_buf())
    }

    fn create_project_from_draft(&mut self) {
        if self.project_name.trim().is_empty() {
            self.status = lang::tr(&self.lang, "project_name_required");
            return;
        }
        let Some(path) = self.new_project_path.clone() else {
            self.status = lang::tr(&self.lang, "project_file_required");
            return;
        };
        if !is_cms_project_path(&path) {
            self.status = lang::tr(&self.lang, "project_file_extension_required");
            return;
        }

        match create_project(&path, self.project_name.trim())
            .and_then(|metadata| load_active_project(path.clone(), metadata))
        {
            Ok(active_project) => {
                self.active_project = Some(active_project);
                self.export_selected_chapter_ids.clear();
                self.clear_puzzle_review_cache();
                self.refresh_selected_puzzles();
                self.project_name.clear();
                self.new_project_path = None;
                self.status.clear();
            }
            Err(error) => self.status = error,
        }
    }

    fn open_project_path(&mut self, path: &Path) {
        match open_project(path)
            .and_then(|metadata| load_active_project(path.to_path_buf(), metadata))
        {
            Ok(active_project) => {
                self.active_project = Some(active_project);
                self.export_selected_chapter_ids.clear();
                self.clear_puzzle_review_cache();
                self.refresh_selected_puzzles();
                self.chapter_name.clear();
                self.target_puzzle_count.clear();
                self.status.clear();
            }
            Err(error) => self.status = error,
        }
    }

    fn create_chapter_from_draft(&mut self) {
        if self.chapter_name.trim().is_empty() {
            self.status = lang::tr(&self.lang, "chapter_name_required");
            return;
        }
        let target_puzzle_count = match parse_target_puzzle_count(&self.target_puzzle_count) {
            Ok(target) => target,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let Some(active_project) = self.active_project.as_ref() else {
            self.status = lang::tr(&self.lang, "no_project_open");
            return;
        };
        let path = active_project.path.clone();
        let metadata = active_project.metadata.clone();

        match create_chapter(&path, self.chapter_name.trim(), target_puzzle_count).and_then(
            |chapter| {
                load_active_project(path, metadata).map(|mut refreshed| {
                    refreshed.active_chapter_id = Some(chapter.id);
                    refreshed
                })
            },
        ) {
            Ok(active_project) => {
                self.active_project = Some(active_project);
                self.reconcile_export_selected_chapter_ids();
                self.clear_puzzle_review_cache();
                self.refresh_selected_puzzles();
                self.chapter_name.clear();
                self.target_puzzle_count.clear();
                self.status.clear();
            }
            Err(error) => self.status = error,
        }
    }

    fn select_chapter(&mut self, chapter_id: i32) {
        let Some(active_project) = self.active_project.as_mut() else {
            self.status = lang::tr(&self.lang, "no_project_open");
            return;
        };
        if active_project
            .chapters
            .iter()
            .any(|chapter| chapter.id == chapter_id)
        {
            active_project.active_chapter_id = Some(chapter_id);
            self.status.clear();
            self.clear_puzzle_review_cache();
            self.refresh_selected_puzzles();
        }
    }

    fn set_chapter_export_selected(&mut self, chapter_id: i32, selected: bool) {
        let chapter_exists = self.active_project.as_ref().is_some_and(|active_project| {
            active_project
                .chapters
                .iter()
                .any(|chapter| chapter.id == chapter_id)
        });
        if !chapter_exists {
            return;
        }

        if selected {
            self.export_selected_chapter_ids.insert(chapter_id);
        } else {
            self.export_selected_chapter_ids.remove(&chapter_id);
        }
    }

    fn reconcile_export_selected_chapter_ids(&mut self) {
        let Some(active_project) = self.active_project.as_ref() else {
            self.export_selected_chapter_ids.clear();
            return;
        };
        self.export_selected_chapter_ids.retain(|chapter_id| {
            active_project
                .chapters
                .iter()
                .any(|chapter| chapter.id == *chapter_id)
        });
    }

    fn selected_export_chapter_ids(&self) -> &HashSet<i32> {
        &self.export_selected_chapter_ids
    }

    fn selected_export_chapter_count(&self) -> usize {
        self.selected_export_chapter_ids().len()
    }

    fn export_chapter_selection_rows(&self) -> Vec<(i32, String, bool)> {
        let Some(active_project) = self.active_project.as_ref() else {
            return Vec::new();
        };
        active_project
            .chapters
            .iter()
            .map(|chapter| {
                (
                    chapter.id,
                    chapter.name.clone(),
                    self.export_selected_chapter_ids.contains(&chapter.id),
                )
            })
            .collect()
    }

    fn active_chapter(&self) -> Option<&ProjectChapter> {
        let active_project = self.active_project.as_ref()?;
        let chapter_id = active_project.active_chapter_id?;
        active_project
            .chapters
            .iter()
            .find(|chapter| chapter.id == chapter_id)
    }

    fn selected_count(&self) -> Option<usize> {
        self.selected_puzzles().map(|puzzles| puzzles.len())
    }

    async fn export_selected_puzzles_pgn(
        puzzles: Vec<crate::config::Puzzle>,
    ) -> SelectedPuzzlesPgnExportResult {
        let Some(path) = AsyncFileDialog::new()
            .add_filter("PGN", &["pgn"])
            .set_file_name("chapter.pgn")
            .save_file()
            .await
            .map(|file| file.path().to_path_buf())
        else {
            return SelectedPuzzlesPgnExportResult::Cancelled;
        };

        SelectedPuzzlesPgnExportResult::Finished(crate::export::write_pgn(&puzzles, &path))
    }

    async fn export_project_pgn(snapshot: ProjectPgnExportSnapshot) -> ProjectPgnExportResult {
        let Some(path) = AsyncFileDialog::new()
            .add_filter("PGN", &["pgn"])
            .set_file_name("project.pgn")
            .save_file()
            .await
            .map(|file| file.path().to_path_buf())
        else {
            return ProjectPgnExportResult::Cancelled;
        };

        ProjectPgnExportResult::Finished(crate::export::write_project_pgn(
            &snapshot.project_name,
            &snapshot.chapters,
            &path,
        ))
    }

    async fn export_selected_puzzles_pdf(
        puzzles: Vec<crate::config::Puzzle>,
        lang: lang::Language,
    ) -> SelectedPuzzlesPdfExportResult {
        let Some(path) = AsyncFileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("chapter.pdf")
            .save_file()
            .await
            .map(|file| file.path().to_path_buf())
        else {
            return SelectedPuzzlesPdfExportResult::Cancelled;
        };

        SelectedPuzzlesPdfExportResult::Finished(crate::export::write_pdf_all(
            &puzzles, &lang, &path,
        ))
    }

    pub fn reviewed_puzzle_ids_for_active_chapter(
        &self,
    ) -> Result<Option<HashSet<String>>, String> {
        let Some(active_project) = self.active_project.as_ref() else {
            return Ok(None);
        };
        let Some(chapter_id) = active_project.active_chapter_id else {
            return Ok(None);
        };
        list_reviewed_puzzle_ids_for_chapter(&active_project.path, chapter_id).map(Some)
    }

    pub fn refresh_puzzle_review(&mut self, puzzle: Option<&Puzzle>) {
        let Some(puzzle) = puzzle else {
            self.clear_puzzle_review_cache();
            return;
        };
        let Some(context) = self.puzzle_review_context(puzzle) else {
            self.clear_puzzle_review_cache();
            return;
        };
        if self.review_cache.as_ref().is_some_and(|cache| cache.context == context) {
            return;
        }
        self.load_puzzle_review(context, false);
    }

    pub fn set_puzzle_review(&mut self, puzzle: &Puzzle, decision: ProjectPuzzleDecision) {
        let Some(context) = self.puzzle_review_context(puzzle) else {
            self.clear_puzzle_review_cache();
            return;
        };
        let stored_puzzle = project_puzzle(puzzle);
        match set_puzzle_decision(&context.path, context.chapter_id, &stored_puzzle, decision) {
            Ok(()) => {
                self.review_cache = Some(CachedPuzzleReview {
                    context,
                    decision: CachedPuzzleReviewDecision::Loaded(Some(decision)),
                    status: String::new(),
                });
                self.refresh_selected_puzzles();
            }
            Err(error) if decision == ProjectPuzzleDecision::Selected && error == "puzzle is already selected in another chapter" => {
                self.set_duplicate_selection_status(&context, puzzle, error);
            }
            Err(error) => self.load_puzzle_review_after_error(context, error),
        }
    }

    pub fn clear_puzzle_review(&mut self, puzzle: &Puzzle) {
        let Some(context) = self.puzzle_review_context(puzzle) else {
            self.clear_puzzle_review_cache();
            return;
        };
        match clear_puzzle_decision(&context.path, context.chapter_id, &puzzle.puzzle_id) {
            Ok(()) => {
                self.review_cache = Some(CachedPuzzleReview {
                    context,
                    decision: CachedPuzzleReviewDecision::Loaded(None),
                    status: String::new(),
                });
                self.refresh_selected_puzzles();
            }
            Err(error) => self.load_puzzle_review_after_error(context, error),
        }
    }

    pub fn review_view(&self) -> Option<PuzzleReviewView> {
        let cache = self.review_cache.as_ref()?;
        let (decision, decision_loaded) = match cache.decision {
            CachedPuzzleReviewDecision::Loaded(decision) => (decision, true),
            CachedPuzzleReviewDecision::Unavailable => (None, false),
        };
        Some(PuzzleReviewView { chapter_name: cache.context.chapter_name.clone(), decision, decision_loaded, status: cache.status.clone() })
    }

    #[cfg(test)]
    pub(crate) fn cached_review_puzzle_id(&self) -> Option<&str> {
        self.review_cache
            .as_ref()
            .map(|cache| cache.context.puzzle_id.as_str())
    }

    fn puzzle_review_context(&self, puzzle: &Puzzle) -> Option<PuzzleReviewContext> {
        let active_project = self.active_project.as_ref()?;
        let chapter_id = active_project.active_chapter_id?;
        let chapter = active_project.chapters.iter().find(|chapter| chapter.id == chapter_id)?;
        Some(PuzzleReviewContext { path: active_project.path.clone(), chapter_id, chapter_name: chapter.name.clone(), puzzle_id: puzzle.puzzle_id.clone() })
    }

    fn load_puzzle_review(&mut self, context: PuzzleReviewContext, preserve_on_error: bool) {
        match get_puzzle_decision(&context.path, context.chapter_id, &context.puzzle_id) {
            Ok(decision) => self.review_cache = Some(CachedPuzzleReview { context, decision: CachedPuzzleReviewDecision::Loaded(decision), status: String::new() }),
            Err(error) => {
                let error_status = self.review_error_status(&error);
                let preserves_current_context = preserve_on_error && self.review_cache.as_ref().is_some_and(|cache| cache.context == context);
                if preserves_current_context {
                    if let Some(cache) = self.review_cache.as_mut() { cache.status = error_status; }
                } else {
                    self.review_cache = Some(CachedPuzzleReview { context, decision: CachedPuzzleReviewDecision::Unavailable, status: error_status });
                }
            }
        }
    }

    fn load_puzzle_review_after_error(&mut self, context: PuzzleReviewContext, error: String) {
        self.load_puzzle_review(context, true);
        let error_status = self.review_error_status(&error);
        if let Some(cache) = self.review_cache.as_mut() {
            if cache.status.is_empty() { cache.status = error_status; }
        }
    }

    fn set_duplicate_selection_status(&mut self, context: &PuzzleReviewContext, puzzle: &Puzzle, error: String) {
        let status = match find_selected_puzzle_chapter(&context.path, &puzzle.puzzle_id) {
            Ok(Some(chapter)) => format!("{}: {}", lang::tr(&self.lang, "already_selected_in_chapter"), chapter.name),
            Ok(None) | Err(_) => self.review_error_status(&error),
        };
        if let Some(cache) = self.review_cache.as_mut() && cache.context == *context {
            cache.status = status;
        } else {
            self.load_puzzle_review(context.clone(), false);
            if let Some(cache) = self.review_cache.as_mut() { cache.status = status; }
        }
    }

    fn review_error_status(&self, error: &str) -> String {
        format!("{}: {error}", lang::tr(&self.lang, "review_error"))
    }

    fn clear_puzzle_review_cache(&mut self) { self.review_cache = None; }

    fn selected_puzzles_context(&self) -> Option<SelectedPuzzlesContext> {
        let active_project = self.active_project.as_ref()?;
        Some(SelectedPuzzlesContext {
            path: active_project.path.clone(),
            chapter_id: active_project.active_chapter_id?,
        })
    }

    fn refresh_selected_puzzles(&mut self) {
        let Some(context) = self.selected_puzzles_context() else {
            self.clear_selected_puzzles_cache();
            return;
        };
        let state = match list_selected_puzzles_for_chapter(&context.path, context.chapter_id) {
            Ok(puzzles) => CachedSelectedPuzzles::Loaded(puzzles),
            Err(error) => CachedSelectedPuzzles::Failed {
                status: format!("{}: {error}", lang::tr(&self.lang, "selected_puzzles_error")),
            },
        };
        self.selected_puzzles_cache = Some(SelectedPuzzlesCache { context, state });
    }

    fn selected_puzzles(&self) -> Option<&[chess_material_studio::models::Puzzle]> {
        let context = self.selected_puzzles_context()?;
        let cache = self.selected_puzzles_cache.as_ref()?;
        if cache.context != context {
            return None;
        }
        match &cache.state {
            CachedSelectedPuzzles::Loaded(puzzles) => Some(puzzles),
            CachedSelectedPuzzles::Failed { .. } => None,
        }
    }

    fn selected_puzzle_load_batch(&self) -> Option<Vec<crate::config::Puzzle>> {
        let puzzles = self.selected_puzzles()?;
        (!puzzles.is_empty()).then(|| puzzles.iter().map(app_puzzle).collect())
    }

    fn selected_puzzle_export_batch(&self) -> Option<Vec<crate::config::Puzzle>> {
        self.selected_puzzle_load_batch()
    }

    fn project_pgn_export_snapshot(&self) -> Result<Option<ProjectPgnExportSnapshot>, String> {
        let active_project = self
            .active_project
            .as_ref()
            .ok_or_else(|| "no project is open".to_string())?;
        let chapters = list_selected_puzzles_by_chapter(&active_project.path)?
            .into_iter()
            .map(|chapter| crate::export::ProjectPgnChapter {
                name: chapter.chapter_name,
                puzzles: chapter.puzzles.iter().map(app_puzzle).collect(),
            })
            .collect::<Vec<_>>();
        Ok((!chapters.is_empty()).then_some(ProjectPgnExportSnapshot {
            project_name: active_project.metadata.project_name.clone(),
            chapters,
        }))
    }

    fn apply_selected_puzzles_pgn_export_result(&mut self, result: SelectedPuzzlesPgnExportResult) {
        match result {
            SelectedPuzzlesPgnExportResult::Cancelled => {}
            SelectedPuzzlesPgnExportResult::Finished(Ok(())) => {
                self.status = lang::tr(&self.lang, "chapter_pgn_exported");
            }
            SelectedPuzzlesPgnExportResult::Finished(Err(error)) => {
                self.status = format!(
                    "{}: {error}",
                    lang::tr(&self.lang, "chapter_pgn_export_failed")
                );
            }
        }
    }

    fn apply_project_pgn_export_result(&mut self, result: ProjectPgnExportResult) {
        match result {
            ProjectPgnExportResult::Cancelled => {}
            ProjectPgnExportResult::Finished(Ok(())) => {
                self.status = lang::tr(&self.lang, "project_pgn_exported");
            }
            ProjectPgnExportResult::Finished(Err(error)) => {
                self.status = format!(
                    "{}: {error}",
                    lang::tr(&self.lang, "project_pgn_export_failed")
                );
            }
        }
    }

    fn apply_selected_puzzles_pdf_export_result(&mut self, result: SelectedPuzzlesPdfExportResult) {
        match result {
            SelectedPuzzlesPdfExportResult::Cancelled => {}
            SelectedPuzzlesPdfExportResult::Finished(Ok(())) => {
                self.status = lang::tr(&self.lang, "chapter_pdf_exported");
            }
            SelectedPuzzlesPdfExportResult::Finished(Err(error)) => {
                self.status = format!(
                    "{}: {error}",
                    lang::tr(&self.lang, "chapter_pdf_export_failed")
                );
            }
        }
    }

    fn selected_puzzles_error(&self) -> Option<&str> {
        let context = self.selected_puzzles_context()?;
        let cache = self.selected_puzzles_cache.as_ref()?;
        if cache.context != context {
            return None;
        }
        match &cache.state {
            CachedSelectedPuzzles::Loaded(_) => None,
            CachedSelectedPuzzles::Failed { status } => Some(status),
        }
    }

    fn clear_selected_puzzles_cache(&mut self) { self.selected_puzzles_cache = None; }
}

fn load_active_project(path: PathBuf, metadata: ProjectMetadata) -> Result<ActiveProject, String> {
    let chapters = list_chapters(&path)?;
    Ok(ActiveProject {
        path,
        metadata,
        active_chapter_id: chapters.first().map(|chapter| chapter.id),
        chapters,
    })
}

fn project_puzzle(puzzle: &Puzzle) -> chess_material_studio::models::Puzzle {
    chess_material_studio::models::Puzzle {
        puzzle_id: puzzle.puzzle_id.clone(), fen: puzzle.fen.clone(), moves: puzzle.moves.clone(),
        rating: puzzle.rating, rating_deviation: puzzle.rating_deviation, popularity: puzzle.popularity,
        nb_plays: puzzle.nb_plays, themes: puzzle.themes.clone(), game_url: puzzle.game_url.clone(), opening: puzzle.opening.clone(),
    }
}

fn app_puzzle(puzzle: &chess_material_studio::models::Puzzle) -> crate::config::Puzzle {
    crate::config::Puzzle {
        puzzle_id: puzzle.puzzle_id.clone(), fen: puzzle.fen.clone(), moves: puzzle.moves.clone(),
        rating: puzzle.rating, rating_deviation: puzzle.rating_deviation, popularity: puzzle.popularity,
        nb_plays: puzzle.nb_plays, themes: puzzle.themes.clone(), game_url: puzzle.game_url.clone(), opening: puzzle.opening.clone(),
    }
}

fn is_cms_project_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".cms.sqlite"))
}

fn parse_target_puzzle_count(value: &str) -> Result<Option<i32>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let target = value
        .parse::<i32>()
        .map_err(|_| "target puzzle count must be a whole number".to_owned())?;
    if target <= 0 {
        return Err("target puzzle count must be greater than zero".to_owned());
    }
    Ok(Some(target))
}

impl Tab for ProjectTab {
    type Message = Message;

    fn title(&self) -> String {
        lang::tr(&self.lang, "project")
    }

    fn tab_label(&self) -> TabLabel {
        TabLabel::Text(self.title())
    }

    fn content(&self) -> Element<'_, Message> {
        let content: Element<'_, ProjectMessage> =
            if let Some(active_project) = &self.active_project {
                self.active_project_content(active_project)
            } else {
                self.no_project_content()
            }
            .into();

        let content: Element<'_, ProjectMessage> =
            Container::new(Scrollable::new(content).height(Length::Fill))
                .align_x(alignment::Horizontal::Center)
                .height(Length::Fill)
                .width(Length::Fill)
                .into();
        content.map(Message::Project)
    }
}

impl ProjectTab {
    fn no_project_content(&self) -> Column<'_, ProjectMessage> {
        let path_text = self
            .new_project_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| lang::tr(&self.lang, "project_file_not_selected"));

        Column::new()
            .padding([0, 30])
            .spacing(10)
            .align_x(Alignment::Center)
            .push(Text::new(lang::tr(&self.lang, "new_project")))
            .push(
                TextInput::new(&lang::tr(&self.lang, "project_name"), &self.project_name)
                    .on_input(ProjectMessage::ProjectNameChanged),
            )
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "choose_project_file")))
                    .on_press(ProjectMessage::ChooseNewProjectPath)
                    .style(btn_style_simple),
            )
            .push(Text::new(path_text))
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "create_project")))
                    .on_press(ProjectMessage::CreateProject)
                    .style(btn_style_simple),
            )
            .push(Text::new(lang::tr(&self.lang, "or")))
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "open_project")))
                    .on_press(ProjectMessage::ChooseProjectToOpen)
                    .style(btn_style_simple),
            )
            .push(Text::new(&self.status))
    }

    fn active_project_content(&self, active_project: &ActiveProject) -> Column<'_, ProjectMessage> {
        let mut chapters = Column::new().spacing(5);
        for chapter in &active_project.chapters {
            let is_active = active_project.active_chapter_id == Some(chapter.id);
            let label = if is_active {
                format!(
                    "{}: {}",
                    lang::tr(&self.lang, "active_chapter"),
                    chapter.name
                )
            } else {
                chapter.name.clone()
            };
            chapters = chapters.push(
                Button::new(Text::new(label))
                    .on_press(ProjectMessage::SelectChapter(chapter.id))
                    .style(btn_style_simple),
            );
        }

        let mut export_chapter_selection = Column::new().spacing(5);
        for (chapter_id, chapter_name, is_selected) in self.export_chapter_selection_rows() {
            export_chapter_selection =
                export_chapter_selection.push(checkbox(is_selected).label(chapter_name).on_toggle(
                    move |selected| ProjectMessage::SetChapterExportSelected {
                        chapter_id,
                        selected,
                    },
                ));
        }

        let progress = match (self.active_chapter(), self.selected_count()) {
            (Some(chapter), Some(selected)) => match chapter.target_puzzle_count {
                Some(target) => format!(
                    "{} — {selected} / {target} {}",
                    chapter.name,
                    lang::tr(&self.lang, "selected")
                ),
                None => format!(
                    "{} — {selected} {}",
                    chapter.name,
                    lang::tr(&self.lang, "selected")
                ),
            },
            (Some(_), None) => self
                .selected_puzzles_error()
                .map(str::to_owned)
                .unwrap_or_else(|| lang::tr(&self.lang, "selected_puzzles_error")),
            _ => lang::tr(&self.lang, "no_active_chapter"),
        };

        let mut content = Column::new()
            .padding([0, 30])
            .spacing(10)
            .align_x(Alignment::Center)
            .push(Text::new(format!(
                "{}: {}",
                lang::tr(&self.lang, "project_name"),
                active_project.metadata.project_name
            )))
            .push(Text::new(format!(
                "{}: {}",
                lang::tr(&self.lang, "project_file"),
                active_project.path.display()
            )))
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "close_project")))
                    .on_press(ProjectMessage::CloseProject)
                    .style(btn_style_simple),
            )
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "export_project_to_pgn")))
                    .on_press(ProjectMessage::ExportProjectPgn)
                    .style(btn_style_simple),
            )
            .push(Text::new(lang::tr(&self.lang, "chapters")))
            .push(chapters)
            .push(Text::new(lang::tr(
                &self.lang,
                "select_chapters_for_export",
            )))
            .push(export_chapter_selection)
            .push(Text::new(format!(
                "{}: {}",
                lang::tr(&self.lang, "selected_chapters_for_export"),
                self.selected_export_chapter_count()
            )))
            .push(Text::new(progress));

        if let Some(selected_puzzles) = self.selected_puzzles_content() {
            content = content.push(selected_puzzles);
        }

        content
            .push(Text::new(lang::tr(&self.lang, "new_chapter")))
            .push(
                TextInput::new(&lang::tr(&self.lang, "chapter_name"), &self.chapter_name)
                    .on_input(ProjectMessage::ChapterNameChanged),
            )
            .push(
                TextInput::new(
                    &lang::tr(&self.lang, "target_puzzle_count_optional"),
                    &self.target_puzzle_count,
                )
                .on_input(ProjectMessage::TargetPuzzleCountChanged),
            )
            .push(
                Button::new(Text::new(lang::tr(&self.lang, "create_chapter")))
                    .on_press(ProjectMessage::CreateChapter)
                    .style(btn_style_simple),
            )
            .push(Text::new(&self.status))
    }

    fn selected_puzzles_content(&self) -> Option<Column<'_, ProjectMessage>> {
        self.active_chapter()?;
        let mut content = Column::new()
            .spacing(5)
            .push(Text::new(lang::tr(&self.lang, "selected_puzzles")));

        if let Some(puzzles) = self.selected_puzzles() {
            if puzzles.is_empty() {
                return Some(content.push(Text::new(lang::tr(
                    &self.lang,
                    "no_selected_puzzles",
                ))));
            }

            for (index, puzzle) in puzzles.iter().enumerate() {
                content = content.push(Text::new(format!(
                    "{}. {} — {}{}   {} {}",
                    index + 1,
                    puzzle.puzzle_id,
                    lang::tr(&self.lang, "rating"),
                    puzzle.rating,
                    lang::tr(&self.lang, "themes"),
                    puzzle.themes,
                )));
            }
            return Some(
                content
                    .push(
                        Button::new(Text::new(lang::tr(&self.lang, "load_selected_puzzles")))
                            .on_press(ProjectMessage::LoadSelectedPuzzles)
                            .style(btn_style_simple),
                    )
                    .push(
                        Button::new(Text::new(lang::tr(&self.lang, "export_chapter_to_pgn")))
                            .on_press(ProjectMessage::ExportSelectedPuzzlesPgn)
                            .style(btn_style_simple),
                    )
                    .push(
                        Button::new(Text::new(lang::tr(&self.lang, "export_chapter_to_pdf")))
                            .on_press(ProjectMessage::ExportSelectedPuzzlesPdf)
                            .style(btn_style_simple),
                    ),
            );
        }

        Some(content.push(Text::new(
            self.selected_puzzles_error()
                .map(str::to_owned)
                .unwrap_or_else(|| lang::tr(&self.lang, "selected_puzzles_error")),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_material_studio::project::{ProjectPuzzleDecision, set_puzzle_decision};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempProjectDb {
        path: PathBuf,
        directory: PathBuf,
    }

    impl TempProjectDb {
        fn new(label: &str) -> Self {
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_023d_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("test directory should be created");
            Self {
                path: directory.join("project.cms.sqlite"),
                directory,
            }
        }
    }

    impl Drop for TempProjectDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(&self.directory);
        }
    }

    fn sample_puzzle() -> Puzzle {
        Puzzle {
            puzzle_id: "cms-023d-selected".into(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".into(),
            moves: "a1a2".into(),
            rating: 1500,
            rating_deviation: 80,
            popularity: 50,
            nb_plays: 10,
            themes: "hangingPiece".into(),
            game_url: "https://lichess.org/game".into(),
            opening: "".into(),
        }
    }

    #[test]
    fn starts_without_an_active_project() {
        let tab = ProjectTab::new();

        assert!(tab.active_project.is_none());
        assert_eq!(tab.selected_count(), None);
        assert!(tab.selected_export_chapter_ids().is_empty());
        assert_eq!(tab.selected_export_chapter_count(), 0);
    }

    #[test]
    fn export_chapter_selection_is_validated_and_independent_from_the_active_chapter() {
        let project = TempProjectDb::new("export-selection");
        create_project(&project.path, "Selección").unwrap();
        let first = create_chapter(&project.path, "Primero", None).unwrap();
        let second = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let selected_cache_context = tab
            .selected_puzzles_cache
            .as_ref()
            .map(|cache| cache.context.clone());
        let active_before = tab.active_project.as_ref().unwrap().active_chapter_id;
        let _ = tab.update(ProjectMessage::SetChapterExportSelected {
            chapter_id: first.id,
            selected: true,
        });
        let _ = tab.update(ProjectMessage::SetChapterExportSelected {
            chapter_id: second.id,
            selected: true,
        });

        assert_eq!(
            tab.selected_export_chapter_ids(),
            &HashSet::from([first.id, second.id])
        );
        assert_eq!(tab.selected_export_chapter_count(), 2);
        assert_eq!(
            tab.active_project.as_ref().unwrap().active_chapter_id,
            active_before
        );
        assert_eq!(
            tab.selected_puzzles_cache
                .as_ref()
                .map(|cache| cache.context.clone()),
            selected_cache_context
        );

        tab.select_chapter(second.id);
        assert_eq!(
            tab.active_project.as_ref().unwrap().active_chapter_id,
            Some(second.id)
        );
        assert_eq!(
            tab.selected_export_chapter_ids(),
            &HashSet::from([first.id, second.id])
        );

        let _ = tab.update(ProjectMessage::SetChapterExportSelected {
            chapter_id: first.id,
            selected: false,
        });
        let _ = tab.update(ProjectMessage::SetChapterExportSelected {
            chapter_id: 999_999,
            selected: true,
        });

        assert_eq!(
            tab.selected_export_chapter_ids(),
            &HashSet::from([second.id])
        );
        assert_eq!(tab.selected_export_chapter_count(), 1);
    }

    #[test]
    fn export_chapter_selection_survives_chapter_creation_and_follows_editorial_order() {
        let project = TempProjectDb::new("export-selection-create-chapter");
        create_project(&project.path, "Capítulos").unwrap();
        let first = create_chapter(&project.path, "Primero", None).unwrap();
        let second = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.set_chapter_export_selected(first.id, true);

        tab.chapter_name = "Tercero".into();
        tab.create_chapter_from_draft();

        let active = tab.active_project.as_ref().unwrap();
        let third = active.chapters[2].id;
        assert_eq!(
            tab.selected_export_chapter_ids(),
            &HashSet::from([first.id])
        );
        assert!(!tab.selected_export_chapter_ids().contains(&third));
        assert_eq!(
            tab.export_chapter_selection_rows(),
            vec![
                (first.id, "Primero".into(), true),
                (second.id, "Segundo".into(), false),
                (third, "Tercero".into(), false)
            ]
        );

        tab.export_selected_chapter_ids.insert(999_999);
        tab.reconcile_export_selected_chapter_ids();
        assert_eq!(
            tab.selected_export_chapter_ids(),
            &HashSet::from([first.id])
        );
    }

    #[test]
    fn export_chapter_selection_is_cleared_for_project_lifecycle_boundaries() {
        let first_project = TempProjectDb::new("export-selection-first-project");
        create_project(&first_project.path, "Primero").unwrap();
        let first_chapter = create_chapter(&first_project.path, "Uno", None).unwrap();
        let second_project = TempProjectDb::new("export-selection-second-project");
        create_project(&second_project.path, "Segundo").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&first_project.path);
        tab.set_chapter_export_selected(first_chapter.id, true);

        let _ = tab.update(ProjectMessage::CloseProject);
        assert!(tab.selected_export_chapter_ids().is_empty());

        tab.open_project_path(&first_project.path);
        tab.set_chapter_export_selected(first_chapter.id, true);
        tab.open_project_path(&second_project.path);
        assert!(tab.selected_export_chapter_ids().is_empty());

        tab.open_project_path(&first_project.path);
        tab.set_chapter_export_selected(first_chapter.id, true);
        let new_project = TempProjectDb::new("export-selection-new-project");
        tab.project_name = "Nuevo".into();
        tab.new_project_path = Some(new_project.path.clone());
        tab.create_project_from_draft();
        assert!(tab.selected_export_chapter_ids().is_empty());
    }

    #[test]
    fn opening_a_valid_project_loads_metadata_and_chapters() {
        let project = TempProjectDb::new("open");
        create_project(&project.path, "Libro táctico").unwrap();
        let chapter = create_chapter(&project.path, "Ataques", Some(20)).unwrap();
        let mut tab = ProjectTab::new();

        tab.open_project_path(&project.path);

        let active = tab.active_project.as_ref().unwrap();
        assert_eq!(active.metadata.project_name, "Libro táctico");
        assert_eq!(active.chapters, vec![chapter.clone()]);
        assert_eq!(active.active_chapter_id, Some(chapter.id));
    }

    #[test]
    fn invalid_open_keeps_the_previous_active_project() {
        let project = TempProjectDb::new("invalid-open");
        create_project(&project.path, "Válido").unwrap();
        let invalid_path = project.directory.join("not-a-project.sqlite");
        std::fs::write(&invalid_path, "not SQLite").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        let before = tab.active_project.clone();

        tab.open_project_path(&invalid_path);

        assert_eq!(tab.active_project, before);
        assert!(!tab.status.is_empty());
    }

    #[test]
    fn closing_clears_project_and_chapter_state() {
        let project = TempProjectDb::new("close");
        create_project(&project.path, "Cerrar").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let _ = tab.update(ProjectMessage::CloseProject);

        assert!(tab.active_project.is_none());
        assert!(tab.chapter_name.is_empty());
        assert!(tab.target_puzzle_count.is_empty());
    }

    #[test]
    fn creating_a_chapter_refreshes_and_activates_it() {
        let project = TempProjectDb::new("create-chapter");
        create_project(&project.path, "Capítulos").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.chapter_name = "Pieza colgante".into();
        tab.target_puzzle_count = "20".into();

        tab.create_chapter_from_draft();

        let active = tab.active_project.as_ref().unwrap();
        assert_eq!(active.chapters.len(), 1);
        assert_eq!(active.chapters[0].name, "Pieza colgante");
        assert_eq!(active.active_chapter_id, Some(active.chapters[0].id));
    }

    #[test]
    fn selecting_a_chapter_changes_only_the_active_chapter() {
        let project = TempProjectDb::new("select-chapter");
        create_project(&project.path, "Selección").unwrap();
        let first = create_chapter(&project.path, "Primero", None).unwrap();
        let second = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        tab.select_chapter(second.id);

        assert_eq!(
            tab.active_project.as_ref().unwrap().active_chapter_id,
            Some(second.id)
        );
        assert_eq!(
            tab.active_project.as_ref().unwrap().chapters[0].id,
            first.id
        );
    }

    #[test]
    fn selected_count_is_derived_from_the_loaded_snapshot_cache() {
        let project = TempProjectDb::new("selected-count");
        create_project(&project.path, "Progreso").unwrap();
        let chapter = create_chapter(&project.path, "Pieza colgante", Some(20)).unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&sample_puzzle()),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        assert_eq!(tab.selected_count(), Some(1));
    }

    #[test]
    fn selected_puzzles_are_loaded_from_the_active_chapter_snapshot_cache() {
        let project = TempProjectDb::new("selected-cache");
        create_project(&project.path, "Selección").unwrap();
        let chapter = create_chapter(&project.path, "Ataques", Some(20)).unwrap();
        let selected = sample_puzzle();
        let mut discarded = sample_puzzle();
        discarded.puzzle_id = "cms-023g-discarded".into();
        let mut second_selected = sample_puzzle();
        second_selected.puzzle_id = "cms-023g-second-selected".into();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&selected),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&second_selected),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&discarded),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();

        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let selected_puzzles = tab.selected_puzzles().unwrap();
        assert_eq!(selected_puzzles.len(), 2);
        assert_eq!(selected_puzzles[0].puzzle_id, selected.puzzle_id);
        assert_eq!(selected_puzzles[0].fen, selected.fen);
        assert_eq!(selected_puzzles[0].moves, selected.moves);
        assert_eq!(selected_puzzles[0].rating, selected.rating);
        assert_eq!(selected_puzzles[0].rating_deviation, selected.rating_deviation);
        assert_eq!(selected_puzzles[0].popularity, selected.popularity);
        assert_eq!(selected_puzzles[0].nb_plays, selected.nb_plays);
        assert_eq!(selected_puzzles[0].themes, selected.themes);
        assert_eq!(selected_puzzles[0].game_url, selected.game_url);
        assert_eq!(selected_puzzles[0].opening, selected.opening);
        assert_eq!(selected_puzzles[1].puzzle_id, second_selected.puzzle_id);
        assert_eq!(tab.selected_count(), Some(2));
    }

    #[test]
    fn selected_puzzle_load_batch_adapts_every_field_and_preserves_cache_order() {
        let project = TempProjectDb::new("selected-load-batch");
        create_project(&project.path, "Selección").unwrap();
        let chapter = create_chapter(&project.path, "Ataques", None).unwrap();
        let mut first = sample_puzzle();
        first.puzzle_id = "cms-023h-first".into();
        first.opening = "Sicilian Defense".into();
        let mut second = sample_puzzle();
        second.puzzle_id = "cms-023h-second".into();
        second.rating = 1800;
        set_puzzle_decision(&project.path, chapter.id, &project_puzzle(&first), ProjectPuzzleDecision::Selected).unwrap();
        set_puzzle_decision(&project.path, chapter.id, &project_puzzle(&second), ProjectPuzzleDecision::Selected).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let batch = tab.selected_puzzle_load_batch().unwrap();

        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].puzzle_id, first.puzzle_id);
        assert_eq!(batch[0].fen, first.fen);
        assert_eq!(batch[0].moves, first.moves);
        assert_eq!(batch[0].rating, first.rating);
        assert_eq!(batch[0].rating_deviation, first.rating_deviation);
        assert_eq!(batch[0].popularity, first.popularity);
        assert_eq!(batch[0].nb_plays, first.nb_plays);
        assert_eq!(batch[0].themes, first.themes);
        assert_eq!(batch[0].game_url, first.game_url);
        assert_eq!(batch[0].opening, first.opening);
        assert_eq!(batch[1].puzzle_id, second.puzzle_id);
    }

    #[test]
    fn selected_puzzle_load_batch_is_unavailable_for_empty_or_failed_cache() {
        let project = TempProjectDb::new("selected-load-unavailable");
        create_project(&project.path, "Selección").unwrap();
        let chapter = create_chapter(&project.path, "Vacío", None).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        assert_eq!(tab.active_project.as_ref().unwrap().active_chapter_id, Some(chapter.id));
        assert!(tab.selected_puzzle_load_batch().is_none());

        std::fs::remove_file(&project.path).unwrap();
        tab.refresh_selected_puzzles();

        assert!(tab.selected_puzzle_load_batch().is_none());
        assert!(tab.selected_puzzles_error().is_some());
    }

    #[test]
    fn selected_puzzle_export_batch_requires_a_current_nonempty_loaded_cache() {
        let project = TempProjectDb::new("selected-export-unavailable");
        create_project(&project.path, "Exportación").unwrap();
        let chapter = create_chapter(&project.path, "Vacío", None).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        assert!(tab.selected_puzzle_export_batch().is_none());

        let puzzle = sample_puzzle();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        tab.refresh_selected_puzzles();
        assert_eq!(
            tab.selected_puzzle_export_batch().unwrap()[0].puzzle_id,
            puzzle.puzzle_id
        );

        tab.selected_puzzles_cache
            .as_mut()
            .unwrap()
            .context
            .chapter_id += 1;
        assert!(tab.selected_puzzle_export_batch().is_none());

        tab.selected_puzzles_cache
            .as_mut()
            .unwrap()
            .context
            .chapter_id = chapter.id;
        tab.selected_puzzles_cache.as_mut().unwrap().state = CachedSelectedPuzzles::Failed {
            status: "Selected puzzles unavailable".into(),
        };
        assert!(tab.selected_puzzle_export_batch().is_none());
    }

    #[test]
    fn selected_puzzle_export_batch_captures_adapter_order_and_result_status() {
        let project = TempProjectDb::new("selected-export-snapshot");
        create_project(&project.path, "Exportación").unwrap();
        let first_chapter = create_chapter(&project.path, "Primero", None).unwrap();
        let second_chapter = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut first = sample_puzzle();
        first.puzzle_id = "cms-023i-first".into();
        let mut second = sample_puzzle();
        second.puzzle_id = "cms-023i-second".into();
        set_puzzle_decision(
            &project.path,
            first_chapter.id,
            &project_puzzle(&first),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            first_chapter.id,
            &project_puzzle(&second),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let snapshot = tab.selected_puzzle_export_batch().unwrap();
        assert_eq!(
            snapshot
                .iter()
                .map(|puzzle| puzzle.puzzle_id.as_str())
                .collect::<Vec<_>>(),
            ["cms-023i-first", "cms-023i-second"]
        );
        assert_eq!(snapshot[0].fen, first.fen);
        assert_eq!(snapshot[0].opening, first.opening);

        tab.select_chapter(second_chapter.id);
        assert_eq!(snapshot[0].puzzle_id, "cms-023i-first");
        tab.status = "existing status".into();
        tab.apply_selected_puzzles_pgn_export_result(SelectedPuzzlesPgnExportResult::Cancelled);
        assert_eq!(tab.status, "existing status");
        tab.apply_selected_puzzles_pgn_export_result(SelectedPuzzlesPgnExportResult::Finished(Ok(
            (),
        )));
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "chapter_pgn_exported"))
        );
        tab.apply_selected_puzzles_pgn_export_result(SelectedPuzzlesPgnExportResult::Finished(
            Err("disk full".into()),
        ));
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "chapter_pgn_export_failed"))
        );
        assert!(tab.status.contains("disk full"));

        tab.status = "existing PDF status".into();
        tab.apply_selected_puzzles_pdf_export_result(SelectedPuzzlesPdfExportResult::Cancelled);
        assert_eq!(tab.status, "existing PDF status");
        tab.apply_selected_puzzles_pdf_export_result(
            SelectedPuzzlesPdfExportResult::Finished(Ok(())),
        );
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "chapter_pdf_exported"))
        );
        tab.apply_selected_puzzles_pdf_export_result(
            SelectedPuzzlesPdfExportResult::Finished(Err("disk full".into())),
        );
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "chapter_pdf_export_failed"))
        );
        assert!(tab.status.contains("disk full"));
    }

    #[test]
    fn project_pgn_snapshot_reads_all_chapters_and_is_owned() {
        let project = TempProjectDb::new("project-pgn-snapshot");
        create_project(&project.path, "Libro editorial").unwrap();
        let first_chapter = create_chapter(&project.path, "Primero", None).unwrap();
        let second_chapter = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut first = sample_puzzle();
        first.puzzle_id = "cms-023k-first".into();
        let mut second = sample_puzzle();
        second.puzzle_id = "cms-023k-second".into();
        set_puzzle_decision(
            &project.path,
            first_chapter.id,
            &project_puzzle(&first),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            second_chapter.id,
            &project_puzzle(&second),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let snapshot = tab.project_pgn_export_snapshot().unwrap().unwrap();

        assert_eq!(snapshot.project_name, "Libro editorial");
        assert_eq!(
            snapshot
                .chapters
                .iter()
                .map(|chapter| chapter.name.as_str())
                .collect::<Vec<_>>(),
            ["Primero", "Segundo"]
        );
        assert_eq!(snapshot.chapters[0].puzzles[0].puzzle_id, first.puzzle_id);
        tab.select_chapter(second_chapter.id);
        let _ = tab.update(ProjectMessage::CloseProject);
        assert_eq!(snapshot.chapters[1].puzzles[0].puzzle_id, second.puzzle_id);
    }

    #[test]
    fn project_pgn_snapshot_distinguishes_empty_and_read_failures() {
        let project = TempProjectDb::new("project-pgn-empty");
        create_project(&project.path, "Vacío").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        assert!(tab.project_pgn_export_snapshot().unwrap().is_none());

        std::fs::remove_file(&project.path).unwrap();
        assert!(tab.project_pgn_export_snapshot().is_err());
    }

    #[test]
    fn project_pgn_action_reports_empty_read_and_completion_states() {
        let project = TempProjectDb::new("project-pgn-status");
        create_project(&project.path, "Vacío").unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        let _ = tab.update(ProjectMessage::ExportProjectPgn);
        assert_eq!(
            tab.status,
            lang::tr(&tab.lang, "no_selected_puzzles_in_project")
        );

        std::fs::remove_file(&project.path).unwrap();
        let _ = tab.update(ProjectMessage::ExportProjectPgn);
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "project_pgn_export_failed"))
        );

        tab.status = "unchanged".into();
        tab.apply_project_pgn_export_result(ProjectPgnExportResult::Cancelled);
        assert_eq!(tab.status, "unchanged");
        tab.apply_project_pgn_export_result(ProjectPgnExportResult::Finished(Ok(())));
        assert!(
            tab.status
                .contains(&lang::tr(&tab.lang, "project_pgn_exported"))
        );
        tab.apply_project_pgn_export_result(ProjectPgnExportResult::Finished(Err(
            "disk full".into()
        )));
        assert!(tab.status.contains("disk full"));
    }

    #[test]
    fn selected_puzzle_load_batch_is_unavailable_for_a_stale_cache_context() {
        let project = TempProjectDb::new("selected-load-stale-context");
        create_project(&project.path, "Selección").unwrap();
        let chapter = create_chapter(&project.path, "Actual", None).unwrap();
        let stale_chapter = create_chapter(&project.path, "Obsoleto", None).unwrap();
        let puzzle = sample_puzzle();
        set_puzzle_decision(&project.path, chapter.id, &project_puzzle(&puzzle), ProjectPuzzleDecision::Selected).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.selected_puzzles_cache.as_mut().unwrap().context.chapter_id = stale_chapter.id;

        assert!(tab.selected_puzzle_load_batch().is_none());
    }

    #[test]
    fn selected_puzzle_load_batch_uses_the_new_active_chapter_cache() {
        let project = TempProjectDb::new("selected-load-active-chapter");
        create_project(&project.path, "Selección").unwrap();
        let first_chapter = create_chapter(&project.path, "Primero", None).unwrap();
        let second_chapter = create_chapter(&project.path, "Segundo", None).unwrap();
        let mut first = sample_puzzle();
        first.puzzle_id = "cms-023h-first-chapter".into();
        let mut second = sample_puzzle();
        second.puzzle_id = "cms-023h-second-chapter".into();
        set_puzzle_decision(&project.path, first_chapter.id, &project_puzzle(&first), ProjectPuzzleDecision::Selected).unwrap();
        set_puzzle_decision(&project.path, second_chapter.id, &project_puzzle(&second), ProjectPuzzleDecision::Selected).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.select_chapter(second_chapter.id);

        let batch = tab.selected_puzzle_load_batch().unwrap();

        assert_eq!(batch.iter().map(|puzzle| puzzle.puzzle_id.as_str()).collect::<Vec<_>>(), vec![second.puzzle_id.as_str()]);
    }

    #[test]
    fn selected_puzzle_cache_is_absent_without_an_active_chapter_and_loaded_empty_for_a_new_one() {
        let project = TempProjectDb::new("selected-cache-empty");
        create_project(&project.path, "Vacío").unwrap();
        let mut tab = ProjectTab::new();

        tab.open_project_path(&project.path);
        assert!(tab.selected_puzzles().is_none());
        assert_eq!(tab.selected_count(), None);

        tab.chapter_name = "Capítulo".into();
        tab.create_chapter_from_draft();
        assert!(tab.selected_puzzles().unwrap().is_empty());
        assert_eq!(tab.selected_count(), Some(0));
    }

    #[test]
    fn switching_chapters_projects_and_closing_replaces_or_clears_selected_puzzle_cache() {
        let first_project = TempProjectDb::new("selected-cache-first");
        create_project(&first_project.path, "Primero").unwrap();
        let first_chapter = create_chapter(&first_project.path, "A", None).unwrap();
        let second_chapter = create_chapter(&first_project.path, "B", None).unwrap();
        let first_puzzle = sample_puzzle();
        let mut second_puzzle = sample_puzzle();
        second_puzzle.puzzle_id = "cms-023g-second-chapter".into();
        set_puzzle_decision(
            &first_project.path,
            first_chapter.id,
            &project_puzzle(&first_puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &first_project.path,
            second_chapter.id,
            &project_puzzle(&second_puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();

        let second_project = TempProjectDb::new("selected-cache-second");
        create_project(&second_project.path, "Segundo").unwrap();
        let third_chapter = create_chapter(&second_project.path, "C", None).unwrap();
        let mut third_puzzle = sample_puzzle();
        third_puzzle.puzzle_id = "cms-023g-second-project".into();
        set_puzzle_decision(
            &second_project.path,
            third_chapter.id,
            &project_puzzle(&third_puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();

        let mut tab = ProjectTab::new();
        tab.open_project_path(&first_project.path);
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, first_puzzle.puzzle_id);
        tab.select_chapter(second_chapter.id);
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, second_puzzle.puzzle_id);
        tab.open_project_path(&second_project.path);
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, third_puzzle.puzzle_id);
        let _ = tab.update(ProjectMessage::CloseProject);
        assert!(tab.selected_puzzles().is_none());
        assert_eq!(tab.selected_count(), None);
    }

    #[test]
    fn failed_selected_puzzle_load_replaces_the_old_context_with_an_error() {
        let project = TempProjectDb::new("selected-cache-error");
        create_project(&project.path, "Errores").unwrap();
        let first = create_chapter(&project.path, "Primero", None).unwrap();
        let second = create_chapter(&project.path, "Segundo", None).unwrap();
        let puzzle = sample_puzzle();
        set_puzzle_decision(
            &project.path,
            first.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        assert_eq!(tab.selected_count(), Some(1));

        std::fs::remove_file(&project.path).unwrap();
        tab.select_chapter(second.id);

        assert!(tab.selected_puzzles().is_none());
        assert_eq!(tab.selected_count(), None);
        assert!(tab.selected_puzzles_error().is_some());
    }

    #[test]
    fn failed_refresh_after_a_successful_persistence_replaces_the_cache_with_an_error() {
        let project = TempProjectDb::new("selected-cache-refresh-error");
        create_project(&project.path, "Errores").unwrap();
        let chapter = create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzle = sample_puzzle();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        assert_eq!(tab.selected_count(), Some(0));

        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        std::fs::remove_file(&project.path).unwrap();
        tab.refresh_selected_puzzles();

        assert!(tab.selected_puzzles().is_none());
        assert_eq!(tab.selected_count(), None);
        assert!(tab.selected_puzzles_error().is_some());
    }

    #[test]
    fn selected_puzzle_cache_survives_a_failed_mutation_and_needs_no_database_read_for_render_data() {
        let project = TempProjectDb::new("selected-cache-failed-mutation");
        create_project(&project.path, "Persistencia").unwrap();
        let chapter = create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzle = sample_puzzle();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        std::fs::remove_file(&project.path).unwrap();

        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Discarded);

        assert_eq!(tab.selected_count(), Some(1));
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, puzzle.puzzle_id);
        let _ = tab.content();
    }

    #[test]
    fn reviewed_puzzle_ids_follow_the_active_project_and_chapter() {
        let first_project = TempProjectDb::new("reviewed-ids-first");
        create_project(&first_project.path, "Primero").unwrap();
        let first_chapter = create_chapter(&first_project.path, "A", None).unwrap();
        let second_chapter = create_chapter(&first_project.path, "B", None).unwrap();
        let first_puzzle = sample_puzzle();
        let mut second_puzzle = sample_puzzle();
        second_puzzle.puzzle_id = "cms-023f-second".into();
        set_puzzle_decision(
            &first_project.path,
            first_chapter.id,
            &project_puzzle(&first_puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &first_project.path,
            second_chapter.id,
            &project_puzzle(&second_puzzle),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();

        let mut tab = ProjectTab::new();
        assert_eq!(tab.reviewed_puzzle_ids_for_active_chapter().unwrap(), None);
        let empty_project = TempProjectDb::new("reviewed-ids-empty");
        create_project(&empty_project.path, "Sin capítulos").unwrap();
        tab.open_project_path(&empty_project.path);
        assert_eq!(tab.reviewed_puzzle_ids_for_active_chapter().unwrap(), None);
        tab.open_project_path(&first_project.path);
        assert_eq!(
            tab.reviewed_puzzle_ids_for_active_chapter().unwrap(),
            Some(HashSet::from([first_puzzle.puzzle_id.clone()]))
        );
        tab.select_chapter(second_chapter.id);
        assert_eq!(
            tab.reviewed_puzzle_ids_for_active_chapter().unwrap(),
            Some(HashSet::from([second_puzzle.puzzle_id.clone()]))
        );

        let second_project = TempProjectDb::new("reviewed-ids-second");
        create_project(&second_project.path, "Segundo").unwrap();
        let third_chapter = create_chapter(&second_project.path, "C", None).unwrap();
        set_puzzle_decision(
            &second_project.path,
            third_chapter.id,
            &project_puzzle(&first_puzzle),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        tab.open_project_path(&second_project.path);
        assert_eq!(
            tab.reviewed_puzzle_ids_for_active_chapter().unwrap(),
            Some(HashSet::from([first_puzzle.puzzle_id]))
        );

        std::fs::remove_file(&second_project.path).unwrap();
        assert!(tab.reviewed_puzzle_ids_for_active_chapter().is_err());
    }

    #[test]
    fn puzzle_adapter_preserves_every_persistent_snapshot_field() {
        let puzzle = Puzzle {
            puzzle_id: "adapter-id".into(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".into(),
            moves: "a1a2 h1h2".into(),
            rating: 1734,
            rating_deviation: 87,
            popularity: 42,
            nb_plays: 1_234,
            themes: "hangingPiece short".into(),
            game_url: "https://lichess.org/adapter-game".into(),
            opening: "C20".into(),
        };

        let persistent = project_puzzle(&puzzle);

        assert_eq!(persistent.puzzle_id, puzzle.puzzle_id);
        assert_eq!(persistent.fen, puzzle.fen);
        assert_eq!(persistent.moves, puzzle.moves);
        assert_eq!(persistent.rating, puzzle.rating);
        assert_eq!(persistent.rating_deviation, puzzle.rating_deviation);
        assert_eq!(persistent.popularity, puzzle.popularity);
        assert_eq!(persistent.nb_plays, puzzle.nb_plays);
        assert_eq!(persistent.themes, puzzle.themes);
        assert_eq!(persistent.game_url, puzzle.game_url);
        assert_eq!(persistent.opening, puzzle.opening);
    }

    #[test]
    fn puzzle_review_context_requires_an_open_project_and_active_chapter() {
        let puzzle = sample_puzzle();
        let mut tab = ProjectTab::new();
        tab.refresh_puzzle_review(Some(&puzzle));
        assert!(tab.review_view().is_none());

        let project = TempProjectDb::new("review-context");
        create_project(&project.path, "Contexto").unwrap();
        tab.open_project_path(&project.path);
        tab.refresh_puzzle_review(Some(&puzzle));
        assert!(tab.review_view().is_none());

        create_chapter(&project.path, "Capítulo", None).unwrap();
        tab.open_project_path(&project.path);
        tab.refresh_puzzle_review(Some(&puzzle));
        assert_eq!(tab.review_view().unwrap().decision, None);
    }

    #[test]
    fn puzzle_review_actions_persist_and_refresh_the_cached_decision() {
        let project = TempProjectDb::new("review-actions");
        create_project(&project.path, "Acciones").unwrap();
        let chapter = create_chapter(&project.path, "Pieza colgante", None).unwrap();
        let puzzle = sample_puzzle();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.refresh_puzzle_review(Some(&puzzle));

        assert_eq!(tab.review_view().unwrap().decision, None);
        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Selected);
        assert_eq!(tab.review_view().unwrap().decision, Some(ProjectPuzzleDecision::Selected));
        assert_eq!(tab.selected_count(), Some(1));
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, puzzle.puzzle_id);
        assert_eq!(chess_material_studio::project::get_puzzle_decision(&project.path, chapter.id, &puzzle.puzzle_id).unwrap(), Some(ProjectPuzzleDecision::Selected));
        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Discarded);
        assert_eq!(tab.review_view().unwrap().decision, Some(ProjectPuzzleDecision::Discarded));
        assert_eq!(tab.selected_count(), Some(0));
        assert!(tab.selected_puzzles().unwrap().is_empty());
        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Selected);
        assert_eq!(tab.selected_count(), Some(1));
        assert_eq!(tab.selected_puzzles().unwrap()[0].puzzle_id, puzzle.puzzle_id);
        tab.clear_puzzle_review(&puzzle);
        assert_eq!(tab.review_view().unwrap().decision, None);
        assert_eq!(tab.selected_count(), Some(0));
        assert!(tab.selected_puzzles().unwrap().is_empty());
        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Discarded);
        assert_eq!(tab.selected_count(), Some(0));
        tab.clear_puzzle_review(&puzzle);
        assert_eq!(tab.review_view().unwrap().decision, None);
        assert_eq!(tab.selected_count(), Some(0));
        assert!(tab.selected_puzzles().unwrap().is_empty());
        assert_eq!(chess_material_studio::project::get_puzzle_decision(&project.path, chapter.id, &puzzle.puzzle_id).unwrap(), None);
    }

    #[test]
    fn switching_chapters_and_puzzles_refreshes_the_review_cache() {
        let project = TempProjectDb::new("review-switching");
        create_project(&project.path, "Cambios").unwrap();
        let first = create_chapter(&project.path, "Primero", None).unwrap();
        let second = create_chapter(&project.path, "Segundo", None).unwrap();
        let first_puzzle = sample_puzzle();
        let mut second_puzzle = sample_puzzle();
        second_puzzle.puzzle_id = "cms-023e-second".into();
        set_puzzle_decision(&project.path, first.id, &project_puzzle(&first_puzzle), ProjectPuzzleDecision::Selected).unwrap();
        set_puzzle_decision(&project.path, second.id, &project_puzzle(&first_puzzle), ProjectPuzzleDecision::Discarded).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.refresh_puzzle_review(Some(&first_puzzle));
        assert_eq!(tab.review_view().unwrap().decision, Some(ProjectPuzzleDecision::Selected));
        tab.select_chapter(second.id);
        tab.refresh_puzzle_review(Some(&first_puzzle));
        assert_eq!(tab.review_view().unwrap().decision, Some(ProjectPuzzleDecision::Discarded));
        tab.refresh_puzzle_review(Some(&second_puzzle));
        assert_eq!(tab.review_view().unwrap().decision, None);
    }

    #[test]
    fn duplicate_selection_keeps_the_existing_review_and_names_the_selected_chapter() {
        let project = TempProjectDb::new("review-duplicate");
        create_project(&project.path, "Duplicados").unwrap();
        let first = create_chapter(&project.path, "Ataque doble", None).unwrap();
        let second = create_chapter(&project.path, "Pieza colgante", None).unwrap();
        let puzzle = sample_puzzle();
        set_puzzle_decision(&project.path, first.id, &project_puzzle(&puzzle), ProjectPuzzleDecision::Selected).unwrap();
        set_puzzle_decision(&project.path, second.id, &project_puzzle(&puzzle), ProjectPuzzleDecision::Discarded).unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.select_chapter(second.id);
        tab.refresh_puzzle_review(Some(&puzzle));
        tab.set_puzzle_review(&puzzle, ProjectPuzzleDecision::Selected);

        let view = tab.review_view().unwrap();
        assert_eq!(view.decision, Some(ProjectPuzzleDecision::Discarded));
        assert!(view.status.contains("Ataque doble"));
        assert_eq!(chess_material_studio::project::get_puzzle_decision(&project.path, second.id, &puzzle.puzzle_id).unwrap(), Some(ProjectPuzzleDecision::Discarded));
    }

    #[test]
    fn closing_a_project_clears_the_review_cache() {
        let project = TempProjectDb::new("review-close");
        create_project(&project.path, "Cerrar").unwrap();
        create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzle = sample_puzzle();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);
        tab.refresh_puzzle_review(Some(&puzzle));
        assert!(tab.review_view().is_some());
        let _ = tab.update(ProjectMessage::CloseProject);
        assert!(tab.review_view().is_none());
    }

    #[test]
    fn opening_another_project_invalidates_the_previous_review_cache() {
        let first_project = TempProjectDb::new("review-project-switch-first");
        create_project(&first_project.path, "Primero").unwrap();
        let first_chapter = create_chapter(&first_project.path, "Capítulo primero", None).unwrap();
        let second_project = TempProjectDb::new("review-project-switch-second");
        create_project(&second_project.path, "Segundo").unwrap();
        let second_chapter = create_chapter(&second_project.path, "Capítulo segundo", None).unwrap();
        let puzzle = sample_puzzle();
        set_puzzle_decision(
            &first_project.path,
            first_chapter.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &second_project.path,
            second_chapter.id,
            &project_puzzle(&puzzle),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&first_project.path);
        tab.refresh_puzzle_review(Some(&puzzle));
        assert_eq!(
            tab.review_view().unwrap().decision,
            Some(ProjectPuzzleDecision::Selected)
        );
        assert_eq!(tab.cached_review_puzzle_id(), Some(puzzle.puzzle_id.as_str()));

        tab.open_project_path(&second_project.path);

        let active = tab.active_project.as_ref().unwrap();
        assert_eq!(active.path, second_project.path);
        assert_eq!(active.active_chapter_id, Some(second_chapter.id));
        assert!(tab.review_view().is_none());
        assert_eq!(tab.cached_review_puzzle_id(), None);

        tab.refresh_puzzle_review(Some(&puzzle));

        let view = tab.review_view().unwrap();
        assert_eq!(view.chapter_name, "Capítulo segundo");
        assert_eq!(view.decision, Some(ProjectPuzzleDecision::Discarded));
        assert_eq!(tab.cached_review_puzzle_id(), Some(puzzle.puzzle_id.as_str()));
    }

    #[test]
    fn project_tab_translation_keys_exist_in_every_language() {
        const PROJECT_KEYS: [&str; 48] = [
            "project",
            "new_project",
            "project_name",
            "project_file",
            "choose_project_file",
            "project_file_not_selected",
            "create_project",
            "open_project",
            "close_project",
            "chapters",
            "select_chapters_for_export",
            "selected_chapters_for_export",
            "active_chapter",
            "new_chapter",
            "chapter_name",
            "target_puzzle_count_optional",
            "create_chapter",
            "selected",
            "or",
            "project_name_required",
            "project_file_required",
            "project_file_extension_required",
            "chapter_name_required",
            "no_project_open",
            "no_active_chapter",
            "selected_puzzles",
            "no_selected_puzzles",
            "selected_puzzles_error",
            "load_selected_puzzles",
            "export_chapter_to_pgn",
            "chapter_pgn_exported",
            "chapter_pgn_export_failed",
            "export_project_to_pgn",
            "project_pgn_exported",
            "project_pgn_export_failed",
            "no_selected_puzzles_in_project",
            "export_chapter_to_pdf",
            "chapter_pdf_exported",
            "chapter_pdf_export_failed",
            "review_status",
            "unreviewed",
            "discarded",
            "select_puzzle",
            "discard_puzzle",
            "clear_review",
            "already_selected_in_chapter",
            "review_error",
            "review_unavailable",
        ];

        for language in lang::Language::ALL {
            for key in PROJECT_KEYS {
                assert!(!lang::tr(&language, key).is_empty(), "missing {key}");
            }
        }
    }
}
