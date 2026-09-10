use iced::widget::{Button, Container, Checkbox, column, Column, Text, TextInput, row, PickList, Scrollable};
use iced::{alignment, Alignment, Element, Length, Task, Theme};

use iced_aw::TabLabel;

use rfd::AsyncFileDialog;

use crate::styles::btn_style_simple;
use crate::{Message, Tab, config, styles, lang, lang::PickListWrapper};
use crate::config::SETTINGS_FILE;

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
    ChangePressed
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
                Task::perform(SettingsTab::send_changes(self.play_sound, self.auto_load_next, self.flip_board, self.show_coordinates, self.piece_theme, self.board_theme, self.engine_path.clone(), self.lang.lang), Message::ChangeSettings)
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
                match Self::persist_config(&config) {
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
                Task::perform(SettingsTab::send_changes(self.play_sound, self.auto_load_next, self.flip_board, self.show_coordinates, self.piece_theme, self.board_theme, self.engine_path.clone(), self.lang.lang), Message::ChangeSettings)
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
                match puzzle_sqlite_config_for_file_choice(
                    config::load_config(),
                    None,
                ) {
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
                Task::perform(SettingsTab::send_changes(self.play_sound, self.auto_load_next, self.flip_board, self.show_coordinates, self.piece_theme, self.board_theme, self.engine_path.clone(), self.lang.lang), Message::ChangeSettings)
            }
            SettingsMessage::CheckAutoLoad(value) => {
                self.auto_load_next = value;
                Task::perform(SettingsTab::send_changes(self.play_sound, self.auto_load_next, self.flip_board, self.show_coordinates, self.piece_theme, self.board_theme, self.engine_path.clone(), self.lang.lang), Message::ChangeSettings)
            }
            SettingsMessage::CheckFlipBoard(value) => {
                self.flip_board = value;
                Task::perform(SettingsTab::send_changes(self.play_sound, self.auto_load_next, self.flip_board, self.show_coordinates, self.piece_theme, self.board_theme, self.engine_path.clone(), self.lang.lang), Message::ChangeSettings)
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
            },
            SettingsMessage::ChangePressed => {
                let config = self.current_config();
                match Self::persist_config(&config) {
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
        let engine_path = (!self.engine_path.is_empty()).then(|| self.engine_path.clone());
        config::OfflinePuzzlesConfig {
            engine_path,
            engine_limit: self.saved_configs.engine_limit.clone(),
            window_width: self.window_width,
            window_height: self.window_height,
            maximized: self.maximized,
            puzzle_db_location: self.puzzle_db_location_value.clone(),
            piece_theme: self.piece_theme,
            search_results_limit: self.search_results_limit_value.parse().unwrap(),
            play_sound: self.play_sound,
            auto_load_next: self.auto_load_next,
            flip_board: self.flip_board,
            show_coordinates: self.show_coordinates,
            board_theme: self.board_theme,
            interface_theme: self.interface_theme,
            lang: self.lang.lang,
            export_pgs: self.export_pgs.parse().unwrap(),
            last_min_rating: self.saved_configs.last_min_rating,
            last_max_rating: self.saved_configs.last_max_rating,
            last_min_popularity: self.saved_configs.last_min_popularity,
            last_theme: self.saved_configs.last_theme,
            last_opening: self.saved_configs.last_opening,
            last_variation: self.saved_configs.last_variation.clone(),
            last_opening_side: self.saved_configs.last_opening_side,
            puzzle_sqlite_location: self.saved_configs.puzzle_sqlite_location.clone(),
        }
    }

    fn persist_config(config: &config::OfflinePuzzlesConfig) -> Result<(), &'static str> {
        let file = std::fs::File::create(SETTINGS_FILE).map_err(|_| "error_reading_config")?;
        serde_json::to_writer_pretty(file, config).map_err(|_| "error_saving")
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
        match Self::persist_config(&config) {
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

    pub fn save_window_size(&self) {
        let mut config = config::load_config();
        config.window_width = self.window_width;
        config.window_height = self.window_height;
        config.maximized = self.maximized;
        let file = std::fs::File::create(SETTINGS_FILE);
        match file {
            Ok(file) => {
                if serde_json::to_writer_pretty(file, &config).is_err() {
                    println!("Error saving config file.");
                }
            } Err(_) => println!("Error opening settings file")
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

    pub async fn send_changes(play_sound: bool, auto_load: bool, flip: bool, coords: bool, pieces: styles::PieceTheme, theme: styles::BoardTheme, engine: String, lang: lang::Language) -> Option<config::OfflinePuzzlesConfig> {
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

        let _task = settings_tab.update(SettingsMessage::SelectBoardTheme(styles::BoardTheme::Green));
        let (play_sound, auto_load, flip, coords, pieces, theme, engine, lang) =
            settings_tab.settings_change_values();
        let payload = SettingsTab::change_payload(
            config, play_sound, auto_load, flip, coords, pieces, theme, engine, lang,
        );
        settings_tab.saved_configs = payload;

        assert_eq!(settings_tab.board_theme, styles::BoardTheme::Green);
        assert_eq!(settings_tab.saved_configs.board_theme, settings_tab.board_theme);
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
        assert_eq!(settings_tab.saved_configs.board_theme, settings_tab.board_theme);
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
                ).style(styles::pick_list_style).menu_style(styles::menu_style)
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "board_theme")),
                PickList::new(
                    &styles::BoardTheme::ALL[..],
                    Some(self.board_theme),
                    SettingsMessage::SelectBoardTheme
                ).style(styles::pick_list_style).menu_style(styles::menu_style)
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "interface_appearance")),
                PickList::new(
                    &styles::InterfaceTheme::ALL[..],
                    Some(self.interface_theme),
                    SettingsMessage::SelectInterfaceTheme
                ).style(styles::pick_list_style).menu_style(styles::menu_style)
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "language")),
                PickList::new(
                    PickListWrapper::get_langs(self.lang.lang),
                    Some(self.lang.clone()),
                    SettingsMessage::SelectLanguage
                ).style(styles::pick_list_style).menu_style(styles::menu_style)
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "play_sound")),
                Checkbox::new(self.play_sound).on_toggle(SettingsMessage::CheckPlaySound).size(20).style(styles::checkbox_style),
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "auto_load")),
                Checkbox::new(self.auto_load_next).on_toggle(SettingsMessage::CheckAutoLoad).size(20).style(styles::checkbox_style),
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "flip_board")),
                Checkbox::new(self.flip_board).on_toggle(SettingsMessage::CheckFlipBoard).size(20).style(styles::checkbox_style),
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "show_coords")),
                Checkbox::new(self.show_coordinates).on_toggle(SettingsMessage::CheckShowCoords).size(20).style(styles::checkbox_style),
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "pdf_number_of_pages")),
                TextInput::new(
                    &self.export_pgs,
                    &self.export_pgs,
                ).on_input(SettingsMessage::ChangePDFExportPgs).width(60),
            ].spacing(5).align_y(Alignment::Center),
            row![
                Text::new(lang::tr(&self.lang.lang, "get_first_puzzles1")),
                TextInput::new(
                    &self.search_results_limit_value,
                    &self.search_results_limit_value,
                ).on_input(SettingsMessage::ChangeSearchResultLimit).width(80),
                Text::new(lang::tr(&self.lang.lang, "get_first_puzzles2"))
            ].spacing(5).align_y(Alignment::Center),
            Text::new(lang::tr(&self.lang.lang, "engine_path")),
            row![
                TextInput::new(
                    &self.engine_path,
                    &self.engine_path,
                ).on_input(SettingsMessage::ChangeEnginePath).width(200),
                Button::new(Text::new(lang::tr(&self.lang.lang, "select"))).on_press(SettingsMessage::SearchEnginePressed).style(btn_style_simple),
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
            Button::new(Text::new(lang::tr(&self.lang.lang, "save"))).padding(5).on_press(SettingsMessage::ChangePressed).style(btn_style_simple),
            Text::new(&self.settings_status).align_y(alignment::Vertical::Bottom),

        ].spacing(10).align_x(Alignment::Center);
        let content: Element<SettingsMessage, Theme, iced::Renderer> = Container::new(
            Scrollable::new(
                Column::new().padding([0, 30]).spacing(10).push(col_settings)
            )
        ).align_x(alignment::Horizontal::Center).height(Length::Fill).width(Length::Fill).into();

        content.map(Message::Settings)
    }
}
