use crate::{
    lang,
    openings::{Openings, Variation},
    search_tab::OpeningSide,
    search_tab::TacticalThemes,
    styles,
};
use chess::{Board, ChessMove, Piece, Square};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

pub use crate::models::Puzzle;

pub static SETTINGS: LazyLock<OfflinePuzzlesConfig> = LazyLock::new(load_config);

pub const MAX_RATING: i32 = 3600;
pub const PDF_TEXT_FONT_BYTES: &[u8] = include_bytes!("../font/NotoSans-Regular.ttf");
pub const PDF_TEXT_FONT_NAME: &str = "NotoSans-Regular";
pub const PDF_CHESS_SYMBOL_FONT_BYTES: &[u8] =
    include_bytes!("../font/NotoSansSymbols2-Regular.ttf");
pub const PDF_CHESS_SYMBOL_FONT_NAME: &str = "NotoSansSymbols2-Regular";

//pub const FONT_DIRECTORY: &str = "font/";
pub const PUZZLES_DIRECTORY: &str = "puzzles/";
pub const PIECES_DIRECTORY: &str = "pieces";
pub const SETTINGS_FILE: &str = "settings.json";
pub const DATABASE_URL: &str = "ocp.db";

pub const REQUIRED_TRANSLATION_RESOURCES: [&str; 6] = [
    "translations/en-US/ocp.ftl",
    "translations/pt-BR/ocp.ftl",
    "translations/es/ocp.ftl",
    "translations/fr/ocp.ftl",
    "translations/cn/ocp.ftl",
    "translations/nl/ocp.ftl",
];

pub const REQUIRED_PIECE_RESOURCES: [&str; 12] = [
    "pieces/cburnett/wP.svg",
    "pieces/cburnett/wR.svg",
    "pieces/cburnett/wN.svg",
    "pieces/cburnett/wB.svg",
    "pieces/cburnett/wQ.svg",
    "pieces/cburnett/wK.svg",
    "pieces/cburnett/bP.svg",
    "pieces/cburnett/bR.svg",
    "pieces/cburnett/bN.svg",
    "pieces/cburnett/bB.svg",
    "pieces/cburnett/bQ.svg",
    "pieces/cburnett/bK.svg",
];

#[derive(Debug)]
pub struct RuntimeResourceError {
    relative_path: PathBuf,
    attempted_paths: Vec<PathBuf>,
    io_error: io::Error,
}

impl RuntimeResourceError {
    fn new(relative_path: &Path, attempted_paths: Vec<PathBuf>, io_error: io::Error) -> Self {
        Self {
            relative_path: relative_path.to_path_buf(),
            attempted_paths,
            io_error,
        }
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    #[cfg(test)]
    pub fn attempted_paths(&self) -> &[PathBuf] {
        &self.attempted_paths
    }

    #[cfg(test)]
    pub fn io_error(&self) -> &io::Error {
        &self.io_error
    }
}

impl fmt::Display for RuntimeResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Resource:")?;
        writeln!(formatter, "{}", self.relative_path.display())?;
        if !self.attempted_paths.is_empty() {
            writeln!(formatter)?;
            writeln!(formatter, "Attempted location(s):")?;
            for path in &self.attempted_paths {
                writeln!(formatter, "{}", path.display())?;
            }
        }
        writeln!(formatter)?;
        write!(formatter, "Error:\n{}", self.io_error)
    }
}

impl std::error::Error for RuntimeResourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.io_error)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeResourceResolver {
    executable_path: PathBuf,
    manifest_directory: PathBuf,
}

impl RuntimeResourceResolver {
    pub(crate) fn new(executable_path: PathBuf, manifest_directory: PathBuf) -> Self {
        Self {
            executable_path,
            manifest_directory,
        }
    }

    fn for_current_process() -> Result<Self, RuntimeResourceError> {
        let executable_path = std::env::current_exe().map_err(|error| {
            RuntimeResourceError::new(Path::new("<current executable>"), Vec::new(), error)
        })?;
        Ok(Self::new(
            executable_path,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ))
    }

    fn cargo_manifest_fallback_allowed(&self) -> bool {
        let Some(executable_directory) = self.executable_path.parent() else {
            return false;
        };
        let Ok(executable_directory) = std::fs::canonicalize(executable_directory) else {
            return false;
        };
        let Ok(cargo_target_directory) =
            std::fs::canonicalize(self.manifest_directory.join("target"))
        else {
            return false;
        };

        executable_directory.starts_with(cargo_target_directory)
    }

