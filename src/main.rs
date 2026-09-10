#![windows_subsystem = "windows"]

use download_db::download_lichess_db;
use eval::{Engine, EngineStatus};
use iced::advanced::widget::Id as GenericId;
use iced::widget::svg::Handle;
use iced::widget::text::LineHeight;
use styles::PieceTheme;
use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;
use std::fs::File as StdFile;
use std::str::FromStr;
use tokio::sync::mpsc::{self, Sender};
use iced::widget::{button, center, container, responsive, row, text, text_input, Button, Column, Container, Radio, Row, Svg, Text};
use iced::{Element, Rectangle, Size, Subscription, Theme};
use iced::{alignment, Task, Alignment, Length};
use iced::window::{self, Screenshot};
use iced::event::{self, Event};
use std::borrow::Cow;
use image::{DynamicImage, RgbaImage};
use rfd::AsyncFileDialog;

use iced_aw::{TabLabel, Tabs};
use chess::{Board, BoardStatus, ChessMove, Color, File, Game, Piece, Rank, Square, ALL_SQUARES};
use chess_material_studio::project::ProjectPuzzleDecision;

use rodio::{MixerDeviceSink, DeviceSinkBuilder};

use rand::rng;
use rand::seq::SliceRandom;

mod config;
use config::{ONE_PIECE_SOUND_FILE, TWO_PIECES_SOUND_FILE};

mod styles;
mod search_tab;
pub mod download_db;
use search_tab::{SearchMesssage, SearchTab};

mod settings;
use settings::{SettingsMessage, SettingsTab};

mod puzzles;
use puzzles::{PuzzleMessage, PuzzleTab, GameStatus};

mod project_tab;
use project_tab::{ProjectMessage, ProjectTab, PuzzleReviewView};

use crate::styles::btn_style_simple;

mod eval;
mod export;
mod lang;
mod openings;

pub mod models;
pub mod schema;
mod db;

#[macro_use]
extern crate diesel;
extern crate serde;
#[macro_use]
extern crate serde_derive;

