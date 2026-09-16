use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Scrollable, Text, TextInput, column, row,
};
use iced::{Alignment, Element, Length, Task, Theme, alignment};

use iced_aw::TabLabel;

use rfd::AsyncFileDialog;

use crate::styles::btn_style_simple;
use crate::{Message, Tab, config, lang, lang::PickListWrapper, styles};

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    CheckPlaySound(bool),
    CheckAutoLoad(bool),
    CheckFlipBoard(bool),
    CheckShowCoords(bool),
    SelectPieceTheme(styles::PieceTheme),
    SelectBoardTheme(styles::BoardTheme),
    SelectInterfaceTheme(styles::InterfaceTheme),
    SelectLanguage(PickListWrapper<lang::Language>),
    ChangePDFExportPgs(String),
    ChangePuzzleDbLocation(String),
    ChangeSearchResultLimit(String),
    ChangeEnginePath(String),
    SearchEnginePressed,
    SelectPuzzleSqlitePressed,
    PuzzleSqliteFileChosen(Option<std::path::PathBuf>),
    UseCsvPuzzles,
    ChangePressed,
}

fn with_puzzle_sqlite_location(
    mut config: config::OfflinePuzzlesConfig,
    puzzle_sqlite_location: Option<String>,
) -> config::OfflinePuzzlesConfig {
    config.puzzle_sqlite_location = puzzle_sqlite_location;
    config
}

fn puzzle_sqlite_config_for_file_choice(
    persisted_config: config::OfflinePuzzlesConfig,
    selected_path: Option<&std::path::Path>,
) -> Result<Option<config::OfflinePuzzlesConfig>, String> {
    let Some(path) = selected_path else {
        return Ok(None);
    };

    chess_material_studio::puzzle_search::validate_puzzle_sqlite_db(path)?;
    Ok(Some(with_puzzle_sqlite_location(
        persisted_config,
        Some(path.display().to_string()),
    )))
}

pub struct SettingsTab {
    pub engine_path: String,
    pub window_width: f32,
    pub window_height: f32,
    windowed_size: iced::Size,
    pub maximized: bool,
    pub piece_theme: styles::PieceTheme,
    pub board_theme: styles::BoardTheme,
    pub interface_theme: styles::InterfaceTheme,
    pub lang: PickListWrapper<lang::Language>,
    pub export_pgs: String,
    play_sound: bool,
    auto_load_next: bool,
    pub flip_board: bool,
    pub show_coordinates: bool,

    puzzle_db_location_value: String,
    search_results_limit_value: String,

    settings_status: String,
    pub saved_configs: config::OfflinePuzzlesConfig,
}

impl SettingsTab {
    pub fn new() -> Self {
        Self::from_config(&config::SETTINGS, config::load_config())
    }

    fn from_config(
        config: &config::OfflinePuzzlesConfig,
        saved_configs: config::OfflinePuzzlesConfig,
    ) -> Self {
        SettingsTab {
            engine_path: config.engine_path.clone().unwrap_or_default(),
            window_width: config.window_width,
            window_height: config.window_height,
            windowed_size: iced::Size::new(config.window_width, config.window_height),
            maximized: config.maximized,
            piece_theme: config.piece_theme,
            board_theme: config.board_theme,
            interface_theme: config.interface_theme,
            lang: PickListWrapper::new_lang(config.lang, config.lang),
            export_pgs: config.export_pgs.to_string(),
            play_sound: config.play_sound,
            auto_load_next: config.auto_load_next,
            flip_board: config.flip_board,
            show_coordinates: config.show_coordinates,
            puzzle_db_location_value: String::from(&config.puzzle_db_location),
            search_results_limit_value: config.search_results_limit.to_string(),
            settings_status: String::new(),
            saved_configs,
        }
    }