    pub(crate) fn resolve(&self, relative_path: &Path) -> Result<PathBuf, RuntimeResourceError> {
        if relative_path.is_absolute() {
            return Err(RuntimeResourceError::new(
                relative_path,
                Vec::new(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "runtime resource path must be relative",
                ),
            ));
        }

        let executable_directory = self.executable_path.parent().ok_or_else(|| {
            RuntimeResourceError::new(
                relative_path,
                Vec::new(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "current executable has no parent directory",
                ),
            )
        })?;
        let primary = executable_directory.join(relative_path);
        match probe_regular_file(&primary) {
            Ok(()) => return Ok(primary),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && self.cargo_manifest_fallback_allowed() => {}
            Err(error) => {
                return Err(RuntimeResourceError::new(
                    relative_path,
                    vec![primary],
                    error,
                ));
            }
        }

        let fallback = self.manifest_directory.join(relative_path);
        probe_regular_file(&fallback).map_err(|error| {
            RuntimeResourceError::new(relative_path, vec![primary, fallback.clone()], error)
        })?;
        Ok(fallback)
    }
}

fn probe_regular_file(path: &Path) -> io::Result<()> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "runtime resource is not a regular file",
        ))
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeResources {
    paths: HashMap<PathBuf, PathBuf>,
}

impl RuntimeResources {
    fn load() -> Result<Self, RuntimeResourceError> {
        let resolver = RuntimeResourceResolver::for_current_process()?;
        let mut paths = HashMap::with_capacity(
            REQUIRED_TRANSLATION_RESOURCES.len() + REQUIRED_PIECE_RESOURCES.len(),
        );
        for relative in REQUIRED_TRANSLATION_RESOURCES
            .iter()
            .chain(REQUIRED_PIECE_RESOURCES.iter())
        {
            let relative = PathBuf::from(relative);
            let resolved = resolver.resolve(&relative)?;
            paths.insert(relative, resolved);
        }
        Ok(Self { paths })
    }

    pub(crate) fn path(&self, relative_path: &Path) -> Option<&Path> {
        self.paths.get(relative_path).map(PathBuf::as_path)
    }
}

static RUNTIME_RESOURCES: LazyLock<Result<RuntimeResources, RuntimeResourceError>> =
    LazyLock::new(RuntimeResources::load);

pub(crate) fn runtime_resources() -> Result<&'static RuntimeResources, &'static RuntimeResourceError>
{
    RUNTIME_RESOURCES.as_ref()
}

pub(crate) fn piece_resource_relative_path(theme: &OsStr, file_name: &OsStr) -> PathBuf {
    Path::new(PIECES_DIRECTORY).join(theme).join(file_name)
}

// Iced widget IDs need to be static
pub static BTN_IDS: [&str; 64] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
    "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46", "47", "48",
    "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "60", "61", "62", "63",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Puzzle,
    Analysis,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OfflinePuzzlesConfig {
    pub engine_path: Option<String>,
    pub engine_limit: String,
    pub window_width: f32,
    pub window_height: f32,
    pub maximized: bool,
    pub puzzle_db_location: String,
    pub piece_theme: styles::PieceTheme,
    pub search_results_limit: usize,
    pub play_sound: bool,
    pub auto_load_next: bool,
    pub flip_board: bool,
    pub show_coordinates: bool,
    pub board_theme: styles::BoardTheme,
    #[serde(default)]
    pub interface_theme: styles::InterfaceTheme,
    pub lang: lang::Language,
    pub export_pgs: i32,
    pub last_min_rating: i32,
    pub last_max_rating: i32,
    pub last_min_popularity: i32,
    pub last_theme: TacticalThemes,
    pub last_opening: Openings,
    pub last_variation: Variation,
    pub last_opening_side: Option<OpeningSide>,
    #[serde(default)]
    pub puzzle_sqlite_location: Option<String>,
}

impl ::std::default::Default for OfflinePuzzlesConfig {
    fn default() -> Self {
        Self {
            engine_path: None,
            engine_limit: String::from("depth 40"),
            window_width: 1010.,
            window_height: 680.,
            maximized: false,
            puzzle_db_location: String::from(PUZZLES_DIRECTORY) + "lichess_db_puzzle.csv",
            piece_theme: styles::PieceTheme::Cburnett,
            search_results_limit: 100,
            play_sound: true,
            auto_load_next: true,
            flip_board: false,
            show_coordinates: false,
            board_theme: styles::BoardTheme::default(),
            interface_theme: styles::InterfaceTheme::default(),
            lang: lang::Language::English,
            export_pgs: 50,
            last_min_rating: 0,
            last_max_rating: 1000,
            last_min_popularity: 0,
            last_theme: TacticalThemes::All,
            last_opening: Openings::Any,
            last_variation: Variation::ANY,
            last_opening_side: Some(OpeningSide::Any),
            puzzle_sqlite_location: None,
        }
    }
}