const HEADER_SIZE: f32 = 32.0;
const TAB_PADDING: u16 = 16;
const LICHESS_DB_URL: &str = "https://database.lichess.org/lichess_db_puzzle.csv.zst";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PositionGUI {
    row: i32,
    col: i32,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TabId {
    Search,
    Settings,
    CurrentPuzzle,
    Project,
}

#[derive(Clone, Copy, Hash, Eq, PartialEq, PartialOrd, Ord)]
enum PieceWithColor {
    WhitePawn, WhiteRook, WhiteKnight, WhiteBishop, WhiteQueen, WhiteKing,
    BlackPawn, BlackRook, BlackKnight, BlackBishop, BlackQueen, BlackKing,
}

impl PieceWithColor {
    fn index(&self) -> usize {
        *self as usize
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    WindowInitialized(Option<iced::window::Id>),
    SelectSquare(Square),
    Search(SearchMesssage),
    Settings(SettingsMessage),
    PuzzleInfo(PuzzleMessage),
    Project(ProjectMessage),
    SelectMode(config::GameMode),
    TabSelected(TabId),
    ShowHint,
    ShowNextPuzzle,
    ShowPreviousPuzzle,
    GoBackMove,
    RedoPuzzle,
    DropPiece(Square, iced::Point, iced::Rectangle),
    HandleDropZones(Square, Vec<(iced::advanced::widget::Id, iced::Rectangle)>),
    ScreenshotCreated(Screenshot),
    SaveScreenshot(Option<(Screenshot, String)>),
    ExportPDF(Option<String>),
    LoadPuzzle(Option<Vec<config::Puzzle>>),
    LoadFavorites(Result<Vec<config::Puzzle>, String>),
    FavoriteStatusLoaded {
        puzzle_id: String,
        generation: u64,
        result: Result<bool, String>,
    },
    FavoriteToggled {
        puzzle_id: String,
        generation: u64,
        result: Result<bool, String>,
    },
    LoadProjectPuzzles(Vec<config::Puzzle>),
    ExportPGN(Option<String>),
    ChangeSettings(Option<config::OfflinePuzzlesConfig>),
    EventOccurred(iced::Event),
    StartEngine,
    EngineStopped(bool),
    UpdateEval((Option<String>, Option<String>)),
    EngineReady(mpsc::Sender<String>),
    EngineFileChosen(Option<String>),
    FavoritePuzzle,
    MinimizeUI,
    SaveMaximizedStatusAndExit(bool),
    StartDBDownload,
    DBDownloadFinished,
    DownloadProgress(String),
    PuzzleSqliteSourceSelected,
    PuzzleInputIndexChange(String),
    JumpToPuzzle,
    SetPuzzleReview(ProjectPuzzleDecision),
    ClearPuzzleReview,
}

struct SoundPlayback {
    handle: MixerDeviceSink,
}

impl SoundPlayback {
    pub const ONE_PIECE_SOUND: u8 = 0;
    pub const TWO_PIECE_SOUND: u8 = 1;
    pub fn init_sound() -> Option<Self> {
        let mut sound_playback = None;
        if let Ok(handle) = DeviceSinkBuilder::open_default_sink() {
            sound_playback = Some (
                SoundPlayback {
                    handle,
            });
    }
        sound_playback
    }
    pub fn play_audio(&self, audio: u8) {
        let sink = match audio {
            SoundPlayback::ONE_PIECE_SOUND => {
                rodio::play(
                    self.handle.mixer(),
                    BufReader::new(
                        StdFile::open(ONE_PIECE_SOUND_FILE).unwrap()
                    )).unwrap()
            },
            _ => {
                rodio::play(
                    self.handle.mixer(),
                    BufReader::new(
                        StdFile::open(TWO_PIECES_SOUND_FILE).unwrap()
                    )).unwrap()
            },
        };
        sink.play();
        sink.detach();
    }
}

fn get_image_handles(theme: &PieceTheme) -> Vec<Handle> {
    let mut handles = Vec::<Handle>::with_capacity(12);
    let theme_str = &theme.to_string();

    handles.insert(PieceWithColor::WhitePawn.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wP.svg"));
    handles.insert(PieceWithColor::WhiteRook.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wR.svg"));
    handles.insert(PieceWithColor::WhiteKnight.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wN.svg"));
    handles.insert(PieceWithColor::WhiteBishop.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wB.svg"));
    handles.insert(PieceWithColor::WhiteQueen.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wQ.svg"));
    handles.insert(PieceWithColor::WhiteKing.index(), Handle::from_path(String::from("pieces/") + theme_str + "/wK.svg"));

    handles.insert(PieceWithColor::BlackPawn.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bP.svg"));
    handles.insert(PieceWithColor::BlackRook.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bR.svg"));
    handles.insert(PieceWithColor::BlackKnight.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bN.svg"));
    handles.insert(PieceWithColor::BlackBishop.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bB.svg"));
    handles.insert(PieceWithColor::BlackQueen.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bQ.svg"));
    handles.insert(PieceWithColor::BlackKing.index(), Handle::from_path(String::from("pieces/") + theme_str + "/bK.svg"));

    handles
}

fn gen_board_button_ids() -> Vec<GenericId> {
    let mut ids = Vec::new();
    for id in config::BTN_IDS {
        ids.push(GenericId::new(id));
    }
    ids
}

fn gen_square_hashmap() -> HashMap<GenericId, Square> {
    let mut squares_map = HashMap::new();
    for square in ALL_SQUARES {
        squares_map.insert(GenericId::new(config::BTN_IDS[square.to_index()]), square);
    }
    squares_map
}

// The chess crate has a bug on how it returns the en passant square
// https://github.com/jordanbray/chess/issues/36
// For communication with the engine we need to pass the correct value,
// so this ugly solution is needed.
fn san_correct_ep(fen: String) -> String {
    let mut tokens_vec: Vec<&str> = fen.split_whitespace().collect::<Vec<&str>>();
    let mut new_ep_square = String::from("-");
    if let Some(en_passant) = tokens_vec.get(3)
        && en_passant != &"-" {
            let rank = if String::from(&en_passant[1..2]).parse::<usize>().unwrap() == 4 {
                3
            } else {
                6
            };
            new_ep_square = String::from(&en_passant[0..1]) + &rank.to_string();
    }
    tokens_vec[3] = &new_ep_square;
    tokens_vec.join(" ")
}

fn get_notation_string(board: Board, promo_piece: Piece, from: Square, to: Square) -> String {

    let mut move_made_notation = from.to_string() + &to.to_string();
    let piece = board.piece_on(from);
    let color = board.color_on(from);

    // Check for promotion and adjust the notation accordingly
    if let (Some(piece), Some(color)) = (piece, color)
        && piece == Piece::Pawn && ((color == Color::White && to.get_rank() == Rank::Eighth) ||
                                   (color == Color::Black && to.get_rank() == Rank::First)) {
            match promo_piece {
                Piece::Rook => move_made_notation += "r",
                Piece::Knight => move_made_notation += "n",
                Piece::Bishop => move_made_notation += "b",
                _ => move_made_notation += "q"
            }
    }
    move_made_notation
}

//#[derive(Clone)]
struct OfflinePuzzles {
    pub window_id: Option<iced::window::Id>,
    has_db: bool,
    from_square: Option<Square>,
    board: Board,
    last_move_from: Option<Square>,
    last_move_to: Option<Square>,
    hint_square: Option<Square>,
    puzzle_status: String,
    puzzle_number_ui: String,
    current_favorite: Option<bool>,
    favorite_generation: u64,

    analysis: Game,
    analysis_history: Vec<Board>,
    engine_state: EngineStatus,
    engine_eval: String,
    engine: Engine,
    engine_sender: Option<Sender<String>>,
    engine_move: String,

    downloading_db: bool,
    download_progress: String,
    active_tab: TabId,
    search_tab: SearchTab,
    settings_tab: SettingsTab,
    puzzle_tab: PuzzleTab,
    project_tab: ProjectTab,
    game_mode: config::GameMode,
    sound_playback: Option<SoundPlayback>,
    lang: lang::Language,
    mini_ui: bool,
    square_ids: HashMap<GenericId, Square>,
    board_btn_ids: Vec<GenericId>,
    piece_imgs: Vec<Handle>,
}

impl Default for OfflinePuzzles {
    fn default() -> Self {
        OfflinePuzzles::new(false)
    }
}

impl OfflinePuzzles {

    pub fn new(has_lichess_db: bool) -> Self {
        Self {
            window_id: None,
            has_db: has_lichess_db,
            from_square: None,
            board: Board::default(),
            last_move_from: None,
            last_move_to: None,
            hint_square: None,

            analysis: Game::new(),
            analysis_history: vec![Board::default()],
            engine_state: EngineStatus::TurnedOff,
            engine_eval: String::new(),
            engine: Engine::new(
                config::SETTINGS.engine_path.clone(),
                config::SETTINGS.engine_limit.clone(),
                String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
            ),
            engine_sender: None,
            engine_move: String::new(),

            downloading_db: false,
            download_progress: String::new(),
            puzzle_status: lang::tr(&config::SETTINGS.lang, "use_search"),
            puzzle_number_ui: String::from("1"),
            current_favorite: None,
            favorite_generation: 0,
            search_tab: SearchTab::new(),
            settings_tab: SettingsTab::new(),
            puzzle_tab: PuzzleTab::new(),
            project_tab: ProjectTab::new(),
            active_tab: TabId::Search,

            game_mode: config::GameMode::Puzzle,
            sound_playback: SoundPlayback::init_sound(),
            lang: config::SETTINGS.lang,
            mini_ui: false,
            square_ids: gen_square_hashmap(),
            board_btn_ids: gen_board_button_ids(),
            piece_imgs: get_image_handles(&config::SETTINGS.piece_theme),
        }
    }

    fn verify_and_make_move(&mut self, from: Square, to: Square) -> bool {
        let mut current_puzzle_changed = false;
        let side =
        match self.game_mode {
            config::GameMode::Analysis => { self.analysis.side_to_move() }
            config::GameMode::Puzzle => { self.board.side_to_move() }
        };
        let color =
            match self.game_mode {
                config::GameMode::Analysis => { self.analysis.current_position().color_on(to) }
                config::GameMode::Puzzle => { self.board.color_on(to) }
            };
        // If the user clicked on another piece of his own side,
        // just replace the previous selection and exit
        if self.puzzle_tab.game_status == GameStatus::Playing && color == Some(side) {
            self.from_square = Some(to);
            return false;
        }
        self.from_square = None;

        if self.game_mode == config::GameMode::Analysis {
            let move_made_notation =
                get_notation_string(self.analysis.current_position(), self.search_tab.piece_to_promote_to, from, to);

            let move_made = ChessMove::new(
                Square::from_str(&String::from(&move_made_notation[..2])).unwrap(),
                Square::from_str(&String::from(&move_made_notation[2..4])).unwrap(), PuzzleTab::check_promotion(&move_made_notation));

            if self.analysis.make_move(move_made) {
                self.analysis_history.push(self.analysis.current_position());
                self.engine.position = self.analysis.current_position().to_string();
                if let Some(sender) = &self.engine_sender
                    && let Err(e) = sender.blocking_send(san_correct_ep(self.analysis.current_position().to_string())) {
                        eprintln!("Lost contact with the engine: {}", e);
                }
                if self.settings_tab.saved_configs.play_sound
                    && let Some(audio) = &self.sound_playback {
                        audio.play_audio(SoundPlayback::ONE_PIECE_SOUND);
                }
            }
        } else if !self.puzzle_tab.puzzles.is_empty() {
            let movement;
            let move_made_notation =
                get_notation_string(self.board, self.search_tab.piece_to_promote_to, from, to);

            let move_made = ChessMove::new(
                Square::from_str(&String::from(&move_made_notation[..2])).unwrap(),
                Square::from_str(&String::from(&move_made_notation[2..4])).unwrap(), PuzzleTab::check_promotion(&move_made_notation));

            let is_mate = self.board.legal(move_made) && self.board.make_move_new(move_made).status() == BoardStatus::Checkmate;

            let correct_moves : Vec<&str> = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle].moves.split_whitespace().collect::<Vec<&str>>();
            let correct_move = ChessMove::new(
                Square::from_str(&String::from(&correct_moves[self.puzzle_tab.current_puzzle_move][..2])).unwrap(),
                Square::from_str(&String::from(&correct_moves[self.puzzle_tab.current_puzzle_move][2..4])).unwrap(), PuzzleTab::check_promotion(correct_moves[self.puzzle_tab.current_puzzle_move]));

            // If the move is correct we can apply it to the board
            if is_mate || (move_made == correct_move) {

                self.board = self.board.make_move_new(move_made);
                self.analysis_history.push(self.board);

                self.puzzle_tab.current_puzzle_move += 1;

                if self.puzzle_tab.current_puzzle_move == correct_moves.len() {
                    if self.settings_tab.saved_configs.play_sound
                        && let Some(audio) = &self.sound_playback {
                            audio.play_audio(SoundPlayback::ONE_PIECE_SOUND);
                    }
                    if self.puzzle_tab.current_puzzle < self.puzzle_tab.puzzles.len() - 1 {
                        if self.settings_tab.saved_configs.auto_load_next {
                            self.load_puzzle(true);
                            current_puzzle_changed = true;
                        } else {
                            self.puzzle_tab.game_status = GameStatus::PuzzleEnded;
                            self.puzzle_status = lang::tr(&self.lang, "correct_puzzle");
                        }
                    } else {
                        if self.settings_tab.saved_configs.auto_load_next {
                            self.board = Board::default();
                            // quite meaningless but allows the user to use the takeback button
                            // to analyze a full game in analysis mode after the puzzles ended.
                            self.analysis_history = vec![self.board];
                            self.puzzle_tab.current_puzzle_move = 1;
                            self.puzzle_tab.game_status = GameStatus::NoPuzzles;
                            self.current_favorite = None;
                            self.refresh_current_puzzle_review();
                        } else {
                            self.puzzle_tab.game_status = GameStatus::PuzzleEnded;
                        }
                        self.last_move_from = None;
                        self.last_move_to = None;
                        self.puzzle_status = lang::tr(&self.lang, "all_puzzles_done");
                    }
                } else {
                    if self.settings_tab.saved_configs.play_sound
                        && let Some(audio) = &self.sound_playback {
                            audio.play_audio(SoundPlayback::TWO_PIECE_SOUND);
                    }
                    movement = ChessMove::new(
                        Square::from_str(&String::from(&correct_moves[self.puzzle_tab.current_puzzle_move][..2])).unwrap(),
                        Square::from_str(&String::from(&correct_moves[self.puzzle_tab.current_puzzle_move][2..4])).unwrap(), PuzzleTab::check_promotion(correct_moves[self.puzzle_tab.current_puzzle_move]));

                    self.last_move_from = Some(movement.get_source());
                    self.last_move_to = Some(movement.get_dest());

                    self.board = self.board.make_move_new(movement);
                    self.analysis_history.push(self.board);

                    self.puzzle_tab.current_puzzle_move += 1;
                    self.puzzle_status = lang::tr(&self.lang, "correct_move");
                }
            } else {
                #[allow(clippy::collapsible_else_if)]
                if self.board.side_to_move() == Color::White {
                    self.puzzle_status = lang::tr(&self.lang, "wrong_move_white_play");
                } else {
                    self.puzzle_status = lang::tr(&self.lang, "wrong_move_black_play");
                }
            }
        }
        current_puzzle_changed
    }

    fn load_puzzle(&mut self, inc_counter: bool) {
        self.hint_square = None;
        self.puzzle_tab.current_puzzle_move = 1;
        if inc_counter {
            self.inc_puzzle_counter();
        }
        let puzzle_moves: Vec<&str> = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle].moves.split_whitespace().collect();

        // The opponent's last move (before the puzzle starts)
        // is in the "moves" field of the cvs, so we need to apply it.
        self.board = Board::from_str(&self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle].fen).unwrap();

        let movement = ChessMove::new(
            Square::from_str(&String::from(&puzzle_moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&puzzle_moves[0][2..4])).unwrap(), PuzzleTab::check_promotion(puzzle_moves[0]));

        self.last_move_from = Some(movement.get_source());
        self.last_move_to = Some(movement.get_dest());

        self.board = self.board.make_move_new(movement);
        self.analysis_history = vec![self.board];

        if self.board.side_to_move() == Color::White {
            self.puzzle_status = lang::tr(&self.lang, "white_to_move");
        } else {
            self.puzzle_status = lang::tr(&self.lang, "black_to_move");
        }

        self.puzzle_tab.current_puzzle_side = self.board.side_to_move();
        self.puzzle_tab.current_puzzle_fen = san_correct_ep(self.board.to_string());
        self.puzzle_tab.game_status = GameStatus::Playing;
        self.game_mode = config::GameMode::Puzzle;
        self.refresh_current_puzzle_review();
    }

    fn replace_puzzle_batch(&mut self, mut puzzles: Vec<config::Puzzle>, shuffle: bool) {
        if shuffle {
            puzzles.shuffle(&mut rng());
        }
        self.puzzle_tab.puzzles = puzzles;
        self.puzzle_tab.current_puzzle = 0;
        self.puzzle_number_ui = String::from("1");
        self.load_puzzle(false);
    }

    fn current_reviewable_puzzle(&self) -> Option<&config::Puzzle> {
        (self.puzzle_tab.game_status != GameStatus::NoPuzzles)
            .then(|| self.puzzle_tab.puzzles.get(self.puzzle_tab.current_puzzle))
            .flatten()
    }

    fn refresh_current_puzzle_review(&mut self) {
        let puzzle = self.current_reviewable_puzzle().cloned();
        self.project_tab.refresh_puzzle_review(puzzle.as_ref());
    }

    fn favorite_error_status(&self, error: &str) -> String {
        format!("{}: {error}", lang::tr(&self.lang, "my_favories"))
    }

    fn current_puzzle_matches(&self, puzzle_id: &str) -> bool {
        self.current_reviewable_puzzle()
            .is_some_and(|puzzle| puzzle.puzzle_id == puzzle_id)
    }

    fn next_favorite_generation(&mut self) -> u64 {
        self.favorite_generation = self.favorite_generation.wrapping_add(1);
        self.favorite_generation
    }

    fn favorite_response_is_current(&self, puzzle_id: &str, generation: u64) -> bool {
        self.favorite_generation == generation && self.current_puzzle_matches(puzzle_id)
    }

    fn refresh_current_favorite_status(&mut self) -> Task<Message> {
        let Some(puzzle_id) = self
            .current_reviewable_puzzle()
            .map(|puzzle| puzzle.puzzle_id.clone())
        else {
            self.current_favorite = None;
            return Task::none();
        };
        let generation = self.next_favorite_generation();
        self.current_favorite = None;

        Task::perform(
            {
                let puzzle_id = puzzle_id.clone();
                async move { db::is_favorite(&puzzle_id) }
            },
            move |result| Message::FavoriteStatusLoaded {
                puzzle_id,
                generation,
                result,
            },
        )
    }

    fn inc_puzzle_counter(&mut self) {
        self.puzzle_tab.current_puzzle += 1;
        self.puzzle_number_ui = (self.puzzle_tab.current_puzzle + 1).to_string();
    }

    // Redundant, but just to make the function names clear
    fn dec_puzzle_counter(&mut self) {
        self.puzzle_tab.current_puzzle -= 1;
        self.puzzle_number_ui = (self.puzzle_tab.current_puzzle + 1).to_string();
    }

    // Old Iced application trait stuff
    fn init() -> (Self, Task<Message>) {
        let has_lichess_db = config::puzzle_source_exists(&config::SETTINGS);
        (
            Self::new(has_lichess_db),
            Task::discard(iced::font::load(Cow::from(config::CHESS_ALPHA_BYTES))).chain(window::latest())
            .map(Message::WindowInitialized)
        )
    }

    fn update(&mut self, message: self::Message) -> Task<Message> {
        match (self.from_square, message) {
            (None, Message::SelectSquare(pos)) => {
                let side =
                    match self.game_mode {
                        config::GameMode::Analysis => { self.analysis.side_to_move() }
                        config::GameMode::Puzzle => { self.board.side_to_move() }
                    };
                let color =
                    match self.game_mode {
                        config::GameMode::Analysis => { self.analysis.current_position().color_on(pos) }
                        config::GameMode::Puzzle => { self.board.color_on(pos) }
                    };

                if (self.puzzle_tab.game_status == GameStatus::Playing || self.game_mode == config::GameMode::Analysis) && color == Some(side) {
                    self.hint_square = None;
                    self.from_square = Some(pos);
                }
                Task::none()
            } (Some(from), Message::SelectSquare(to)) if from != to => {
                if self.verify_and_make_move(from, to) {
                    self.refresh_current_favorite_status()
                } else {
                    Task::none()
                }
            } (Some(_), Message::SelectSquare(to)) => {
                self.from_square = Some(to);
                Task::none()
            } (_, Message::TabSelected(selected)) => {
                self.active_tab = selected;
                Task::none()
            } (_, Message::Settings(message)) => {
                self.settings_tab.update(message)
            } (_, Message::Project(message)) => {
                let task = self.project_tab.update(message);
                self.refresh_current_puzzle_review();
                task
            } (_, Message::PuzzleSqliteSourceSelected) => {
                self.has_db = config::puzzle_source_exists(&self.settings_tab.saved_configs);
                Task::none()
            } (_, Message::SelectMode(message)) => {
                self.game_mode = message;
                if message == config::GameMode::Analysis {
                    self.analysis = Game::new_with_board(self.board);
                } else {
                    if self.engine_state != EngineStatus::TurnedOff
                        && let Some(sender) = &self.engine_sender {
                            sender.blocking_send(String::from(eval::STOP_COMMAND)).expect("Error stopping engine.");
                    }
                    self.analysis_history.truncate(self.puzzle_tab.current_puzzle_move);
                }
                Task::none()
            } (_, Message::ShowHint) => {
                let moves = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle].moves.split_whitespace().collect::<Vec<&str>>();
                if !moves.is_empty() && moves.len() > self.puzzle_tab.current_puzzle_move {
                    self.hint_square = Some(Square::from_str(&moves[self.puzzle_tab.current_puzzle_move][..2]).unwrap());
                } else {
                    self.hint_square = None;
                }

                Task::none()
            } (_, Message::ShowNextPuzzle) => {
                self.inc_puzzle_counter();
                self.load_puzzle(false);
                self.refresh_current_favorite_status()
            } (_, Message::ShowPreviousPuzzle) => {
                if self.puzzle_tab.current_puzzle > 0 && self.game_mode == config::GameMode::Puzzle {
                    self.dec_puzzle_counter();
                    self.load_puzzle(false);
                    self.refresh_current_favorite_status()
                } else {
                    Task::none()
                }
            } (_, Message::GoBackMove) => {
                if self.game_mode == config::GameMode::Analysis && self.analysis_history.len() > self.puzzle_tab.current_puzzle_move {
                    self.analysis_history.pop();
                    self.analysis = Game::new_with_board(*self.analysis_history.last().unwrap());
                    if let Some(sender) = &self.engine_sender
                        && let Err(e) = sender.blocking_send(san_correct_ep(self.analysis.current_position().to_string())) {
                            eprintln!("Lost contact with the engine: {}", e);
                    }
                }
                Task::none()
            } (_, Message::RedoPuzzle) => {
                self.load_puzzle(false);
                Task::none()
            } (_, Message::LoadProjectPuzzles(puzzles_vec)) => {
                if puzzles_vec.is_empty() {
                    return Task::none();
                }
                self.from_square = None;
                self.game_mode = config::GameMode::Puzzle;
                if self.engine_state != EngineStatus::TurnedOff
                    && let Some(sender) = &self.engine_sender {
                        sender.blocking_send(String::from(eval::STOP_COMMAND)).expect("Error stopping engine.");
                }
                self.replace_puzzle_batch(puzzles_vec, false);
                self.refresh_current_favorite_status()
            } (_, Message::LoadPuzzle(puzzles_vec)) => {
                self.from_square = None;
                self.search_tab.show_searching_msg = false;
                self.game_mode = config::GameMode::Puzzle;
                if self.engine_state != EngineStatus::TurnedOff
                    && let Some(sender) = &self.engine_sender {
                        sender.blocking_send(String::from(eval::STOP_COMMAND)).expect("Error stopping engine.");
                }
                if let Some(puzzles_vec) = puzzles_vec {
                    if !puzzles_vec.is_empty() {
                        self.replace_puzzle_batch(puzzles_vec, true);
                        return self.refresh_current_favorite_status();
                    } else {
                        // Just putting the default position to make it obvious the search ended.
                        self.board = Board::default();
                        self.last_move_from = None;
                        self.last_move_to = None;
                        self.puzzle_tab.game_status = GameStatus::NoPuzzles;
                        self.puzzle_status = lang::tr(&self.lang, "no_puzzle_found");
                        self.current_favorite = None;
                    }
                } else {
                    self.board = Board::default();
                    self.last_move_from = None;
                    self.last_move_to = None;
                    self.puzzle_tab.game_status = GameStatus::NoPuzzles;
                    self.puzzle_status = lang::tr(&self.lang, "no_puzzle_found");
                    self.current_favorite = None;
                }
                self.refresh_current_puzzle_review();
                Task::none()
            } (_, Message::LoadFavorites(result)) => {
                self.search_tab.show_searching_msg = false;
                match result {
                    Ok(puzzles_vec) => {
                        self.from_square = None;
                        self.game_mode = config::GameMode::Puzzle;
                        if self.engine_state != EngineStatus::TurnedOff
                            && let Some(sender) = &self.engine_sender {
                                if let Err(error) = sender.blocking_send(String::from(eval::STOP_COMMAND)) {
                                    eprintln!("Lost contact with the engine: {error}");
                                }
                        }
                        if !puzzles_vec.is_empty() {
                            self.replace_puzzle_batch(puzzles_vec, true);
                            self.refresh_current_favorite_status()
                        } else {
                            self.board = Board::default();
                            self.last_move_from = None;
                            self.last_move_to = None;
                            self.puzzle_tab.game_status = GameStatus::NoPuzzles;
                            self.puzzle_status = lang::tr(&self.lang, "no_puzzle_found");
                            self.current_favorite = None;
                            self.refresh_current_puzzle_review();
                            Task::none()
                        }
                    }
                    Err(error) => {
                        self.puzzle_status = self.favorite_error_status(&error);
                        Task::none()
                    }
                }
            } (_, Message::FavoriteStatusLoaded { puzzle_id, generation, result }) => {
                if self.favorite_response_is_current(&puzzle_id, generation) {
                    match result {
                        Ok(is_favorite) => self.current_favorite = Some(is_favorite),
                        Err(error) => {
                            self.current_favorite = None;
                            self.puzzle_status = self.favorite_error_status(&error);
                        }
                    }
                }
                Task::none()
            } (_, Message::FavoriteToggled { puzzle_id, generation, result }) => {
                if self.favorite_response_is_current(&puzzle_id, generation) {
                    match result {
                        Ok(is_favorite) => self.current_favorite = Some(is_favorite),
                        Err(error) => {
                            self.current_favorite = None;
                            self.puzzle_status = self.favorite_error_status(&error);
                        }
                    }
                }
                Task::none()
            } (_, Message::ChangeSettings(message)) => {
                if let Some(settings) = message {
                    self.search_tab.piece_theme_promotion = self.settings_tab.piece_theme;
                    self.engine.engine_path = self.settings_tab.engine_path.clone();
                    self.lang = settings.lang;
                    self.search_tab.lang = self.lang;
                    self.search_tab.theme.lang = self.lang;
                    self.search_tab.opening.lang = self.lang;
                    self.puzzle_tab.lang = self.lang;
                    self.project_tab.lang = self.lang;
                    self.settings_tab.saved_configs = settings;
                    self.piece_imgs = get_image_handles(&self.settings_tab.piece_theme);
                    self.search_tab.promotion_piece_img = search_tab::gen_piece_vec(&self.settings_tab.piece_theme);
                }
                Task::none()
            }
             (_, Message::PuzzleInfo(message)) => {
                self.puzzle_tab.update(message)
            } (_, Message::Search(SearchMesssage::ClickSearch)) if !self.search_tab.is_favorites() => {
                match self.project_tab.reviewed_puzzle_ids_for_active_chapter() {
                    Ok(excluded_ids) => self.search_tab.start_lichess_search(
                        excluded_ids.unwrap_or_default(),
                    ),
                    Err(error) => {
                        self.search_tab.show_searching_msg = false;
                        self.puzzle_status = format!("{}: {error}", lang::tr(&self.lang, "review_error"));
                        Task::none()
                    }
                }
            } (_, Message::Search(message)) => {
                self.search_tab.update(message)
            } (_, Message::PuzzleInputIndexChange(puzzle_input)) => {
                self.puzzle_number_ui = puzzle_input;
                Task::none()
            } (_, Message::JumpToPuzzle) => {
                // Test if puzzle index typed is valid
                let puzzle_index = self.puzzle_number_ui.parse::<usize>();
                if let Ok(index) = puzzle_index
                    && index > 0 && index <= self.puzzle_tab.puzzles.len() {
                        // The user typed value starts on 1, not zero, so we subtract 1
                        self.puzzle_tab.current_puzzle = index - 1;
                }
                self.load_puzzle(false);
                self.refresh_current_favorite_status()
            } (_, Message::SetPuzzleReview(decision)) => {
                if let Some(puzzle) = self.current_reviewable_puzzle().cloned() {
                    self.project_tab.set_puzzle_review(&puzzle, decision);
                }
                Task::none()
            } (_, Message::ClearPuzzleReview) => {
                if let Some(puzzle) = self.current_reviewable_puzzle().cloned() {
                    self.project_tab.clear_puzzle_review(&puzzle);
                }
                Task::none()
            } (_, Message::ScreenshotCreated(screenshot)) => {
                Task::perform(screenshot_save_dialog(screenshot), Message::SaveScreenshot)
            } (_, Message::SaveScreenshot(img_and_path)) => {
                let (crop_height, crop_width) = if self.settings_tab.show_coordinates {
                    (self.settings_tab.window_height - 125., self.settings_tab.window_height - 130.)
                } else {
                    (self.settings_tab.window_height - 135., self.settings_tab.window_height - 135.)
                };
                if let Some(img_and_path) = img_and_path {
                    let screenshot = img_and_path.0;
                    let path = img_and_path.1;
                    let crop = screenshot.crop(Rectangle::<u32> {
                        x: 0,
                        y: 0,
                        width: crop_width as u32,
                        height: crop_height as u32,
                    });
                    if let Ok (screenshot) = crop {
                        let img = RgbaImage::from_raw(screenshot.size.width, screenshot.size.height, screenshot.rgba.to_vec());
                        if let Some(image) = img {
                            let rgb_img = DynamicImage::ImageRgba8(image).into_rgb8();
                            let _ = rgb_img.save_with_format(path, image::ImageFormat::Jpeg);
                        }
                    }
                }
                Task::none()
            } (_, Message::ExportPDF(file_path)) => {
                if let Some(file_path) = file_path {
                    export::to_pdf(&self.puzzle_tab.puzzles, self.settings_tab.export_pgs.parse::<i32>().unwrap(), &self.lang, file_path);
                }
                Task::none()
            } (_, Message::ExportPGN(file_path)) => {
                if let Some(file_path) = file_path {
                    export::to_pgn(&self.puzzle_tab.puzzles, &self.lang, file_path);
                }
                Task::none()
            } (_, Message::EventOccurred(event)) => {
                if let Event::Window(window::Event::CloseRequested) = event {
                    match self.engine_state {
                        EngineStatus::TurnedOff => {
                            iced::window::is_maximized(self.window_id.unwrap()).map(Message::SaveMaximizedStatusAndExit)
                        } _ => {
                            if let Some(sender) = &self.engine_sender {
                                sender.blocking_send(String::from(eval::EXIT_APP_COMMAND)).expect("Error stopping engine.");
                            }
                            Task::none()
                        }
                    }
                } else if let Event::Window(window::Event::Resized(size)) = event {
                    if !self.mini_ui {
                        self.settings_tab.window_width = size.width;
                        self.settings_tab.window_height = size.height;
                    }
                    Task::none()
                } else {
                    Task::none()
                }
            } (_, Message::SaveMaximizedStatusAndExit(is_maximized)) => {
                self.settings_tab.maximized = is_maximized;
                self.settings_tab.save_window_size();
                window::close(self.window_id.unwrap())
            } (_, Message::EngineFileChosen(engine_path)) => {
                if let Some(engine_path) = engine_path {
                    self.settings_tab.engine_path = engine_path.clone();
                    self.engine.engine_path = engine_path;
                }
                Task::none()
            } (_, Message::StartEngine) => {
                match self.engine_state {
                    EngineStatus::TurnedOff => {
                        //Check if the path is correct first
                        if Path::new(&self.engine.engine_path).exists() {
                            self.engine.position = san_correct_ep(self.analysis.current_position().to_string());
                            self.engine_state = EngineStatus::Started;
                        }
                    } _ => {
                        if let Some(sender) = &self.engine_sender {
                            sender.blocking_send(String::from(eval::STOP_COMMAND)).expect("Error stopping engine.");
                            self.engine_sender = None;
                        }
                    }
                }
                Task::none()
            } (_, Message::EngineStopped(exit)) => {
                self.engine_state = EngineStatus::TurnedOff;
                if exit {
                    self.settings_tab.save_window_size();
                    window::close(self.window_id.unwrap())
                } else {
                    self.engine_eval = String::new();
                    self.engine_move = String::new();
                    Task::none()
                }
            } (_, Message::EngineReady(sender)) => {
                self.engine_sender = Some(sender);
                Task::none()
            } (_, Message::UpdateEval(eval)) => {
                match self.engine_state {
                    EngineStatus::TurnedOff => {
                        Task::none()
                    } _ => {
                        let (eval, best_move) = eval;
                        if let Some(eval_str) = eval {
                            if eval_str.contains("Mate") {
                                let tokens: Vec<&str> = eval_str.split_whitespace().collect();
                                let distance_to_mate_num = tokens[2].parse::<i32>().unwrap();
                                match distance_to_mate_num {
                                    1.. => {
                                        self.engine_eval = lang::tr(&self.lang, "mate_in") + &distance_to_mate_num.to_string();
                                    } 0 => {
                                        self.engine_eval = lang::tr(&self.lang, "mate");
                                        self.engine_move = String::from("");
                                        return Task::none();
                                    } _ => {
                                        self.engine_eval = lang::tr(&self.lang, "mate_in") + &(-distance_to_mate_num).to_string();
                                    }
                                };
                            } else if self.analysis.side_to_move() == Color::White {
                                self.engine_eval = eval_str;
                            } else {
                                // Invert to keep the values relative to white,
                                // like it's usually done in GUIs.
                                let eval = (-eval_str.parse::<f32>().unwrap()).to_string();
                                self.engine_eval = eval.to_string().clone();
                            }
                        }
                        if let Some(best_move) = best_move
                            && let Some(best_move) = config::coord_to_san(&self.analysis.current_position(), best_move, &self.lang) {
                                self.engine_move = best_move;
                        }
                        Task::none()
                    }
                }
            } (_, Message::StartDBDownload) => {
                if self.settings_tab.is_using_sqlite_puzzles() {
                    let _ = self.settings_tab.update(SettingsMessage::UseCsvPuzzles);
                    if self.settings_tab.is_using_sqlite_puzzles() {
                        return Task::none();
                    }
                }
                self.downloading_db = true;
                Task::none()
            } (_, Message::DBDownloadFinished) => {
                self.downloading_db = false;
                self.has_db = true;
                Task::none()
            } (_, Message::DownloadProgress(progress)) => {
                self.download_progress = progress;
                Task::none()
            } (_, Message::FavoritePuzzle) => {
                let Some(puzzle) = self.current_reviewable_puzzle().cloned() else {
                    return Task::none();
                };
                if self.current_favorite.is_none() {
                    return Task::none();
                }

                let puzzle_id = puzzle.puzzle_id.clone();
                let generation = self.next_favorite_generation();
                self.current_favorite = None;
                Task::perform(
                    async move { db::toggle_favorite(puzzle) },
                    move |result| Message::FavoriteToggled {
                        puzzle_id,
                        generation,
                        result,
                    },
                )
            } (_, Message::WindowInitialized(id)) => {
                self.window_id = id;
                self.puzzle_tab.window_id = id;
                iced::window::maximize(self.window_id.unwrap(), self.settings_tab.maximized)
            } (_, Message::MinimizeUI) => {
                if self.mini_ui {
                    self.mini_ui = false;
                    let new_size = Size::new(self.settings_tab.window_width, self.settings_tab.window_height);
                    iced::window::resize(self.window_id.unwrap(), new_size)
                } else {
                    self.mini_ui = true;
                    let new_size =
                        // "110" accounts for the buttons below the board, since the board
                        // is a square, we make the width the same as the height,
                        // with just a bit extra for the > button
                        Size::new((self.settings_tab.window_height - 120.) + 25.,
                        self.settings_tab.window_height);
                    iced::window::resize(self.window_id.unwrap(), new_size)
                }
            } (_, Message::DropPiece(square, cursor_pos, _bounds)) => {
                if self.puzzle_tab.game_status == GameStatus::Playing || self.game_mode == config::GameMode::Analysis {
                    iced_drop::zones_on_point(
                        move |zones| Message::HandleDropZones(square, zones),
                        cursor_pos,
                        None,
                        None,
                    )
                } else {
                    Task::none()
                }
            } (_, Message::HandleDropZones(from, zones)) => {
                if !zones.is_empty() {
                    let id: &GenericId = &zones[0].0.clone();
                    if let Some(to) = self.square_ids.get(id) {
                        if self.verify_and_make_move(from, *to) {
                            return self.refresh_current_favorite_status();
                        }
                    }
                }
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        match self.engine_state {
            EngineStatus::TurnedOff => {
                if self.downloading_db {
                    Subscription::batch(vec![
                        download_lichess_db(),
                        event::listen().map(Message::EventOccurred)
                    ])
                } else {
                    event::listen().map(Message::EventOccurred)
                }
            } _ => {
                Subscription::batch(vec![
                    Engine::run_engine(self.engine.clone()),
                    event::listen().map(Message::EventOccurred)
                ])
            }
        }
    }

    fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.has_db {
            let has_previous = !self.puzzle_tab.puzzles.is_empty() && self.puzzle_tab.current_puzzle > 0;
            let has_more_puzzles = !self.puzzle_tab.puzzles.is_empty() && self.puzzle_tab.current_puzzle < self.puzzle_tab.puzzles.len() - 1;
            let is_fav = self.current_reviewable_puzzle().and(self.current_favorite);
            let puzzle_review = self
                .current_reviewable_puzzle()
                .and_then(|_| self.project_tab.review_view());
            let resp = responsive(move |size| {
                gen_view(
                    self.game_mode,
                    self.puzzle_tab.current_puzzle_side,
                    self.settings_tab.flip_board,
                    self.settings_tab.show_coordinates,
                    &self.board,
                    &self.analysis.current_position(),
                    self.from_square,
                    self.last_move_from,
                    self.last_move_to,
                    self.hint_square,
                    self.settings_tab.saved_configs.piece_theme,
                    self.settings_tab.board_theme,
                    &self.puzzle_status,
                    is_fav,
                    has_more_puzzles,
                    has_previous,
                    self.analysis_history.len(),
                    &self.puzzle_number_ui,
                    self.puzzle_tab.puzzles.len(),
                    self.puzzle_tab.current_puzzle_move,
                    self.puzzle_tab.game_status,
                    puzzle_review.clone(),
                    &self.active_tab,
                    &self.engine_eval,
                    &self.engine_move,

                    self.engine_state != EngineStatus::TurnedOff,
                    self.search_tab.tab_label(),
                    self.settings_tab.tab_label(),
                    self.puzzle_tab.tab_label(),
                    self.project_tab.tab_label(),
                    self.search_tab.view(),
                    self.settings_tab.view(),
                    self.puzzle_tab.view(),
                    self.project_tab.view(),
                    &self.lang,
                    size,
                    self.mini_ui,
                    &self.board_btn_ids,
                    &self.piece_imgs,
                )});
            Container::new(resp)
                .padding(1)
                .into()
        } else {
            let mut col = Column::new()
                .push(
                    container(
                        Text::new(lang::tr(&self.lang, "db_not_found"))
                        .size(30)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center))
                    )
                .push(
                    Text::new(lang::tr(&self.lang, "do_you_wanna_download"))
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center))
                .push(
                    Text::new(lang::tr(&self.lang, "download_size_info"))
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center));
            if self.downloading_db {
                col = col
                    .push(
                        container(button(Text::new(lang::tr(&self.lang, "downloading")))).width(Length::Fill).center_x(Length::Fill).padding(20)
                    )
                    .push(Text::new(&self.download_progress)
                        .size(20)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                    );
            } else {
                col = col
                    .push(
                        container(
                            button(Text::new(lang::tr(&self.lang, "download_btn")))
                                .on_press(Message::StartDBDownload)
                        )
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(20),
                    )
                    .push(
                        container(
                            button(Text::new(lang::tr(&self.lang, "select_puzzle_sqlite_db")))
                                .on_press(Message::Settings(
                                    SettingsMessage::SelectPuzzleSqlitePressed,
                                ))
                        )
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(20),
                    )
                    .push(
                        Text::new(self.settings_tab.status())
                            .width(Length::Fill)
                            .align_x(alignment::Horizontal::Center),
                    );
            };
            center(col)
                .padding(1)
                .into()
        }
    }

    fn theme(&self) -> iced::Theme {
        iced::Theme::custom(
            String::from("Theme"),
            self.settings_tab.interface_theme.palette(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_material_studio::models::Puzzle as PersistentPuzzle;
    use chess_material_studio::project::{create_chapter, create_project, set_puzzle_decision};
    use crate::search_tab::SearchBase;
    use std::path::PathBuf;
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
                .join("cms_023e_main_tests")
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

    fn navigation_puzzle(id: &str) -> config::Puzzle {
        config::Puzzle {
            puzzle_id: id.into(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".into(),
            moves: "a1a2".into(),
            rating: 1500,
            rating_deviation: 80,
            popularity: 50,
            nb_plays: 10,
            themes: "hangingPiece".into(),
            game_url: "https://lichess.org/game".into(),
            opening: String::new(),
        }
    }

    fn auto_advance_puzzle(id: &str) -> config::Puzzle {
        config::Puzzle {
            puzzle_id: id.into(),
            fen: "8/8/8/8/8/8/8/K6k b - - 0 1".into(),
            moves: "h1h2 a1a2".into(),
            rating: 1500,
            rating_deviation: 80,
            popularity: 50,
            nb_plays: 10,
            themes: "hangingPiece".into(),
            game_url: "https://lichess.org/game".into(),
            opening: String::new(),
        }
    }

    fn persistent_puzzle(puzzle: &config::Puzzle) -> PersistentPuzzle {
        PersistentPuzzle {
            puzzle_id: puzzle.puzzle_id.clone(),
            fen: puzzle.fen.clone(),
            moves: puzzle.moves.clone(),
            rating: puzzle.rating,
            rating_deviation: puzzle.rating_deviation,
            popularity: puzzle.popularity,
            nb_plays: puzzle.nb_plays,
            themes: puzzle.themes.clone(),
            game_url: puzzle.game_url.clone(),
            opening: puzzle.opening.clone(),
        }
    }

    fn assert_current_review(
        app: &OfflinePuzzles,
        puzzle_id: &str,
        decision: ProjectPuzzleDecision,
    ) {
        assert_eq!(app.current_reviewable_puzzle().unwrap().puzzle_id, puzzle_id);
        assert_eq!(app.project_tab.cached_review_puzzle_id(), Some(puzzle_id));
        assert_eq!(app.project_tab.review_view().unwrap().decision, Some(decision));
    }

    #[test]
    fn unavailable_puzzle_source_keeps_startup_in_missing_source_state() {
        let config = config::OfflinePuzzlesConfig {
            puzzle_db_location: "/nonexistent/lichess-puzzles.csv".into(),
            puzzle_sqlite_location: None,
            ..config::OfflinePuzzlesConfig::default()
        };

        assert!(!config::puzzle_source_exists(&config));
    }

    #[test]
    fn no_puzzle_state_is_not_reviewable_or_actionable() {
        let mut app = OfflinePuzzles::new(false);
        app.puzzle_tab.puzzles = vec![config::Puzzle::default()];
        app.puzzle_tab.current_puzzle = 0;
        app.puzzle_tab.game_status = GameStatus::NoPuzzles;

        assert!(app.current_reviewable_puzzle().is_none());
        let _ = app.update(Message::SetPuzzleReview(ProjectPuzzleDecision::Selected));
        let _ = app.update(Message::ClearPuzzleReview);

        assert!(app.project_tab.review_view().is_none());
    }

    #[test]
    fn previous_next_and_jump_refresh_the_review_for_the_current_puzzle() {
        let project = TempProjectDb::new("review-navigation");
        create_project(&project.path, "Navegación").unwrap();
        let chapter = create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzles = vec![
            navigation_puzzle("cms-023e-first"),
            navigation_puzzle("cms-023e-second"),
            navigation_puzzle("cms-023e-third"),
        ];
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &persistent_puzzle(&puzzles[0]),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &persistent_puzzle(&puzzles[1]),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &persistent_puzzle(&puzzles[2]),
            ProjectPuzzleDecision::Discarded,
        )
        .unwrap();
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));
        app.puzzle_tab.puzzles = puzzles;
        app.puzzle_tab.current_puzzle = 0;
        app.load_puzzle(false);
        assert_current_review(&app, "cms-023e-first", ProjectPuzzleDecision::Selected);

        let _ = app.update(Message::ShowNextPuzzle);
        assert_current_review(&app, "cms-023e-second", ProjectPuzzleDecision::Discarded);

        let _ = app.update(Message::ShowPreviousPuzzle);
        assert_current_review(&app, "cms-023e-first", ProjectPuzzleDecision::Selected);

        let _ = app.update(Message::PuzzleInputIndexChange("3".into()));
        let _ = app.update(Message::JumpToPuzzle);
        assert_current_review(&app, "cms-023e-third", ProjectPuzzleDecision::Discarded);
    }

    #[test]
    fn project_puzzle_batch_preserves_order_initializes_the_board_and_refreshes_review() {
        let project = TempProjectDb::new("project-load-batch");
        create_project(&project.path, "Proyecto").unwrap();
        let chapter = create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzles = vec![
            navigation_puzzle("cms-023h-first"),
            navigation_puzzle("cms-023h-second"),
            navigation_puzzle("cms-023h-third"),
        ];
        for puzzle in &puzzles {
            set_puzzle_decision(&project.path, chapter.id, &persistent_puzzle(puzzle), ProjectPuzzleDecision::Selected).unwrap();
        }
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(project.path.clone()))));

        let _ = app.update(Message::LoadProjectPuzzles(puzzles.clone()));

        assert_eq!(app.puzzle_tab.puzzles.iter().map(|puzzle| puzzle.puzzle_id.as_str()).collect::<Vec<_>>(), vec!["cms-023h-first", "cms-023h-second", "cms-023h-third"]);
        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.puzzle_number_ui, "1");
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert_eq!(app.board, Board::from_str(&puzzles[0].fen).unwrap().make_move_new(ChessMove::new(Square::A1, Square::A2, None)));
        assert_current_review(&app, "cms-023h-first", ProjectPuzzleDecision::Selected);

        let _ = app.update(Message::ShowNextPuzzle);
        assert_current_review(&app, "cms-023h-second", ProjectPuzzleDecision::Selected);
        let _ = app.update(Message::ShowPreviousPuzzle);
        assert_current_review(&app, "cms-023h-first", ProjectPuzzleDecision::Selected);
        let _ = app.update(Message::PuzzleInputIndexChange("3".into()));
        let _ = app.update(Message::JumpToPuzzle);
        assert_current_review(&app, "cms-023h-third", ProjectPuzzleDecision::Selected);
    }

    #[test]
    fn empty_project_puzzle_batch_keeps_the_loaded_snapshot_stable() {
        let mut app = OfflinePuzzles::new(false);
        let loaded = navigation_puzzle("cms-023h-existing");
        app.puzzle_tab.puzzles = vec![loaded.clone()];
        app.puzzle_tab.current_puzzle = 0;
        app.load_puzzle(false);
        let board = app.board;

        let _ = app.update(Message::LoadProjectPuzzles(Vec::new()));

        assert_eq!(app.puzzle_tab.puzzles.len(), 1);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.board, board);
    }

    #[test]
    fn review_mutation_does_not_remove_a_loaded_project_snapshot() {
        let project = TempProjectDb::new("project-load-mutation");
        create_project(&project.path, "Proyecto").unwrap();
        let chapter = create_chapter(&project.path, "Capítulo", None).unwrap();
        let puzzles = vec![navigation_puzzle("cms-023h-mutation-first"), navigation_puzzle("cms-023h-mutation-second")];
        for puzzle in &puzzles {
            set_puzzle_decision(&project.path, chapter.id, &persistent_puzzle(puzzle), ProjectPuzzleDecision::Selected).unwrap();
        }
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(project.path.clone()))));
        let _ = app.update(Message::LoadProjectPuzzles(puzzles.clone()));

        let _ = app.update(Message::SetPuzzleReview(ProjectPuzzleDecision::Discarded));

        assert_eq!(app.puzzle_tab.puzzles.len(), puzzles.len());
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, puzzles[0].puzzle_id);
        assert_eq!(app.puzzle_tab.puzzles[1].puzzle_id, puzzles[1].puzzle_id);
        assert_current_review(&app, "cms-023h-mutation-first", ProjectPuzzleDecision::Discarded);

        let _ = app.update(Message::ClearPuzzleReview);

        assert_eq!(app.puzzle_tab.puzzles.len(), puzzles.len());
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, puzzles[0].puzzle_id);
        assert_eq!(app.puzzle_tab.puzzles[1].puzzle_id, puzzles[1].puzzle_id);
        assert_eq!(app.project_tab.review_view().unwrap().decision, None);
    }

    #[test]
    fn search_preparation_fails_closed_for_review_read_errors_and_skips_favorites() {
        let project = TempProjectDb::new("search-exclusions-error");
        create_project(&project.path, "Proyecto").unwrap();
        create_chapter(&project.path, "Capítulo", None).unwrap();
        let mut app = OfflinePuzzles::new(false);
        let loaded = navigation_puzzle("already-loaded");
        app.puzzle_tab.puzzles = vec![loaded.clone()];
        app.puzzle_tab.current_puzzle = 0;
        app.puzzle_tab.game_status = GameStatus::Playing;
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));
        std::fs::remove_file(&project.path).unwrap();

        let _ = app.update(Message::Search(SearchMesssage::ClickSearch));

        assert!(!app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles.len(), 1);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
        assert!(app.puzzle_status.contains(&lang::tr(&app.lang, "review_error")));

        let _ = app.update(Message::Search(SearchMesssage::SelectBase(SearchBase::Favorites)));
        let _ = app.update(Message::Search(SearchMesssage::ClickSearch));
        assert!(app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles.len(), 1);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
    }

    #[test]
    fn lichess_search_without_a_project_starts_normally() {
        let mut app = OfflinePuzzles::new(false);

        let _ = app.update(Message::Search(SearchMesssage::ClickSearch));

        assert!(app.search_tab.show_searching_msg);
    }

    fn app_with_current_puzzle(id: &str) -> OfflinePuzzles {
        let mut app = OfflinePuzzles::new(false);
        app.puzzle_tab.puzzles = vec![navigation_puzzle(id)];
        app.puzzle_tab.current_puzzle = 0;
        app.load_puzzle(false);
        app
    }

    #[test]
    fn favorite_search_error_preserves_the_loaded_puzzle() {
        let mut app = app_with_current_puzzle("loaded-favorite");
        let board = app.board;
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadFavorites(Err("controlled search failure".into())));

        assert!(!app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "loaded-favorite");
        assert_eq!(app.board, board);
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert!(app.puzzle_status.contains(&lang::tr(&app.lang, "my_favories")));
        assert!(app.puzzle_status.contains("controlled search failure"));
    }

    #[test]
    fn empty_favorite_search_keeps_the_historical_no_puzzles_behavior() {
        let mut app = app_with_current_puzzle("loaded-favorite");

        let _ = app.update(Message::LoadFavorites(Ok(Vec::new())));

        assert_eq!(app.puzzle_tab.game_status, GameStatus::NoPuzzles);
        assert_eq!(app.puzzle_status, lang::tr(&app.lang, "no_puzzle_found"));
        assert_eq!(app.current_favorite, None);
    }

    #[test]
    fn current_favorite_status_result_updates_only_the_current_puzzle() {
        let mut app = app_with_current_puzzle("current-favorite");
        let generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "current-favorite".into(),
            generation,
            result: Ok(true),
        });
        assert_eq!(app.current_favorite, Some(true));

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "stale-favorite".into(),
            generation,
            result: Ok(false),
        });
        assert_eq!(app.current_favorite, Some(true));
    }

    #[test]
    fn favorite_status_error_stays_indeterminate_and_visible() {
        let mut app = app_with_current_puzzle("favorite-error");
        let generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "favorite-error".into(),
            generation,
            result: Err("controlled status failure".into()),
        });

        assert_eq!(app.current_favorite, None);
        assert!(app.puzzle_status.contains(&lang::tr(&app.lang, "my_favories")));
        assert!(app.puzzle_status.contains("controlled status failure"));
    }

    #[test]
    fn favorite_action_is_safe_without_a_current_puzzle_or_known_status() {
        let mut empty_app = OfflinePuzzles::new(false);
        let _ = empty_app.update(Message::FavoritePuzzle);
        assert_eq!(empty_app.current_favorite, None);

        let mut unknown_app = app_with_current_puzzle("unknown-favorite");
        let _ = unknown_app.update(Message::FavoritePuzzle);
        assert_eq!(unknown_app.current_favorite, None);
    }

    #[test]
    fn favorite_action_marks_a_known_state_indeterminate_before_its_task_completes() {
        let mut app = app_with_current_puzzle("pending-toggle");
        app.current_favorite = Some(false);

        let _ = app.update(Message::FavoritePuzzle);

        assert_eq!(app.current_favorite, None);
    }

    #[test]
    fn favorite_toggle_result_updates_only_the_matching_puzzle() {
        let mut app = app_with_current_puzzle("toggle-current");
        let generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteToggled {
            puzzle_id: "toggle-current".into(),
            generation,
            result: Ok(true),
        });
        assert_eq!(app.current_favorite, Some(true));

        let _ = app.update(Message::FavoriteToggled {
            puzzle_id: "toggle-stale".into(),
            generation,
            result: Ok(false),
        });
        assert_eq!(app.current_favorite, Some(true));
    }

    #[test]
    fn favorite_toggle_error_stays_indeterminate_and_visible() {
        let mut app = app_with_current_puzzle("toggle-error");
        let generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteToggled {
            puzzle_id: "toggle-error".into(),
            generation,
            result: Err("controlled toggle failure".into()),
        });

        assert_eq!(app.current_favorite, None);
        assert!(app.puzzle_status.contains(&lang::tr(&app.lang, "my_favories")));
        assert!(app.puzzle_status.contains("controlled toggle failure"));
    }

    #[test]
    fn stale_status_for_the_same_puzzle_cannot_overwrite_a_newer_status() {
        let mut app = app_with_current_puzzle("same-status");
        let first_generation = app.next_favorite_generation();
        let second_generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "same-status".into(),
            generation: second_generation,
            result: Ok(true),
        });
        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "same-status".into(),
            generation: first_generation,
            result: Ok(false),
        });

        assert_eq!(app.current_favorite, Some(true));
    }

    #[test]
    fn stale_status_error_for_the_same_puzzle_is_invisible() {
        let mut app = app_with_current_puzzle("same-status-error");
        let first_generation = app.next_favorite_generation();
        let second_generation = app.next_favorite_generation();
        let status_before_stale_error = app.puzzle_status.clone();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "same-status-error".into(),
            generation: second_generation,
            result: Ok(true),
        });
        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "same-status-error".into(),
            generation: first_generation,
            result: Err("stale status failure".into()),
        });

        assert_eq!(app.current_favorite, Some(true));
        assert_eq!(app.puzzle_status, status_before_stale_error);
    }

    #[test]
    fn stale_status_after_a_same_puzzle_toggle_cannot_revert_the_toggle() {
        let mut app = app_with_current_puzzle("status-then-toggle");
        let status_generation = app.next_favorite_generation();
        let toggle_generation = app.next_favorite_generation();

        let _ = app.update(Message::FavoriteToggled {
            puzzle_id: "status-then-toggle".into(),
            generation: toggle_generation,
            result: Ok(true),
        });
        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "status-then-toggle".into(),
            generation: status_generation,
            result: Ok(false),
        });

        assert_eq!(app.current_favorite, Some(true));
    }

    #[test]
    fn stale_toggle_for_the_same_puzzle_is_invisible() {
        let mut app = app_with_current_puzzle("stale-toggle");
        let toggle_generation = app.next_favorite_generation();
        let newer_generation = app.next_favorite_generation();
        let status_before_stale_error = app.puzzle_status.clone();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "stale-toggle".into(),
            generation: newer_generation,
            result: Ok(false),
        });
        let _ = app.update(Message::FavoriteToggled {
            puzzle_id: "stale-toggle".into(),
            generation: toggle_generation,
            result: Err("stale toggle failure".into()),
        });

        assert_eq!(app.current_favorite, Some(false));
        assert_eq!(app.puzzle_status, status_before_stale_error);
    }

    #[test]
    fn returning_to_a_puzzle_does_not_accept_its_first_visit_response() {
        let mut app = app_with_current_puzzle("first-a");
        app.puzzle_tab.puzzles = vec![
            navigation_puzzle("first-a"),
            navigation_puzzle("middle-b"),
            navigation_puzzle("first-a"),
        ];
        app.puzzle_tab.current_puzzle = 0;
        app.load_puzzle(false);
        let first_a_generation = app.next_favorite_generation();

        let _ = app.update(Message::ShowNextPuzzle);
        let _ = app.update(Message::ShowNextPuzzle);
        let returning_a_generation = app.favorite_generation;
        assert_ne!(returning_a_generation, first_a_generation);
        app.current_favorite = Some(true);
        let status_before_stale_response = app.puzzle_status.clone();

        let _ = app.update(Message::FavoriteStatusLoaded {
            puzzle_id: "first-a".into(),
            generation: first_a_generation,
            result: Err("first visit failure".into()),
        });

        assert_eq!(app.current_favorite, Some(true));
        assert_eq!(app.puzzle_status, status_before_stale_response);
    }

    #[test]
    fn redo_keeps_known_favorite_state_and_generation_for_the_same_puzzle() {
        let mut app = app_with_current_puzzle("redo-favorite");
        app.current_favorite = Some(true);
        let generation_before_redo = app.favorite_generation;
        let puzzle_id_before_redo = app.current_reviewable_puzzle().unwrap().puzzle_id.clone();

        let _ = app.update(Message::RedoPuzzle);

        assert_eq!(app.current_reviewable_puzzle().unwrap().puzzle_id, puzzle_id_before_redo);
        assert_eq!(app.current_favorite, Some(true));
        assert_eq!(app.favorite_generation, generation_before_redo);
    }

    #[test]
    fn drag_auto_advance_refreshes_favorites_for_the_new_puzzle() {
        let mut app = OfflinePuzzles::new(false);
        app.puzzle_tab.puzzles = vec![
            auto_advance_puzzle("drag-first"),
            auto_advance_puzzle("drag-second"),
        ];
        app.load_puzzle(false);
        app.settings_tab.saved_configs.auto_load_next = true;
        app.current_favorite = Some(true);
        app.favorite_generation = 41;

        let _ = app.update(Message::HandleDropZones(
            Square::A1,
            vec![(GenericId::new(config::BTN_IDS[Square::A2.to_index()]), Rectangle::default())],
        ));

        assert_eq!(app.puzzle_tab.current_puzzle, 1);
        assert_eq!(app.current_reviewable_puzzle().unwrap().puzzle_id, "drag-second");
        assert_eq!(app.current_favorite, None);
        assert_eq!(app.favorite_generation, 42);
    }

    #[test]
    fn drag_without_auto_advance_preserves_favorites_state() {
        let mut app = OfflinePuzzles::new(false);
        app.puzzle_tab.puzzles = vec![
            auto_advance_puzzle("drag-first"),
            auto_advance_puzzle("drag-second"),
        ];
        app.load_puzzle(false);
        app.settings_tab.saved_configs.auto_load_next = true;
        app.current_favorite = Some(true);
        app.favorite_generation = 41;

        let _ = app.update(Message::HandleDropZones(
            Square::A1,
            vec![(GenericId::new(config::BTN_IDS[Square::B1.to_index()]), Rectangle::default())],
        ));

        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.current_favorite, Some(true));
        assert_eq!(app.favorite_generation, 41);
    }
}

