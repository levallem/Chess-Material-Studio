use crate::{styles, search_tab::TacticalThemes, search_tab::OpeningSide, lang, openings::{Openings, Variation}};
use chess::{Board, ChessMove, Piece, Square};
use std::str::FromStr;
use std::sync::LazyLock;
use iced::Font;

pub use crate::models::Puzzle;

pub static SETTINGS: LazyLock<OfflinePuzzlesConfig> = LazyLock::new(|| {
    load_config()
});

pub const MAX_RATING: i32 = 3600;
pub const CHESS_ALPHA_BYTES: &[u8] = include_bytes!("../font/Alpha.ttf");
pub const CHESS_ALPHA: Font = iced::Font::with_name("Chess Alpha");

//pub const FONT_DIRECTORY: &str = "font/";
pub const PUZZLES_DIRECTORY: &str = "puzzles/";
pub const TRANSLATIONS_DIRECTORY: &str = "./translations/";
pub const PIECES_DIRECTORY: &str = "pieces/";
pub const SETTINGS_FILE: &str = "settings.json";
pub const ONE_PIECE_SOUND_FILE: &str = "1piece.ogg";
pub const TWO_PIECES_SOUND_FILE: &str = "2pieces.ogg";
pub const DATABASE_URL: &str = "ocp.db";

// Iced widget IDs need to be static
pub static BTN_IDS: [&str; 64] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19",
    "20", "21", "22", "23", "24", "25", "26", "27", "28", "29",
    "30", "31", "32", "33", "34", "35", "36", "37", "38", "39",
    "40", "41", "42", "43", "44", "45", "46", "47", "48", "49",
    "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
    "60", "61", "62", "63",
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
            search_results_limit: 20000,
            play_sound: true,
            auto_load_next: true,
            flip_board: false,
            show_coordinates: false,
            board_theme: styles::BoardTheme::default(),
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
    let config;
    let file = std::fs::File::open(SETTINGS_FILE);
    match file {
        Ok(file) => {
            let reader = std::io::BufReader::new(file);
            let config_json = serde_json::from_reader(reader);
            match config_json {
                Ok(cfg) => config = cfg,
                Err(_) => config = OfflinePuzzlesConfig::default()
            }
        } Err(_) => config = OfflinePuzzlesConfig::default()
    }
    config
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
            let is_en_passant = piece == Piece::Pawn &&
                board.piece_on(dest_square).is_none() &&
                dest_square.get_file() != orig_square.get_file();
            let is_capture = board.piece_on(dest_square).is_some();
            match piece {
                Piece::Pawn => {
                    // We're also creating the san in English notation because
                    // we use the chess crate to check if it's valid (in order
                    // to know if it needs disambiguation or not)
                    san_str.push_str(&coords[0..1]);
                    san_localized.push_str(&coords[0..1]);
                } Piece::Bishop => {
                    san_str.push('B');
                    san_localized.push_str(&lang::tr(lang, "bishop"));
                } Piece::Knight => {
                    san_str.push('N');
                    san_localized.push_str(&lang::tr(lang, "knight"));
                } Piece::Rook => {
                    san_str.push('R');
                    san_localized.push_str(&lang::tr(lang, "rook"));
                } Piece::Queen => {
                    san_str.push('Q');
                    san_localized.push_str(&lang::tr(lang, "queen"));
                } Piece::King =>  {
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
                        san_localized.push_str(&(String::from("x") + &coords[2..] + "=" + &piece_localized(lang, &promotion_piece)));
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
                    san_localized = String::from(&coords[2..]) + "=" + &piece_localized(lang, &promotion_piece);
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
        assert_eq!(puzzle.fen, "r1bqkb1r/pppppppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 2 3");
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
        assert_eq!(p.opening, "", "empty OpeningTags should deserialize to empty string");
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
    fn test_old_settings_json_deserializes() {
        let settings_json = include_str!("../settings.json");
        let config: OfflinePuzzlesConfig = serde_json::from_str(settings_json)
            .expect("existing settings.json should deserialize");
        assert!(
            config.puzzle_sqlite_location.is_none(),
            "puzzle_sqlite_location should be None when absent from JSON"
        );
    }

    #[test]
    fn test_default_puzzle_sqlite_location_is_none() {
        let config = OfflinePuzzlesConfig::default();
        assert_eq!(config.puzzle_sqlite_location, None);
    }

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}_{}", name, std::process::id()))
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
        assert!(!puzzle_source_exists(&cfg), "SQLite missing should NOT fall back to CSV");
        let _ = std::fs::remove_file(&csv_path);
    }
}