pub fn puzzle_source_exists(config: &OfflinePuzzlesConfig) -> bool {
    match &config.puzzle_sqlite_location {
        Some(sqlite_path) => std::path::Path::new(sqlite_path).is_file(),
        None => std::path::Path::new(&config.puzzle_db_location).is_file(),
    }
}

pub fn load_config() -> OfflinePuzzlesConfig {
    load_config_from_path(Path::new(SETTINGS_FILE))
}

pub fn load_config_from_path(path: &Path) -> OfflinePuzzlesConfig {
    let config;
    let file = std::fs::File::open(path);
    match file {
        Ok(file) => {
            let reader = std::io::BufReader::new(file);
            let config_json = deserialize_config(reader);
            match config_json {
                Ok(cfg) => config = cfg,
                Err(_) => config = OfflinePuzzlesConfig::default(),
            }
        }
        Err(_) => config = OfflinePuzzlesConfig::default(),
    }
    config
}

pub(crate) fn persist_config(config: &OfflinePuzzlesConfig) -> Result<(), &'static str> {
    persist_config_to_path(config, Path::new(SETTINGS_FILE))
}

pub(crate) fn persist_config_to_path(
    config: &OfflinePuzzlesConfig,
    path: &Path,
) -> Result<(), &'static str> {
    persist_config_to_path_with_renamer(config, path, |from, to| std::fs::rename(from, to))
}

fn persist_config_to_path_with_renamer<F>(
    config: &OfflinePuzzlesConfig,
    path: &Path,
    renamer: F,
) -> Result<(), &'static str>
where
    F: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    let serialized = serde_json::to_vec_pretty(config).map_err(|_| "error_saving")?;
    let temporary = temporary_config_path(path)?;
    let result = (|| {
        let mut file = std::fs::File::create(&temporary).map_err(|_| "error_saving")?;
        file.write_all(&serialized).map_err(|_| "error_saving")?;
        file.sync_all().map_err(|_| "error_saving")?;
        drop(file);
        renamer(&temporary, path).map_err(|_| "error_saving")
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }

    result
}

fn temporary_config_path(path: &Path) -> Result<PathBuf, &'static str> {
    let file_name = path.file_name().ok_or("error_saving")?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    Ok(path.with_file_name(temporary_name))
}

fn deserialize_config(reader: impl std::io::Read) -> serde_json::Result<OfflinePuzzlesConfig> {
    let mut config: OfflinePuzzlesConfig = serde_json::from_reader(reader)?;
    config.piece_theme = config.piece_theme.normalize();
    Ok(config)
}

fn piece_localized(lang: &lang::Language, piece: &str) -> String {
    match piece {
        "B" => lang::tr(lang, "bishop"),
        "N" => lang::tr(lang, "knight"),
        "R" => lang::tr(lang, "rook"),
        "Q" => lang::tr(lang, "queen"),
        _ => lang::tr(lang, "king"),
    }
}