pub async fn screenshot_save_dialog(img: Screenshot) -> Option<(Screenshot, String)> {
    let file_path = AsyncFileDialog::new().add_filter("jpg", &["jpg", "jpeg"]).save_file().await;
    file_path.map(|file_path| (img, file_path.path().display().to_string()))
}

fn gen_view<'a>(
    game_mode: config::GameMode,
    current_puzzle_side: Color,
    flip_board: bool,
    show_coordinates: bool,
    board: &Board,
    analysis: &Board,
    from_square: Option<Square>,
    last_move_from: Option<Square>,
    last_move_to: Option<Square>,
    hint_square: Option<Square>,
    piece_theme: styles::PieceTheme,
    board_theme: styles::BoardTheme,
    puzzle_status: &'a str,
    is_fav: Option<bool>,
    has_more_puzzles: bool,
    has_previous: bool,
    analysis_history_len: usize,
    puzzle_number_ui: &'a str,
    total_puzzles: usize,
    current_puzzle_move: usize,
    game_status: GameStatus,
    puzzle_review: Option<PuzzleReviewView>,
    active_tab: &TabId,
    engine_eval: &str,
    engine_move: &str,

    engine_started: bool,
    search_tab_label: TabLabel,
    settings_tab_label: TabLabel,
    puzzle_tab_label: TabLabel,
    project_tab_label: TabLabel,
    search_tab: Element<'a, Message, Theme, iced::Renderer>,
    settings_tab: Element<'a, Message, Theme, iced::Renderer>,
    puzzle_tab: Element<'a, Message, Theme, iced::Renderer>,
    project_tab: Element<'a, Message, Theme, iced::Renderer>,
    lang: &lang::Language,
    size: Size,
    mini_ui: bool,
    board_ids: &[GenericId],
    imgs: &[Handle],
) -> Element<'a, Message, Theme, iced::Renderer> {

    let font = piece_theme == PieceTheme::FontAlpha;
    let mut board_col = Column::new().spacing(0).align_x(Alignment::Center);
    let mut board_row = Row::new().spacing(0).align_y(Alignment::Center);

    let is_white = (current_puzzle_side == Color::White) ^ flip_board;

    let board_controls_height = 135.
        + if show_coordinates { 10. } else { 0. }
        + if engine_eval.is_empty() { 0. } else { 30. }
        + if puzzle_review.is_some() { 55. } else { 0. };
    let board_height = (size.height - board_controls_height) / 8.;

    let ranks;
    let files;
    if is_white {
        ranks = (0..8).rev().collect::<Vec<i32>>();
        files = (0..8).collect::<Vec<i32>>();
    } else {
        ranks = (0..8).collect::<Vec<i32>>();
        files = (0..8).rev().collect::<Vec<i32>>();
    };
    for rank in ranks {
        for file in &files {
            let pos = Square::make_square(Rank::from_index(rank as usize), File::from_index(*file as usize));

            let (piece, color) =
                match game_mode {
                    config::GameMode::Analysis => {
                        (analysis.piece_on(pos),
                        analysis.color_on(pos))
                    } config::GameMode::Puzzle => {
                        (board.piece_on(pos),
                        board.color_on(pos))
                    }
                };

            let mut text;
            let light_square = (rank + file) % 2 != 0;

            let selected =
                if game_mode == config::GameMode::Puzzle && game_status == GameStatus::Playing {
                    from_square == Some(pos)    ||
                    last_move_from == Some(pos) ||
                    last_move_to == Some(pos)   ||
                    hint_square == Some(pos)
                } else {
                    from_square == Some(pos)
            };
            if font {
                let square_style = if selected {
                    styles::board_button_style(
                        board_theme,
                        styles::BoardSquareStyle::Light,
                    )
                } else {
                    styles::board_button_style(
                        board_theme,
                        styles::BoardSquareStyle::Paper,
                    )
                };

                if let Some(piece) = piece {
                    if color.unwrap() == Color::White {
                        text = match piece {
                            Piece::Pawn => String::from("P"),
                            Piece::Rook => String::from("R"),
                            Piece::Knight => String::from("H"),
                            Piece::Bishop => String::from("B"),
                            Piece::Queen => String::from("Q"),
                            Piece::King => String::from("K"),
                        };
                    } else {
                        text = match piece {
                            Piece::Pawn => String::from("O"),
                            Piece::Rook => String::from("T"),
                            Piece::Knight => String::from("J"),
                            Piece::Bishop => String::from("N"),
                            Piece::Queen => String::from("W"),
                            Piece::King => String::from("L"),
                        };
                    }
                    if light_square {
                        text = text.to_lowercase();
                    }
                } else if light_square {
                    text = String::from(" ");
                } else {
                    text = String::from("+");
                }

                board_row =
                    board_row.push(Button::new(
                        Text::new(text)
                        .width(board_height)
                        .height(board_height)
                        .font(config::CHESS_ALPHA)
                        .size(board_height)
                        .align_y(alignment::Vertical::Center)
                        .line_height(LineHeight::Absolute(board_height.into())
                    ))
                .padding(0)
                .on_press(Message::SelectSquare(pos))
                .style(square_style)
                );
            } else {
                let square_style;
                let container_style;

                if light_square {
                    if selected {
                        square_style = styles::board_button_style(
                            board_theme,
                            styles::BoardSquareStyle::SelectedLight,
                        );
                        container_style = styles::board_container_style(
                            board_theme,
                            styles::BoardSquareStyle::SelectedLight,
                        );
                    } else {
                        square_style = styles::board_button_style(
                            board_theme,
                            styles::BoardSquareStyle::Light,
                        );
                        container_style = styles::board_container_style(
                            board_theme,
                            styles::BoardSquareStyle::Light,
                        );
                    }
                } else {
                    if selected {
                        square_style = styles::board_button_style(
                            board_theme,
                            styles::BoardSquareStyle::SelectedDark,
                        );
                        container_style = styles::board_container_style(
                            board_theme,
                            styles::BoardSquareStyle::SelectedDark,
                        );
                    } else {
                        square_style = styles::board_button_style(
                            board_theme,
                            styles::BoardSquareStyle::Dark,
                        );
                        container_style = styles::board_container_style(
                            board_theme,
                            styles::BoardSquareStyle::Dark,
                        );
                    }
                }

                if let Some(piece) = piece {
                    let piece_index = if color.unwrap() == Color::White {
                        match piece {
                            Piece::Pawn => PieceWithColor::WhitePawn.index(),
                            Piece::Rook => PieceWithColor::WhiteRook.index(),
                            Piece::Knight => PieceWithColor::WhiteKnight.index(),
                            Piece::Bishop => PieceWithColor::WhiteBishop.index(),
                            Piece::Queen => PieceWithColor::WhiteQueen.index(),
                            Piece::King => PieceWithColor::WhiteKing.index(),
                        }
                    } else {
                        match piece {
                            Piece::Pawn => PieceWithColor::BlackPawn.index(),
                            Piece::Rook => PieceWithColor::BlackRook.index(),
                            Piece::Knight => PieceWithColor::BlackKnight.index(),
                            Piece::Bishop => PieceWithColor::BlackBishop.index(),
                            Piece::Queen => PieceWithColor::BlackQueen.index(),
                            Piece::King => PieceWithColor::BlackKing.index(),
                        }
                    };

                    board_row = board_row.push(
                        container(
                            iced_drop::droppable(
                                Svg::new(imgs[piece_index].clone()).width(board_height)
                                .height(board_height)
                            ).drag_hide(true).drag_center(true).on_drop(move |point, rect| Message::DropPiece(pos, point, rect)).on_click(Message::SelectSquare(pos))
                        ).style(container_style).id(board_ids[pos.to_index()].clone())
                     );
                } else {
                    board_row = board_row.push(container(
                            Button::new(Text::new(""))
                            .width(board_height)
                            .height(board_height)
                            .on_press(Message::SelectSquare(pos))
                            .style(square_style)
                        ).id(board_ids[pos.to_index()].clone())
                    );
                }
            }
        }

        if show_coordinates {
            board_row = board_row.push(
                Container::new(
                    Text::new((rank + 1).to_string()).size(15)
                ).align_y(iced::alignment::Vertical::Bottom)
                .align_x(iced::alignment::Horizontal::Right)
                .padding(3)
                .height(board_height)
            );
        }
        board_col = board_col.push(board_row);
        board_row = Row::new().spacing(0).align_y(Alignment::Center);
    }
    if show_coordinates {
        if is_white {
            board_col = board_col.push(row![
                Text::new("a").size(15).width(board_height),
                Text::new("b").size(15).width(board_height),
                Text::new("c").size(15).width(board_height),
                Text::new("d").size(15).width(board_height),
                Text::new("e").size(15).width(board_height),
                Text::new("f").size(15).width(board_height),
                Text::new("g").size(15).width(board_height),
                Text::new("h").size(15).width(board_height),
            ]);
        } else {
            board_col = board_col.push(row![
                Text::new("h").size(15).width(board_height),
                Text::new("g").size(15).width(board_height),
                Text::new("f").size(15).width(board_height),
                Text::new("e").size(15).width(board_height),
                Text::new("d").size(15).width(board_height),
                Text::new("c").size(15).width(board_height),
                Text::new("b").size(15).width(board_height),
                Text::new("a").size(15).width(board_height),
            ]);
        }
    }

    let game_mode_row = row![
        Text::new(lang::tr(lang, "mode")),
        Radio::new(lang::tr(lang, "mode_puzzle"), config::GameMode::Puzzle, Some(game_mode), Message::SelectMode).style(styles::radio_style),
        Radio::new(lang::tr(lang, "mode_analysis"), config::GameMode::Analysis, Some(game_mode), Message::SelectMode).style(styles::radio_style)
    ].spacing(10).padding(10).align_y(Alignment::Center);

    let fav_label = match is_fav {
        Some(true) => lang::tr(lang, "unfav"),
        Some(false) => lang::tr(lang, "fav"),
        None => lang::tr(lang, "my_favories"),
    };
    let favorite_button = {
        let button = Button::new(Text::new(fav_label)).style(btn_style_simple);
        if is_fav.is_some() {
            button.on_press(Message::FavoritePuzzle)
        } else {
            button
        }
    };
    let mut navigation_row = Row::new().padding(3).spacing(10);
    if game_mode == config::GameMode::Analysis {
        if analysis_history_len > current_puzzle_move {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "takeback"))).on_press(Message::GoBackMove).style(btn_style_simple));
        } else {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "takeback"))).style(btn_style_simple));
        }
        if engine_started {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "stop_engine"))).on_press(Message::StartEngine).style(btn_style_simple));
        } else {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "start_engine"))).on_press(Message::StartEngine).style(btn_style_simple));
        }
    } else {
        if has_previous {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "previous"))).on_press(Message::ShowPreviousPuzzle).style(btn_style_simple))
        } else {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "previous"))).style(btn_style_simple));
        }
        if has_more_puzzles {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "next"))).on_press(Message::ShowNextPuzzle).style(btn_style_simple))
        } else {
            navigation_row = navigation_row.push(Button::new(Text::new(lang::tr(lang, "next"))).style(btn_style_simple));
        }
        if game_status == GameStatus::NoPuzzles {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "redo"))).style(btn_style_simple))
                .push(favorite_button)
                .push(Button::new(Text::new(lang::tr(lang, "hint"))).style(btn_style_simple));
        } else if game_status == GameStatus::PuzzleEnded {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "redo"))).on_press(Message::RedoPuzzle).style(btn_style_simple))
                .push(favorite_button)
                .push(Button::new(Text::new(lang::tr(lang, "hint"))).style(btn_style_simple));
        } else {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "redo"))).on_press(Message::RedoPuzzle).style(btn_style_simple))
                .push(favorite_button)
                .push(Button::new(Text::new(lang::tr(lang, "hint"))).on_press(Message::ShowHint).style(btn_style_simple));
        }
    }

    let (input_index, btn_go) = if game_status == GameStatus::Playing {
        (text_input(puzzle_number_ui, puzzle_number_ui).
            on_input(Message::PuzzleInputIndexChange).width(Length::Fixed(150.)),
        button(text(lang::tr(lang, "go"))).on_press(Message::JumpToPuzzle).style(btn_style_simple))
    } else {
        (text_input(puzzle_number_ui, puzzle_number_ui).width(Length::Fixed(150.)),
        button(text(lang::tr(lang, "go"))).style(btn_style_simple))
    };

    let pagination_row = row![
        text(lang::tr(lang, "puzzle")),
        input_index,
        text(lang::tr(lang, "of") + &total_puzzles.to_string()),
        btn_go
    ].spacing(10).align_y(Alignment::Center);

    board_col = board_col.push(Text::new(puzzle_status)).push(game_mode_row).push(navigation_row).push(pagination_row);
    if let Some(review) = puzzle_review {
        let review_status = if review.decision_loaded {
            match review.decision {
                Some(ProjectPuzzleDecision::Selected) => lang::tr(lang, "selected"),
                Some(ProjectPuzzleDecision::Discarded) => lang::tr(lang, "discarded"),
                None => lang::tr(lang, "unreviewed"),
            }
        } else {
            lang::tr(lang, "review_unavailable")
        };
        let mut select_button = Button::new(Text::new(lang::tr(lang, "select_puzzle"))).style(btn_style_simple);
        let mut discard_button = Button::new(Text::new(lang::tr(lang, "discard_puzzle"))).style(btn_style_simple);
        let mut clear_button = Button::new(Text::new(lang::tr(lang, "clear_review"))).style(btn_style_simple);
        if review.decision_loaded && review.decision != Some(ProjectPuzzleDecision::Selected) {
            select_button = select_button.on_press(Message::SetPuzzleReview(ProjectPuzzleDecision::Selected));
        }
        if review.decision_loaded && review.decision != Some(ProjectPuzzleDecision::Discarded) {
            discard_button = discard_button.on_press(Message::SetPuzzleReview(ProjectPuzzleDecision::Discarded));
        }
        if review.decision_loaded && review.decision.is_some() {
            clear_button = clear_button.on_press(Message::ClearPuzzleReview);
        }
        board_col = board_col.push(
            Column::new().spacing(5)
                .push(Text::new(format!("{}: {}   {}: {}", lang::tr(lang, "active_chapter"), review.chapter_name, lang::tr(lang, "review_status"), review_status)))
                .push(Row::new().spacing(10).push(select_button).push(discard_button).push(clear_button))
                .push(Text::new(review.status))
        );
    }
    if !engine_eval.is_empty() {
        board_col = board_col.push(
            row![
                Text::new(lang::tr(lang, "eval") + engine_eval),
                Text::new(lang::tr(lang, "best_move") + engine_move)
            ].padding(5).spacing(15)
        );
    }
    if  mini_ui {
        let button_mini = Button::new(Text::new(">")).on_press(Message::MinimizeUI).style(btn_style_simple);
        row![board_col,button_mini].spacing(5).align_y(Alignment::Start).into()
    } else {
        let button_mini = Button::new(Text::new("<")).on_press(Message::MinimizeUI).style(btn_style_simple);
        let tabs = Tabs::new(Message::TabSelected)
                .push(TabId::Search, search_tab_label, search_tab)
                .push(TabId::Settings, settings_tab_label, settings_tab)
                .push(TabId::CurrentPuzzle ,puzzle_tab_label, puzzle_tab)
                .push(TabId::Project, project_tab_label, project_tab)
                .tab_bar_position(iced_aw::TabBarPosition::Top)
                .tab_bar_style(styles::tab_style)
                .set_active_tab(active_tab);

        row![board_col,button_mini,tabs].spacing(5).align_y(Alignment::Start).into()
    }
}

trait Tab {
    type Message;

    fn title(&self) -> String;

    fn tab_label(&self) -> TabLabel;

    fn view(&self) -> Element<'_, Self::Message> {
        let column = Column::new()
            .spacing(20)
            .push(Text::new(self.title()).size(HEADER_SIZE))
            .push(self.content());

        Container::new(column)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .padding(TAB_PADDING)
            .into()
    }

    fn content(&self) -> Element<'_, Self::Message>;
}

fn main() -> iced::Result {
    let window_settings = iced::window::Settings {
            size: Size {
                width: config::SETTINGS.window_width, //(config::SETTINGS.square_size * 8) as u32 + 450,
                height: config::SETTINGS.window_height,//(config::SETTINGS.square_size * 8) as u32 + 120,
            },
            resizable: true,
            exit_on_close_request: false,
            ..iced::window::Settings::default()
        };

    iced::application(OfflinePuzzles::init, OfflinePuzzles::update, OfflinePuzzles::view)
        .theme(OfflinePuzzles::theme)
        .subscription(OfflinePuzzles::subscription)
        .window(window_settings)
        .title("Chess Material Studio")
        .run()
}