    pub fn update(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::SelectPieceTheme(value) => {
                self.piece_theme = value;
                Task::perform(
                    SettingsTab::send_changes(
                        self.play_sound,
                        self.auto_load_next,
                        self.flip_board,
                        self.show_coordinates,
                        self.piece_theme,
                        self.board_theme,
                        self.engine_path.clone(),
                        self.lang.lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::SelectBoardTheme(value) => {
                self.board_theme = value;
                let (play_sound, auto_load, flip, coords, pieces, theme, engine, lang) =
                    self.settings_change_values();
                Task::perform(
                    SettingsTab::send_changes(
                        play_sound, auto_load, flip, coords, pieces, theme, engine, lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::SelectInterfaceTheme(value) => {
                self.interface_theme = value;
                let mut config = config::load_config();
                config.interface_theme = value;
                match config::persist_config(&config) {
                    Ok(()) => self.saved_configs = config,
                    Err(status_key) => self.settings_status = lang::tr(&self.lang.lang, status_key),
                }
                Task::none()
            }
            SettingsMessage::SelectLanguage(value) => {
                self.lang = value;
                self.lang.lang = self.lang.item;
                let (play_sound, auto_load, flip, coords, pieces, theme, engine, lang) =
                    self.settings_change_values();
                Task::perform(
                    SettingsTab::send_changes(
                        play_sound, auto_load, flip, coords, pieces, theme, engine, lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::ChangePuzzleDbLocation(value) => {
                self.puzzle_db_location_value = value;
                Task::none()
            }
            SettingsMessage::ChangeEnginePath(value) => {
                self.engine_path = value;
                Task::perform(
                    SettingsTab::send_changes(
                        self.play_sound,
                        self.auto_load_next,
                        self.flip_board,
                        self.show_coordinates,
                        self.piece_theme,
                        self.board_theme,
                        self.engine_path.clone(),
                        self.lang.lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::SearchEnginePressed => {
                Task::perform(Self::open_engine_exe(), Message::EngineFileChosen)
            }
            SettingsMessage::SelectPuzzleSqlitePressed => {
                Task::perform(Self::open_puzzle_sqlite_db(), |path| {
                    Message::Settings(SettingsMessage::PuzzleSqliteFileChosen(path))
                })
            }
            SettingsMessage::PuzzleSqliteFileChosen(Some(path)) => {
                match puzzle_sqlite_config_for_file_choice(config::load_config(), Some(&path)) {
                    Ok(Some(config)) => match self.persist_puzzle_source_config(config) {
                        Ok(()) => return Task::done(Message::PuzzleSqliteSourceSelected),
                        Err(error) => self.settings_status = error,
                    },
                    Ok(None) => unreachable!("a selected SQLite path must produce a configuration"),
                    Err(error) => {
                        self.settings_status = format!(
                            "{}: {}",
                            lang::tr(&self.lang.lang, "puzzle_sqlite_invalid"),
                            error
                        );
                    }
                }
                Task::none()
            }
            SettingsMessage::PuzzleSqliteFileChosen(None) => {
                match puzzle_sqlite_config_for_file_choice(config::load_config(), None) {
                    Ok(None) => Task::none(),
                    Ok(Some(_)) => {
                        unreachable!("a cancelled SQLite choice must not produce a configuration")
                    }
                    Err(_) => unreachable!("a cancelled SQLite choice cannot fail validation"),
                }
            }
            SettingsMessage::UseCsvPuzzles => {
                self.save_puzzle_sqlite_location(None);
                Task::none()
            }
            SettingsMessage::ChangeSearchResultLimit(value) => {
                if value.is_empty() {
                    self.search_results_limit_value = String::from("0");
                } else if let Ok(new_val) = value.parse::<usize>() {
                    self.search_results_limit_value = new_val.to_string();
                    self.settings_status = String::from("");
                }
                Task::none()
            }
            SettingsMessage::CheckPlaySound(value) => {
                self.play_sound = value;
                Task::perform(
                    SettingsTab::send_changes(
                        self.play_sound,
                        self.auto_load_next,
                        self.flip_board,
                        self.show_coordinates,
                        self.piece_theme,
                        self.board_theme,
                        self.engine_path.clone(),
                        self.lang.lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::CheckAutoLoad(value) => {
                self.auto_load_next = value;
                Task::perform(
                    SettingsTab::send_changes(
                        self.play_sound,
                        self.auto_load_next,
                        self.flip_board,
                        self.show_coordinates,
                        self.piece_theme,
                        self.board_theme,
                        self.engine_path.clone(),
                        self.lang.lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::CheckFlipBoard(value) => {
                self.flip_board = value;
                Task::perform(
                    SettingsTab::send_changes(
                        self.play_sound,
                        self.auto_load_next,
                        self.flip_board,
                        self.show_coordinates,
                        self.piece_theme,
                        self.board_theme,
                        self.engine_path.clone(),
                        self.lang.lang,
                    ),
                    Message::ChangeSettings,
                )
            }
            SettingsMessage::CheckShowCoords(value) => {
                self.show_coordinates = value;
                Task::none()
            }
            SettingsMessage::ChangePDFExportPgs(value) => {
                if value.parse::<i32>().is_ok() {
                    self.export_pgs = value;
                } else if value.is_empty() {
                    self.export_pgs = String::from("0");
                }
                Task::none()
            }
            SettingsMessage::ChangePressed => {
                let config = self.current_config();
                match config::persist_config(&config) {
                    Ok(()) => {
                        self.saved_configs = config;
                        self.settings_status = lang::tr(&self.lang.lang, "settings_saved");
                    }
                    Err(status_key) => self.settings_status = lang::tr(&self.lang.lang, status_key),
                }
                Task::none()
            }
        }
    }

    async fn open_engine_exe() -> Option<String> {
        let engine_exe = AsyncFileDialog::new().pick_file().await;
        engine_exe.map(|engine_path| engine_path.path().display().to_string())
    }

    async fn open_puzzle_sqlite_db() -> Option<std::path::PathBuf> {
        AsyncFileDialog::new()
            .pick_file()
            .await
            .map(|file| file.path().to_path_buf())
    }

    fn current_config(&self) -> config::OfflinePuzzlesConfig {
        self.current_config_from_base(config::load_config())
    }

    fn current_config_from_base(
        &self,
        mut config: config::OfflinePuzzlesConfig,
    ) -> config::OfflinePuzzlesConfig {
        let engine_path = (!self.engine_path.is_empty()).then(|| self.engine_path.clone());
        config.engine_path = engine_path;
        config.window_width = self.windowed_size.width;
        config.window_height = self.windowed_size.height;
        config.maximized = self.maximized;
        config.puzzle_db_location = self.puzzle_db_location_value.clone();
        config.piece_theme = self.piece_theme;
        config.search_results_limit = self.search_results_limit_value.parse().unwrap();
        config.play_sound = self.play_sound;
        config.auto_load_next = self.auto_load_next;
        config.flip_board = self.flip_board;
        config.show_coordinates = self.show_coordinates;
        config.board_theme = self.board_theme;
        config.interface_theme = self.interface_theme;
        config.lang = self.lang.lang;
        config.export_pgs = self.export_pgs.parse().unwrap();
        config
    }

    fn save_puzzle_sqlite_location(&mut self, puzzle_sqlite_location: Option<String>) {
        let config = with_puzzle_sqlite_location(config::load_config(), puzzle_sqlite_location);
        if let Err(error) = self.persist_puzzle_source_config(config) {
            self.settings_status = error;
        }
    }

    fn persist_puzzle_source_config(
        &mut self,
        config: config::OfflinePuzzlesConfig,
    ) -> Result<(), String> {
        match config::persist_config(&config) {
            Ok(()) => {
                let status_key = if config.puzzle_sqlite_location.is_some() {
                    "puzzle_sqlite_selected"
                } else {
                    "csv_puzzles_selected"
                };
                self.saved_configs = config;
                self.settings_status = lang::tr(&self.lang.lang, status_key);
                Ok(())
            }
            Err(status_key) => Err(lang::tr(&self.lang.lang, status_key)),
        }
    }

    pub fn status(&self) -> &str {
        &self.settings_status
    }

    pub fn is_using_sqlite_puzzles(&self) -> bool {
        self.saved_configs.puzzle_sqlite_location.is_some()
    }

    pub(crate) fn save_window_size_to_path(
        &self,
        path: &std::path::Path,
    ) -> Result<(), &'static str> {
        let mut config = config::load_config_from_path(path);
        config.window_width = self.windowed_size.width;
        config.window_height = self.windowed_size.height;
        config.maximized = self.maximized;
        config::persist_config_to_path(&config, path)
    }

    pub(crate) fn record_window_resize(&mut self, size: iced::Size, maximized: bool) {
        self.window_width = size.width;
        self.window_height = size.height;
        self.maximized = maximized;
        if !maximized {
            self.windowed_size = size;
        }
    }

    fn settings_change_values(
        &self,
    ) -> (
        bool,
        bool,
        bool,
        bool,
        styles::PieceTheme,
        styles::BoardTheme,
        String,
        lang::Language,
    ) {
        (
            self.play_sound,
            self.auto_load_next,
            self.flip_board,
            self.show_coordinates,
            self.piece_theme,
            self.board_theme,
            self.engine_path.clone(),
            self.lang.lang,
        )
    }

    fn change_payload(
        mut config: config::OfflinePuzzlesConfig,
        play_sound: bool,
        auto_load: bool,
        flip: bool,
        coords: bool,
        pieces: styles::PieceTheme,
        theme: styles::BoardTheme,
        engine: String,
        lang: lang::Language,
    ) -> config::OfflinePuzzlesConfig {
        let engine = if engine.is_empty() {
            None
        } else {
            Some(engine)
        };
        config.board_theme = theme;
        config.piece_theme = pieces;
        config.lang = lang;
        config.play_sound = play_sound;
        config.auto_load_next = auto_load;
        config.flip_board = flip;
        config.show_coordinates = coords;
        config.engine_path = engine;
        config
    }

    pub async fn send_changes(
        play_sound: bool,
        auto_load: bool,
        flip: bool,
        coords: bool,
        pieces: styles::PieceTheme,
        theme: styles::BoardTheme,
        engine: String,
        lang: lang::Language,
    ) -> Option<config::OfflinePuzzlesConfig> {
        Some(Self::change_payload(
            config::load_config(),
            play_sound,
            auto_load,
            flip,
            coords,
            pieces,
            theme,
            engine,
            lang,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::Connection;
    use diesel::sqlite::SqliteConnection;
    use diesel_migrations::MigrationHarness;

    fn settings_tab_from_config(config: config::OfflinePuzzlesConfig) -> SettingsTab {
        SettingsTab::from_config(&config, config.clone())
    }

    struct TempSettingsFile {
        directory: std::path::PathBuf,
        path: std::path::PathBuf,
    }

    impl TempSettingsFile {
        fn new(label: &str) -> Self {
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_h4_settings_tests")
                .join(format!(
                    "{label}-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("system clock should be after UNIX epoch")
                        .as_nanos(),
                ));
            std::fs::create_dir_all(&directory).expect("test directory should be created");
            Self {
                path: directory.join("settings.json"),
                directory,
            }
        }
    }

    impl Drop for TempSettingsFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn save_window_size_to_path_preserves_unrelated_config() {
        let settings_file = TempSettingsFile::new("window-size");
        let path = &settings_file.path;
        let persisted = config::OfflinePuzzlesConfig {
            engine_limit: "nodes 123".into(),
            window_width: 800.0,
            window_height: 600.0,
            ..config::OfflinePuzzlesConfig::default()
        };
        config::persist_config_to_path(&persisted, &path)
            .expect("test configuration should persist");
        let mut settings_tab = settings_tab_from_config(persisted);
        settings_tab.record_window_resize(iced::Size::new(1234.0, 567.0), false);
        settings_tab.record_window_resize(iced::Size::new(1900.0, 1000.0), true);

        settings_tab
            .save_window_size_to_path(&path)
            .expect("window size should use the safe persistence helper");

        let restored = config::load_config_from_path(&path);
        assert_eq!(restored.window_width, 1234.0);
        assert_eq!(restored.window_height, 567.0);
        assert!(restored.maximized);
        assert_eq!(restored.engine_limit, "nodes 123");
    }

    #[test]
    fn window_resize_tracks_live_size_without_overwriting_windowed_size_when_maximized() {
        let mut settings_tab = settings_tab_from_config(config::OfflinePuzzlesConfig::default());

        settings_tab.record_window_resize(iced::Size::new(1200.0, 800.0), false);
        assert_eq!(settings_tab.window_width, 1200.0);
        assert_eq!(settings_tab.window_height, 800.0);
        assert_eq!(settings_tab.windowed_size, iced::Size::new(1200.0, 800.0));
        assert!(!settings_tab.maximized);

        settings_tab.record_window_resize(iced::Size::new(1900.0, 1000.0), true);
        assert_eq!(settings_tab.window_width, 1900.0);
        assert_eq!(settings_tab.window_height, 1000.0);
        assert_eq!(settings_tab.windowed_size, iced::Size::new(1200.0, 800.0));
        assert!(settings_tab.maximized);

        settings_tab.record_window_resize(iced::Size::new(1250.0, 820.0), false);
        assert_eq!(settings_tab.window_width, 1250.0);
        assert_eq!(settings_tab.window_height, 820.0);
        assert_eq!(settings_tab.windowed_size, iced::Size::new(1250.0, 820.0));
        assert!(!settings_tab.maximized);
    }

    #[test]
    fn settings_tab_uses_configured_window_height() {
        let config = config::OfflinePuzzlesConfig {
            window_width: 1234.0,
            window_height: 567.0,
            ..config::OfflinePuzzlesConfig::default()
        };

        let settings_tab = settings_tab_from_config(config);

        assert_eq!(settings_tab.window_width, 1234.0);
        assert_eq!(settings_tab.window_height, 567.0);
    }

    #[test]
    fn board_theme_change_payload_uses_selected_theme() {
        let config = config::OfflinePuzzlesConfig {
            board_theme: styles::BoardTheme::Brown,
            ..config::OfflinePuzzlesConfig::default()
        };
        let mut settings_tab = settings_tab_from_config(config.clone());

        let _task =
            settings_tab.update(SettingsMessage::SelectBoardTheme(styles::BoardTheme::Green));
        let (play_sound, auto_load, flip, coords, pieces, theme, engine, lang) =
            settings_tab.settings_change_values();
        let payload = SettingsTab::change_payload(
            config, play_sound, auto_load, flip, coords, pieces, theme, engine, lang,
        );
        settings_tab.saved_configs = payload;

        assert_eq!(settings_tab.board_theme, styles::BoardTheme::Green);
        assert_eq!(
            settings_tab.saved_configs.board_theme,
            settings_tab.board_theme
        );
    }

    #[test]
    fn language_change_payload_preserves_selected_board_theme() {
        let config = config::OfflinePuzzlesConfig {
            board_theme: styles::BoardTheme::Purple,
            ..config::OfflinePuzzlesConfig::default()
        };
        let mut settings_tab = settings_tab_from_config(config.clone());

        let _task = settings_tab.update(SettingsMessage::SelectLanguage(
            PickListWrapper::new_lang(lang::Language::English, lang::Language::Spanish),
        ));
        let (play_sound, auto_load, flip, coords, pieces, theme, engine, lang) =
            settings_tab.settings_change_values();
        let payload = SettingsTab::change_payload(
            config, play_sound, auto_load, flip, coords, pieces, theme, engine, lang,
        );
        settings_tab.saved_configs = payload;

        assert_eq!(settings_tab.board_theme, styles::BoardTheme::Purple);
        assert_eq!(
            settings_tab.saved_configs.board_theme,
            settings_tab.board_theme
        );
    }

    #[test]
    fn current_config_merge_preserves_fresh_external_fields_and_applies_settings_values() {
        let stale_snapshot = config::OfflinePuzzlesConfig::default();
        let mut settings_tab = settings_tab_from_config(stale_snapshot);
        settings_tab.engine_path = "new-engine.exe".into();
        settings_tab.record_window_resize(iced::Size::new(1234.0, 567.0), false);
        settings_tab.record_window_resize(iced::Size::new(1900.0, 1000.0), true);
        settings_tab.puzzle_db_location_value = "new-puzzles.csv".into();
        settings_tab.piece_theme = styles::PieceTheme::Alpha;
        settings_tab.board_theme = styles::BoardTheme::Green;
        settings_tab.interface_theme = styles::InterfaceTheme::Dark;
        settings_tab.lang =
            PickListWrapper::new_lang(lang::Language::Spanish, lang::Language::Spanish);
        settings_tab.export_pgs = "75".into();
        settings_tab.search_results_limit_value = "250".into();
        settings_tab.play_sound = false;
        settings_tab.auto_load_next = false;
        settings_tab.flip_board = true;
        settings_tab.show_coordinates = true;

        let fresh_filters = config::OfflinePuzzlesConfig {
            engine_limit: "nodes 99".into(),
            last_min_rating: 1500,
            last_max_rating: 2500,
            last_min_popularity: 33,
            last_theme: crate::search_tab::TacticalThemes::Fork,
            last_opening: crate::openings::Openings::Sicilian,
            last_variation: crate::openings::Variation {
                name: std::borrow::Cow::Borrowed("Sicilian_Defense_Najdorf_Variation"),
                family: crate::openings::Openings::Sicilian,
            },
            last_opening_side: Some(crate::search_tab::OpeningSide::Black),
            puzzle_sqlite_location: Some("fresh-puzzles.sqlite".into()),
            ..config::OfflinePuzzlesConfig::default()
        };

        let merged = settings_tab.current_config_from_base(fresh_filters);

        assert_eq!(merged.last_min_rating, 1500);
        assert_eq!(merged.last_max_rating, 2500);
        assert_eq!(merged.last_min_popularity, 33);
        assert_eq!(merged.last_theme, crate::search_tab::TacticalThemes::Fork);
        assert_eq!(merged.last_opening, crate::openings::Openings::Sicilian);
        assert_eq!(
            merged.last_variation,
            crate::openings::Variation {
                name: std::borrow::Cow::Borrowed("Sicilian_Defense_Najdorf_Variation"),
                family: crate::openings::Openings::Sicilian,
            }
        );
        assert_eq!(
            merged.last_opening_side,
            Some(crate::search_tab::OpeningSide::Black)
        );
        assert_eq!(merged.engine_limit, "nodes 99");
        assert_eq!(
            merged.puzzle_sqlite_location.as_deref(),
            Some("fresh-puzzles.sqlite")
        );
        assert_eq!(merged.engine_path.as_deref(), Some("new-engine.exe"));
        assert_eq!(merged.window_width, 1234.0);
        assert_eq!(merged.window_height, 567.0);
        assert!(merged.maximized);
        assert_eq!(merged.puzzle_db_location, "new-puzzles.csv");
        assert_eq!(merged.piece_theme, styles::PieceTheme::Alpha);
        assert_eq!(merged.board_theme, styles::BoardTheme::Green);
        assert_eq!(merged.interface_theme, styles::InterfaceTheme::Dark);
        assert_eq!(merged.lang, lang::Language::Spanish);
        assert_eq!(merged.search_results_limit, 250);
        assert_eq!(merged.export_pgs, 75);
        assert!(!merged.play_sound);
        assert!(!merged.auto_load_next);
        assert!(merged.flip_board);
        assert!(merged.show_coordinates);
    }

    struct TempPuzzleDb {
        path: std::path::PathBuf,
        directory: Option<std::path::PathBuf>,
    }

    impl TempPuzzleDb {
        fn new(label: &str) -> Self {
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_022b_tests");
            std::fs::create_dir_all(&directory).expect("test directory should be created");
            let path = directory.join(format!(
                "{label}-{}-{}.sqlite",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after UNIX epoch")
                    .as_nanos(),
            ));
            Self {
                path,
                directory: None,
            }
        }

        fn protected_ocp_db(label: &str) -> Self {
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_022b_tests")
                .join(format!("{label}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("test directory should be created");
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

    fn create_valid_puzzle_db(path: &std::path::Path) {
        let mut connection =
            SqliteConnection::establish(path.to_str().expect("test path should be UTF-8"))
                .expect("test database should open");
        connection
            .run_pending_migrations(chess_material_studio::puzzle_import::MIGRATIONS)
            .expect("test database schema should migrate");
    }

    #[test]
    fn puzzle_sqlite_location_update_preserves_unrelated_persisted_config() {
        let mut persisted = config::OfflinePuzzlesConfig::default();
        persisted.engine_limit = "nodes 123".into();
        persisted.puzzle_db_location = "custom-puzzles.csv".into();
        persisted.last_min_rating = 1234;
        persisted.last_max_rating = 2345;

        let updated = with_puzzle_sqlite_location(persisted, Some("puzzles.sqlite".into()));

        assert_eq!(
            updated.puzzle_sqlite_location.as_deref(),
            Some("puzzles.sqlite")
        );
        assert_eq!(updated.engine_limit, "nodes 123");
        assert_eq!(updated.puzzle_db_location, "custom-puzzles.csv");
        assert_eq!(updated.last_min_rating, 1234);
        assert_eq!(updated.last_max_rating, 2345);
    }

    #[test]
    fn sqlite_file_choice_updates_source_config_without_changing_other_settings() {
        let database = TempPuzzleDb::new("valid-selection");
        create_valid_puzzle_db(database.path());
        let mut persisted = config::OfflinePuzzlesConfig::default();
        persisted.engine_limit = "nodes 123".into();
        persisted.puzzle_db_location = "custom-puzzles.csv".into();

        let selected = puzzle_sqlite_config_for_file_choice(persisted, Some(database.path()))
            .expect("a valid puzzle SQLite database should be accepted")
            .expect("a selected database should update the configuration");

        assert_eq!(
            selected.puzzle_sqlite_location.as_deref(),
            database.path().to_str()
        );
        assert!(config::puzzle_source_exists(&selected));
        assert_eq!(selected.engine_limit, "nodes 123");
        assert_eq!(selected.puzzle_db_location, "custom-puzzles.csv");
    }

    #[test]
    fn cancelling_sqlite_file_choice_leaves_source_config_unchanged() {
        let persisted = config::OfflinePuzzlesConfig {
            puzzle_sqlite_location: Some("existing-puzzles.sqlite".into()),
            ..config::OfflinePuzzlesConfig::default()
        };

        assert!(
            puzzle_sqlite_config_for_file_choice(persisted, None)
                .expect("cancelling a picker should not fail")
                .is_none()
        );
    }

    #[test]
    fn invalid_sqlite_file_choice_does_not_produce_a_replacement_config() {
        let persisted = config::OfflinePuzzlesConfig {
            puzzle_sqlite_location: Some("existing-puzzles.sqlite".into()),
            ..config::OfflinePuzzlesConfig::default()
        };
        let invalid = TempPuzzleDb::new("invalid-selection");
        std::fs::write(invalid.path(), b"not a SQLite database")
            .expect("invalid test database should be created");

        assert!(puzzle_sqlite_config_for_file_choice(persisted, Some(invalid.path())).is_err());
    }

    #[test]
    fn protected_ocp_db_file_choice_does_not_produce_a_replacement_config() {
        let persisted = config::OfflinePuzzlesConfig::default();
        let protected = TempPuzzleDb::protected_ocp_db("protected-selection");
        create_valid_puzzle_db(protected.path());

        assert!(puzzle_sqlite_config_for_file_choice(persisted, Some(protected.path())).is_err());
    }
}

impl Tab for SettingsTab {
    type Message = Message;

    fn title(&self) -> String {
        lang::tr(&self.lang.lang, "settings")
    }

    fn tab_label(&self) -> TabLabel {
        TabLabel::Text(self.title())
    }

    fn content(&self) -> Element<'_, Message> {
        let col_settings = column![
            row![
                Text::new(lang::tr(&self.lang.lang, "piece_theme")),
                PickList::new(
                    &styles::PieceTheme::ALL[..],
                    Some(self.piece_theme),
                    SettingsMessage::SelectPieceTheme
                )
                .style(styles::pick_list_style)
                .menu_style(styles::menu_style)
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "board_theme")),
                PickList::new(
                    &styles::BoardTheme::ALL[..],
                    Some(self.board_theme),
                    SettingsMessage::SelectBoardTheme
                )
                .style(styles::pick_list_style)
                .menu_style(styles::menu_style)
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "interface_appearance")),
                PickList::new(
                    &styles::InterfaceTheme::ALL[..],
                    Some(self.interface_theme),
                    SettingsMessage::SelectInterfaceTheme
                )
                .style(styles::pick_list_style)
                .menu_style(styles::menu_style)
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "language")),
                PickList::new(
                    PickListWrapper::get_langs(self.lang.lang),
                    Some(self.lang.clone()),
                    SettingsMessage::SelectLanguage
                )
                .style(styles::pick_list_style)
                .menu_style(styles::menu_style)
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "play_sound")),
                Checkbox::new(self.play_sound)
                    .on_toggle(SettingsMessage::CheckPlaySound)
                    .size(20)
                    .style(styles::checkbox_style),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "auto_load")),
                Checkbox::new(self.auto_load_next)
                    .on_toggle(SettingsMessage::CheckAutoLoad)
                    .size(20)
                    .style(styles::checkbox_style),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "flip_board")),
                Checkbox::new(self.flip_board)
                    .on_toggle(SettingsMessage::CheckFlipBoard)
                    .size(20)
                    .style(styles::checkbox_style),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "show_coords")),
                Checkbox::new(self.show_coordinates)
                    .on_toggle(SettingsMessage::CheckShowCoords)
                    .size(20)
                    .style(styles::checkbox_style),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "pdf_number_of_pages")),
                TextInput::new(&self.export_pgs, &self.export_pgs,)
                    .on_input(SettingsMessage::ChangePDFExportPgs)
                    .width(60),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "get_first_puzzles1")),
                TextInput::new(
                    &self.search_results_limit_value,
                    &self.search_results_limit_value,
                )
                .on_input(SettingsMessage::ChangeSearchResultLimit)
                .width(80),
                Text::new(lang::tr(&self.lang.lang, "get_first_puzzles2"))
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            Text::new(lang::tr(&self.lang.lang, "engine_path")),
            row![
                TextInput::new(&self.engine_path, &self.engine_path,)
                    .on_input(SettingsMessage::ChangeEnginePath)
                    .width(200),
                Button::new(Text::new(lang::tr(&self.lang.lang, "select")))
                    .on_press(SettingsMessage::SearchEnginePressed)
                    .style(btn_style_simple),
            ],
            Text::new(lang::tr(&self.lang.lang, "puzzle_sqlite_db")),
            row![
                Button::new(Text::new(lang::tr(&self.lang.lang, "select")))
                    .on_press(SettingsMessage::SelectPuzzleSqlitePressed)
                    .style(btn_style_simple),
                Button::new(Text::new(lang::tr(&self.lang.lang, "use_csv_puzzles")))
                    .on_press(SettingsMessage::UseCsvPuzzles)
                    .style(btn_style_simple),
            ],
            Button::new(Text::new(lang::tr(&self.lang.lang, "save")))
                .padding(5)
                .on_press(SettingsMessage::ChangePressed)
                .style(btn_style_simple),
            Text::new(&self.settings_status).align_y(alignment::Vertical::Bottom),
        ]
        .spacing(10)
        .align_x(Alignment::Center);
        let content: Element<SettingsMessage, Theme, iced::Renderer> =
            Container::new(Scrollable::new(
                Column::new()
                    .padding([0, 30])
                    .spacing(10)
                    .push(col_settings),
            ))
            .align_x(alignment::Horizontal::Center)
            .height(Length::Fill)
            .width(Length::Fill)
            .into();

        content.map(Message::Settings)
    }
}
