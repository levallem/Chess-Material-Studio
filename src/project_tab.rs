use iced::widget::{Button, Column, Container, Scrollable, Text, TextInput};
use iced::{Alignment, Element, Length, Task, alignment};
use iced_aw::TabLabel;
use rfd::AsyncFileDialog;
use std::path::{Path, PathBuf};

use chess_material_studio::project::{
    ProjectChapter, ProjectMetadata, create_chapter, create_project, list_chapters,
    list_selected_puzzles_for_chapter, open_project,
};

use crate::lang;
use crate::styles::btn_style_simple;
use crate::{Message, Tab};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveProject {
    path: PathBuf,
    metadata: ProjectMetadata,
    chapters: Vec<ProjectChapter>,
    active_chapter_id: Option<i32>,
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
}

pub struct ProjectTab {
    pub lang: lang::Language,
    active_project: Option<ActiveProject>,
    project_name: String,
    new_project_path: Option<PathBuf>,
    chapter_name: String,
    target_puzzle_count: String,
    status: String,
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
                self.chapter_name.clear();
                self.target_puzzle_count.clear();
                self.status.clear();
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
        }
    }

    fn active_chapter(&self) -> Option<&ProjectChapter> {
        let active_project = self.active_project.as_ref()?;
        let chapter_id = active_project.active_chapter_id?;
        active_project
            .chapters
            .iter()
            .find(|chapter| chapter.id == chapter_id)
    }

    fn selected_count(&self) -> Result<Option<usize>, String> {
        let Some(active_project) = self.active_project.as_ref() else {
            return Ok(None);
        };
        let Some(chapter_id) = active_project.active_chapter_id else {
            return Ok(None);
        };
        list_selected_puzzles_for_chapter(&active_project.path, chapter_id)
            .map(|puzzles| Some(puzzles.len()))
    }
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

        let progress = match (self.active_chapter(), self.selected_count()) {
            (Some(chapter), Ok(Some(selected))) => match chapter.target_puzzle_count {
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
            (_, Err(error)) => error,
            _ => lang::tr(&self.lang, "no_active_chapter"),
        };

        Column::new()
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
            .push(Text::new(lang::tr(&self.lang, "chapters")))
            .push(chapters)
            .push(Text::new(progress))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_material_studio::models::Puzzle;
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
        assert_eq!(tab.selected_count().unwrap(), None);
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
    fn selected_count_is_derived_from_persisted_reviews() {
        let project = TempProjectDb::new("selected-count");
        create_project(&project.path, "Progreso").unwrap();
        let chapter = create_chapter(&project.path, "Pieza colgante", Some(20)).unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &sample_puzzle(),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        let mut tab = ProjectTab::new();
        tab.open_project_path(&project.path);

        assert_eq!(tab.selected_count().unwrap(), Some(1));
    }

    #[test]
    fn project_tab_translation_keys_exist_in_every_language() {
        const PROJECT_KEYS: [&str; 23] = [
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
        ];

        for language in lang::Language::ALL {
            for key in PROJECT_KEYS {
                assert!(!lang::tr(&language, key).is_empty(), "missing {key}");
            }
        }
    }
}