pub fn coord_to_san(board: &Board, coords: String, lang: &lang::Language) -> Option<String> {
    let (promotion_piece, coords) = if coords.len() > 4 {
        (coords[4..5].to_uppercase(), String::from(&coords[0..4]))
    } else {
        (String::from(""), coords)
    };

    let mut san = None;
    let orig_square = Square::from_str(&coords[0..2]).unwrap();
    let dest_square = Square::from_str(&coords[2..4]).unwrap();
    let piece = board.piece_on(orig_square);
    if let Some(piece) = piece {
        if piece == Piece::King && (coords == "e1g1" || coords == "e8g8") {
            san = Some(String::from("0-0"));
        } else if piece == Piece::King && (coords == "e1c1" || coords == "e8c8") {
            san = Some(String::from("0-0-0"));
        } else {
            let mut san_str = String::new();
            let mut san_localized = String::new();
            let is_en_passant = piece == Piece::Pawn
                && board.piece_on(dest_square).is_none()
                && dest_square.get_file() != orig_square.get_file();
            let is_capture = board.piece_on(dest_square).is_some();
            match piece {
                Piece::Pawn => {
                    // We're also creating the san in English notation because
                    // we use the chess crate to check if it's valid (in order
                    // to know if it needs disambiguation or not)
                    san_str.push_str(&coords[0..1]);
                    san_localized.push_str(&coords[0..1]);
                }
                Piece::Bishop => {
                    san_str.push('B');
                    san_localized.push_str(&lang::tr(lang, "bishop"));
                }
                Piece::Knight => {
                    san_str.push('N');
                    san_localized.push_str(&lang::tr(lang, "knight"));
                }
                Piece::Rook => {
                    san_str.push('R');
                    san_localized.push_str(&lang::tr(lang, "rook"));
                }
                Piece::Queen => {
                    san_str.push('Q');
                    san_localized.push_str(&lang::tr(lang, "queen"));
                }
                Piece::King => {
                    san_str.push('K');
                    san_localized.push_str(&lang::tr(lang, "king"));
                }
            }
            // Checking fist the cases of capture
            if is_en_passant {
                san_localized.push_str(&(String::from("x") + &coords[2..4] + " e.p."));
            } else if is_capture {
                let capture = if piece == Piece::Pawn {
                    // Note: For the from_san() function we really can't use the equal sign: https://github.com/jordanbray/chess/issues/80
                    san_str.clone() + "x" + &coords[2..] + &promotion_piece
                } else {
                    san_str.clone() + "x" + &coords[2..]
                };
                let try_move = ChessMove::from_san(board, &capture);
                if try_move.is_ok() {
                    if promotion_piece.is_empty() {
                        san_str.push_str(&(String::from("x") + &coords[2..]));
                        san_localized.push_str(&(String::from("x") + &coords[2..]));
                    } else {
                        san_str.push_str(&(String::from("x") + &coords[2..] + &promotion_piece));
                        san_localized.push_str(
                            &(String::from("x")
                                + &coords[2..]
                                + "="
                                + &piece_localized(lang, &promotion_piece)),
                        );
                    }
                } else {
                    //the simple notation can only fail because of ambiguity, so we try to specify
                    //either the file or the rank
                    let capture_with_file = san_str.clone() + &coords[0..1] + "x" + &coords[2..];
                    let try_move_file = ChessMove::from_san(board, &capture_with_file);
                    if try_move_file.is_ok() {
                        san_localized.push_str(&(String::from(&coords[0..1]) + "x" + &coords[2..]));
                    } else {
                        san_localized.push_str(&(String::from(&coords[1..2]) + "x" + &coords[2..]));
                    }
                }
            // And now the regular moves
            } else if piece == Piece::Pawn {
                if promotion_piece.is_empty() {
                    san_localized = String::from(&coords[2..]);
                } else {
                    san_str = san_str + &coords[2..] + &promotion_piece;
                    san_localized =
                        String::from(&coords[2..]) + "=" + &piece_localized(lang, &promotion_piece);
                }
            } else {
                let move_with_regular_notation = san_str.clone() + &coords[2..];
                let move_to_try = ChessMove::from_san(board, &move_with_regular_notation);
                if move_to_try.is_ok() {
                    san_str.push_str(&coords[2..]);
                    san_localized.push_str(&coords[2..]);
                } else {
                    //the simple notation can only fail because of ambiguity, so we try to specify
                    //either the file or the rank
                    let move_notation_with_file = san_str.clone() + &coords[0..1] + &coords[2..];
                    let try_move_file = ChessMove::from_san(board, &move_notation_with_file);
                    if try_move_file.is_ok() {
                        san_localized.push_str(&(String::from(&coords[0..1]) + &coords[2..]));
                    } else {
                        san_localized.push_str(&(String::from(&coords[1..2]) + &coords[2..]));
                    }
                }
            }
            let chess_move = ChessMove::from_san(board, &san_str);
            // Note: It can indeed return Err for a moment when using the engine (and quickly taking
            // back moves), I guess for a sec the engine & board may get desynced, so we can't just unwrap it.
            if let Ok(chess_move) = chess_move {
                let current_board = board.make_move_new(chess_move);
                if current_board.status() == chess::BoardStatus::Checkmate {
                    san_localized.push('#');
                } else if current_board.checkers().popcnt() != 0 {
                    san_localized.push('+');
                }
            }
            san = Some(san_localized);
        }
    }
    san
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

    fn read_fixture_puzzles() -> Vec<Puzzle> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(FIXTURE.as_bytes());
        reader
            .deserialize::<Puzzle>()
            .map(|r| r.expect("fixture row should deserialize into Puzzle"))
            .collect()
    }

    #[test]
    fn test_current_csv_has_daily_date_header_and_data() {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(FIXTURE.as_bytes());
        let headers = reader.headers().expect("fixture should have headers");
        assert_eq!(headers.len(), 11);
        assert_eq!(headers.get(10), Some("DailyDate"));

        let records: Vec<csv::StringRecord> = reader
            .records()
            .map(|r| r.expect("fixture row should be valid CSV"))
            .collect();
        assert_eq!(records.len(), 4);
        assert_eq!(records[2].len(), 11);
        assert_eq!(records[2].get(10), Some("2026-08-28"));

        let puzzles = read_fixture_puzzles();
        let puzzle = &puzzles[2];
        assert_eq!(puzzle.puzzle_id, "00010");
        assert_eq!(
            puzzle.fen,
            "r1bqkb1r/pppppppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 2 3"
        );
        assert_eq!(puzzle.moves, "f3g5 e7e6 g5f7");
        assert_eq!(puzzle.rating, 1700);
        assert_eq!(puzzle.rating_deviation, 80);
        assert_eq!(puzzle.popularity, 92);
        assert_eq!(puzzle.nb_plays, 7500);
        assert_eq!(puzzle.themes, "fork sacrifice middlegame");
        assert_eq!(puzzle.game_url, "https://lichess.org/training/ghi789");
        assert_eq!(puzzle.opening, "Italian_Game");
    }

    #[test]
    fn test_csv_deserializes_correct_row_count() {
        let puzzles = read_fixture_puzzles();
        assert_eq!(puzzles.len(), 4, "fixture should produce exactly 4 puzzles");
    }

    #[test]
    fn test_all_fields_on_normal_puzzle() {
        let puzzles = read_fixture_puzzles();
        let p = &puzzles[0];
        assert_eq!(p.puzzle_id, "00008");
        assert_eq!(p.fen, "N4k3/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1");
        assert_eq!(p.moves, "e1e8");
        assert_eq!(p.rating, 1500);
        assert_eq!(p.rating_deviation, 70);
        assert_eq!(p.popularity, 95);
        assert_eq!(p.nb_plays, 10000);
        assert_eq!(p.themes, "fork opening");
        assert_eq!(p.game_url, "https://lichess.org/training/abc123");
        assert_eq!(p.opening, "Italian_Game");
    }

    #[test]
    fn test_empty_opening_tags() {
        let puzzles = read_fixture_puzzles();
        let p = &puzzles[1];
        assert_eq!(
            p.opening, "",
            "empty OpeningTags should deserialize to empty string"
        );
    }

    #[test]
    fn test_multiple_themes_preserved() {
        let puzzles = read_fixture_puzzles();
        let p = &puzzles[2];
        assert_eq!(p.themes, "fork sacrifice middlegame");
    }

    #[test]
    fn test_solver_move_count_cases() {
        let cases: Vec<(&str, usize)> = vec![
            ("", 0),
            ("e2e4", 0),
            ("e2e4 e7e5", 1),
            ("e2e4 e7e5 g1f3", 1),
            ("e2e4 e7e5 g1f3 d7d5", 2),
            ("e2e4 e7e5 g1f3 d7d5 d2d4", 2),
            ("e2e4 e7e5 g1f3 d7d5 d2d4 e5d4", 3),
        ];
        for (moves_str, expected) in cases {
            let puzzle = Puzzle {
                puzzle_id: String::new(),
                fen: String::new(),
                moves: moves_str.to_string(),
                rating: 0,
                rating_deviation: 0,
                popularity: 0,
                nb_plays: 0,
                themes: String::new(),
                game_url: String::new(),
                opening: String::new(),
            };
            assert_eq!(
                puzzle.solver_move_count(),
                expected,
                "moves: {:?}",
                moves_str
            );
        }
    }

    #[test]
    fn test_solver_move_count_whitespace_robustness() {
        let puzzle = Puzzle {
            puzzle_id: String::new(),
            fen: String::new(),
            moves: "e2e4   e7e5    g1f3".to_string(),
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        assert_eq!(puzzle.solver_move_count(), 1);
    }

    #[test]
    fn config_without_new_fields_deserializes_with_defaults() {
        let mut settings_json = serde_json::to_value(OfflinePuzzlesConfig::default())
            .expect("default configuration should serialize");
        let fields = settings_json
            .as_object_mut()
            .expect("serialized configuration should be an object");
        fields.remove("puzzle_sqlite_location");
        fields.remove("interface_theme");

        let config: OfflinePuzzlesConfig = serde_json::from_value(settings_json)
            .expect("configuration without new fields should deserialize");
        assert!(
            config.puzzle_sqlite_location.is_none(),
            "puzzle_sqlite_location should be None when absent from JSON"
        );
        assert_eq!(
            config.interface_theme,
            styles::InterfaceTheme::Light,
            "existing settings without an interface theme must keep the light appearance"
        );
    }

    #[test]
    fn interface_theme_round_trips_without_changing_board_theme() {
        let config = OfflinePuzzlesConfig {
            board_theme: styles::BoardTheme::Blue,
            interface_theme: styles::InterfaceTheme::Dark,
            ..OfflinePuzzlesConfig::default()
        };

        let serialized = serde_json::to_string(&config).expect("configuration should serialize");
        let restored = deserialize_config(serialized.as_bytes())
            .expect("serialized configuration should deserialize");

        assert_eq!(restored.interface_theme, styles::InterfaceTheme::Dark);
        assert_eq!(restored.board_theme, styles::BoardTheme::Blue);
    }

    #[test]
    fn test_default_puzzle_sqlite_location_is_none() {
        let config = OfflinePuzzlesConfig::default();
        assert_eq!(config.puzzle_sqlite_location, None);
    }

    #[test]
    fn default_search_results_limit_is_100() {
        assert_eq!(OfflinePuzzlesConfig::default().search_results_limit, 100);
    }

    #[test]
    fn missing_config_path_loads_defaults() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp")
            .join(format!(
                "missing_settings_{}_{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after UNIX epoch")
                    .as_nanos(),
            ));

        assert!(!path.exists(), "the test configuration path must not exist");
        let config = load_config_from_path(&path);

        assert_eq!(config.search_results_limit, 100);
        assert_eq!(config.engine_limit, "depth 40");
    }

    #[test]
    fn deserializing_config_normalizes_every_legacy_piece_theme() {
        for legacy_theme in [
            "Alpha",
            "FontAlpha",
            "Merida",
            "California",
            "Cardinal",
            "Governor",
            "Dubrovny",
            "Gioco",
            "Icpieces",
            "Maestro",
            "Staunty",
            "Tatiana",
        ] {
            let mut config_json = serde_json::to_value(OfflinePuzzlesConfig::default())
                .expect("default config should serialize");
            config_json["piece_theme"] = serde_json::json!(legacy_theme);
            config_json["engine_limit"] = serde_json::json!("nodes 42");

            let config = deserialize_config(config_json.to_string().as_bytes())
                .expect("persisted config should deserialize");

            assert_eq!(
                config.piece_theme,
                styles::PieceTheme::Cburnett,
                "{legacy_theme}"
            );
            assert_eq!(config.engine_limit, "nodes 42", "{legacy_theme}");
        }
    }

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}_{}", name, std::process::id()))
    }

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
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
            Self(directory)
        }

        fn settings_path(&self) -> std::path::PathBuf {
            self.0.join("settings.json")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn persist_config_to_path_round_trips_without_touching_repository_settings() {
        let directory = TestDirectory::new("round-trip");
        let path = directory.settings_path();
        let config = OfflinePuzzlesConfig {
            engine_limit: "nodes 123".into(),
            window_width: 1234.0,
            window_height: 567.0,
            interface_theme: styles::InterfaceTheme::Dark,
            ..OfflinePuzzlesConfig::default()
        };

        persist_config_to_path(&config, &path).expect("test configuration should persist");

        let restored = load_config_from_path(&path);
        assert_eq!(restored.engine_limit, "nodes 123");
        assert_eq!(restored.window_width, 1234.0);
        assert_eq!(restored.window_height, 567.0);
        assert_eq!(restored.interface_theme, styles::InterfaceTheme::Dark);
        assert!(!path.with_file_name("settings.json.tmp").exists());
    }

    #[test]
    fn failed_temporary_creation_preserves_existing_config_bytes() {
        let directory = TestDirectory::new("blocked-temporary");
        let path = directory.settings_path();
        let original = b"{\"engine_limit\":\"original\"}";
        std::fs::write(&path, original).expect("existing configuration should be seeded");
        std::fs::create_dir(path.with_file_name("settings.json.tmp"))
            .expect("temporary path should be blocked by a directory");

        let result = persist_config_to_path(&OfflinePuzzlesConfig::default(), &path);

        assert_eq!(result, Err("error_saving"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn failed_replace_preserves_existing_config_bytes_and_cleans_temporary() {
        let directory = TestDirectory::new("failed-replace");
        let path = directory.settings_path();
        let original = b"{\"engine_limit\":\"original\"}";
        std::fs::write(&path, original).expect("existing configuration should be seeded");

        let result =
            persist_config_to_path_with_renamer(&OfflinePuzzlesConfig::default(), &path, |_, _| {
                Err(std::io::Error::other("injected rename failure"))
            });

        assert_eq!(result, Err("error_saving"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_file_name("settings.json.tmp").exists());
    }

    #[test]
    fn test_source_exists_csv_active() {
        let csv_path = tmp_path("source_csv_active.csv");
        std::fs::write(&csv_path, b"test").unwrap();
        let cfg = OfflinePuzzlesConfig {
            puzzle_sqlite_location: None,
            puzzle_db_location: csv_path.to_str().unwrap().to_string(),
            ..OfflinePuzzlesConfig::default()
        };
        assert!(puzzle_source_exists(&cfg));
        let _ = std::fs::remove_file(&csv_path);
    }

    #[test]
    fn test_source_exists_csv_missing() {
        let cfg = OfflinePuzzlesConfig {
            puzzle_sqlite_location: None,
            puzzle_db_location: "/nonexistent/path.csv".to_string(),
            ..OfflinePuzzlesConfig::default()
        };
        assert!(!puzzle_source_exists(&cfg));
    }

    #[test]
    fn test_source_exists_sqlite_active_csv_missing() {
        let sqlite_path = tmp_path("source_sqlite_active.sqlite");
        std::fs::write(&sqlite_path, b"test").unwrap();
        let cfg = OfflinePuzzlesConfig {
            puzzle_sqlite_location: Some(sqlite_path.to_str().unwrap().to_string()),
            puzzle_db_location: "/nonexistent/path.csv".to_string(),
            ..OfflinePuzzlesConfig::default()
        };
        assert!(puzzle_source_exists(&cfg));
        let _ = std::fs::remove_file(&sqlite_path);
    }

    #[test]
    fn test_source_exists_sqlite_missing_csv_existing() {
        let csv_path = tmp_path("source_csv_fallback.csv");
        std::fs::write(&csv_path, b"test").unwrap();
        let cfg = OfflinePuzzlesConfig {
            puzzle_sqlite_location: Some("/nonexistent/db.sqlite".to_string()),
            puzzle_db_location: csv_path.to_str().unwrap().to_string(),
            ..OfflinePuzzlesConfig::default()
        };
        assert!(
            !puzzle_source_exists(&cfg),
            "SQLite missing should NOT fall back to CSV"
        );
        let _ = std::fs::remove_file(&csv_path);
    }

    fn write_resource(path: &Path, contents: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("resource should have a parent"))
            .expect("resource parent should be created");
        std::fs::write(path, contents).expect("resource should be written");
    }

    fn create_executable_directory(executable: &Path) {
        std::fs::create_dir_all(
            executable
                .parent()
                .expect("executable should have a parent directory"),
        )
        .expect("executable directory should be created");
    }

    #[test]
    fn runtime_resource_prefers_candidate_beside_executable() {
        let directory = TestDirectory::new("runtime-resource-primary");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/es/ocp.ftl");
        let primary = executable.parent().unwrap().join(relative);
        write_resource(&primary, b"primary");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), primary);
    }

    #[test]
    fn runtime_resource_primary_wins_over_manifest_fallback() {
        let directory = TestDirectory::new("runtime-resource-priority");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/release/chess-material-studio.exe");
        let relative = Path::new("pieces/cburnett/wK.svg");
        let primary = executable.parent().unwrap().join(relative);
        write_resource(&primary, b"primary");
        write_resource(&manifest.join(relative), b"fallback");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), primary);
    }

    #[test]
    fn runtime_resource_uses_manifest_only_for_cargo_target_execution() {
        let directory = TestDirectory::new("runtime-resource-cargo-fallback");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/nl/ocp.ftl");
        let fallback = manifest.join(relative);
        create_executable_directory(&executable);
        write_resource(&fallback, b"fallback");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), fallback);
    }

    #[test]
    fn runtime_resource_allows_manifest_fallback_from_release() {
        let directory = TestDirectory::new("runtime-resource-release-fallback");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/release/chess-material-studio.exe");
        let relative = Path::new("translations/es/ocp.ftl");
        let fallback = manifest.join(relative);
        create_executable_directory(&executable);
        write_resource(&fallback, b"fallback");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), fallback);
    }

    #[test]
    fn runtime_resource_allows_manifest_fallback_from_debug_deps() {
        let directory = TestDirectory::new("runtime-resource-deps-fallback");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/deps/chess-material-studio.exe");
        let relative = Path::new("translations/pt-BR/ocp.ftl");
        let fallback = manifest.join(relative);
        create_executable_directory(&executable);
        write_resource(&fallback, b"fallback");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), fallback);
    }

    #[test]
    fn runtime_resource_denies_lexical_target_parent_escape() {
        let directory = TestDirectory::new("runtime-resource-target-escape");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/../outside/chess-material-studio.exe");
        let relative = Path::new("translations/fr/ocp.ftl");
        create_executable_directory(&executable);
        write_resource(&manifest.join(relative), b"must not be used");

        let error = RuntimeResourceResolver::new(executable.clone(), manifest)
            .resolve(relative)
            .unwrap_err();

        assert_eq!(
            error.attempted_paths(),
            &[executable.parent().unwrap().join(relative)]
        );
    }

    #[test]
    fn runtime_resource_denies_fallback_when_target_cannot_be_canonicalized() {
        let directory = TestDirectory::new("runtime-resource-canonicalization-failure");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/cn/ocp.ftl");
        write_resource(&manifest.join(relative), b"must not be used");

        let error = RuntimeResourceResolver::new(executable.clone(), manifest)
            .resolve(relative)
            .unwrap_err();

        assert_eq!(
            error.attempted_paths(),
            &[executable.parent().unwrap().join(relative)]
        );
    }

    #[cfg(windows)]
    #[test]
    fn runtime_resource_allows_windows_case_variants_of_the_same_real_target() {
        let directory = TestDirectory::new("runtime-resource-windows-casing");
        let manifest = directory.0.join("repo-lowercase");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/en-US/ocp.ftl");
        let fallback = manifest.join(relative);
        create_executable_directory(&executable);
        write_resource(&fallback, b"fallback");
        let differently_cased_executable =
            PathBuf::from(executable.to_string_lossy().to_uppercase());

        let resolver = RuntimeResourceResolver::new(differently_cased_executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), fallback);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_resource_allows_symlinked_path_to_real_target() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new("runtime-resource-unix-symlink");
        let manifest = directory.0.join("repo");
        let target = manifest.join("target");
        let linked_target = directory.0.join("linked-target");
        let executable = linked_target.join("debug/chess-material-studio");
        let relative = Path::new("translations/nl/ocp.ftl");
        let fallback = manifest.join(relative);
        std::fs::create_dir_all(target.join("debug")).expect("target/debug should be created");
        symlink(&target, &linked_target).expect("target symlink should be created");
        write_resource(&fallback, b"fallback");

        let resolver = RuntimeResourceResolver::new(executable, manifest);

        assert_eq!(resolver.resolve(relative).unwrap(), fallback);
    }

    #[test]
    fn distributed_executable_never_borrows_manifest_resource() {
        let directory = TestDirectory::new("runtime-resource-distribution");
        let manifest = directory.0.join("repo");
        let executable = directory.0.join("package/chess-material-studio.exe");
        let relative = Path::new("translations/fr/ocp.ftl");
        write_resource(&manifest.join(relative), b"must not be used");

        let error = RuntimeResourceResolver::new(executable.clone(), manifest)
            .resolve(relative)
            .unwrap_err();

        assert_eq!(error.relative_path(), relative);
        assert_eq!(
            error.attempted_paths(),
            &[executable.parent().unwrap().join(relative)]
        );
    }

    #[test]
    fn missing_runtime_resource_reports_every_attempted_location() {
        let directory = TestDirectory::new("runtime-resource-missing");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("pieces/cburnett/bQ.svg");
        create_executable_directory(&executable);

        let error = RuntimeResourceResolver::new(executable.clone(), manifest.clone())
            .resolve(relative)
            .unwrap_err();
        let diagnostic = error.to_string();

        assert_eq!(error.relative_path(), relative);
        assert_eq!(
            error.attempted_paths(),
            &[
                executable.parent().unwrap().join(relative),
                manifest.join(relative),
            ]
        );
        assert!(diagnostic.contains("pieces/cburnett/bQ.svg"));
        for attempted in error.attempted_paths() {
            assert!(diagnostic.contains(&attempted.display().to_string()));
        }
    }

    #[test]
    fn runtime_resource_preserves_unicode_pathbufs() {
        let directory = TestDirectory::new("cms025b-á-é-棋");
        let manifest = directory.0.join("repo-棋");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/es/ocp.ftl");
        let fallback = manifest.join(relative);
        create_executable_directory(&executable);
        write_resource(&fallback, b"unicode");

        let resolved = RuntimeResourceResolver::new(executable, manifest)
            .resolve(relative)
            .unwrap();

        assert_eq!(resolved, fallback);
    }

    #[test]
    fn non_not_found_primary_error_is_never_hidden_by_fallback() {
        let directory = TestDirectory::new("runtime-resource-primary-error");
        let manifest = directory.0.join("repo");
        let executable = manifest.join("target/debug/chess-material-studio.exe");
        let relative = Path::new("translations/en-US/ocp.ftl");
        let primary = executable.parent().unwrap().join(relative);
        std::fs::create_dir_all(&primary).expect("primary directory should be created");
        write_resource(&manifest.join(relative), b"fallback must not be used");

        let error = RuntimeResourceResolver::new(executable, manifest)
            .resolve(relative)
            .unwrap_err();

        assert_eq!(error.attempted_paths(), &[primary]);
        assert_ne!(error.io_error().kind(), std::io::ErrorKind::NotFound);
    }
}
