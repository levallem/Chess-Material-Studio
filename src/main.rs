#![windows_subsystem = "windows"]

use download_db::download_lichess_db;
use eval::{Engine, EngineStatus};
use iced::advanced::widget::Id as GenericId;
use iced::event::{self, Event};
use iced::widget::svg::Handle;
use iced::widget::{
    Button, Column, Container, Radio, Row, Svg, Text, button, center, container, responsive, row,
    text, text_input,
};
use iced::window::{self, Screenshot};
use iced::{Alignment, Length, Task, alignment};
use iced::{Element, Rectangle, Size, Subscription, Theme};
use image::{DynamicImage, RgbaImage};
use rfd::AsyncFileDialog;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use styles::PieceTheme;
use tokio::sync::mpsc::{self, Sender};

use chess::{ALL_SQUARES, Board, BoardStatus, ChessMove, Color, File, Game, Piece, Rank, Square};
use chess_material_studio::project::ProjectPuzzleDecision;
use iced_aw::{TabLabel, Tabs};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Source, source::SineWave};

use rand::rng;
use rand::seq::SliceRandom;

mod config;

mod pgn_tab;
use pgn_tab::{PgnMessage, PgnTab};

pub mod download_db;
mod search_tab;
mod styles;
use search_tab::{SearchMesssage, SearchTab};

mod settings;
use settings::{SettingsMessage, SettingsTab};

mod puzzles;
use puzzles::{GameStatus, PuzzleMessage, PuzzleTab, validate_puzzle_batch};

mod project_tab;
use project_tab::{ProjectMessage, ProjectTab, PuzzleReviewView};

use crate::styles::btn_style_simple;

mod eval;
mod export;
mod lang;
mod openings;

mod db;
pub mod models;
pub mod schema;

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
    Pgn,
}

#[derive(Clone, Copy, Hash, Eq, PartialEq, PartialOrd, Ord)]
enum PieceWithColor {
    WhitePawn,
    WhiteRook,
    WhiteKnight,
    WhiteBishop,
    WhiteQueen,
    WhiteKing,
    BlackPawn,
    BlackRook,
    BlackKnight,
    BlackBishop,
    BlackQueen,
    BlackKing,
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
    Pgn(PgnMessage),
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
    SaveScreenshot(Option<(Screenshot, PathBuf)>),
    ScreenshotFailed(String),
    ExportPDF(Option<String>),
    LoadPuzzle {
        generation: u64,
        result: Result<Vec<config::Puzzle>, String>,
    },
    LoadFavorites {
        generation: u64,
        result: Result<Vec<config::Puzzle>, String>,
    },
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
    EngineFailed {
        reason: String,
        exit_requested: bool,
    },
    UpdateEval((Option<String>, Option<String>)),
    EngineReady(mpsc::Sender<String>),
    EngineFileChosen(Option<String>),
    FavoritePuzzle,
    MinimizeUI,
    WindowResizeStateResolved {
        size: Size,
        maximized: bool,
        generation: u64,
    },
    ResolveMaximizedStatusBeforeExit,
    SaveMaximizedStatusAndExit(bool),
    StartDBDownload,
    DBDownloadFinished,
    DBDownloadFailed(String),
    DownloadProgress(String),
    PuzzleSqliteSourceSelected,
    PuzzleInputIndexChange(String),
    JumpToPuzzle,
    SetPuzzleReview(ProjectPuzzleDecision),
    ClearPuzzleReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitPersistenceAction {
    ResolveMaximizedStatus,
    StayOpen,
}

#[derive(Clone, Copy)]
struct PendingWindowResize {
    size: Size,
    generation: u64,
}

struct SoundPlayback {
    handle: MixerDeviceSink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioCue {
    OnePiece,
    TwoPieces,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CueSpec {
    frequency_hz: f32,
    duration_ms: u64,
    amplitude: f32,
}

impl CueSpec {
    const fn new(frequency_hz: f32, duration_ms: u64, amplitude: f32) -> Self {
        Self {
            frequency_hz,
            duration_ms,
            amplitude,
        }
    }
}

fn cue_spec(cue: AudioCue) -> CueSpec {
    match cue {
        AudioCue::OnePiece => CueSpec::new(660.0, 70, 0.12),
        AudioCue::TwoPieces => CueSpec::new(880.0, 100, 0.12),
    }
}

fn cue_source(cue: AudioCue) -> impl Source<Item = f32> {
    let spec = cue_spec(cue);
    SineWave::new(spec.frequency_hz)
        .take_duration(std::time::Duration::from_millis(spec.duration_ms))
        .amplify(spec.amplitude)
}

impl SoundPlayback {
    pub fn init_sound() -> Option<Self> {
        let mut sound_playback = None;
        if let Ok(handle) = DeviceSinkBuilder::open_default_sink() {
            sound_playback = Some(SoundPlayback { handle });
        }
        sound_playback
    }
    pub fn play_audio(&self, cue: AudioCue) {
        self.handle.mixer().add(cue_source(cue));
    }
}

fn play_audio_if_available(playback: Option<&SoundPlayback>, cue: AudioCue) -> bool {
    let Some(playback) = playback else {
        return false;
    };
    playback.play_audio(cue);
    true
}

fn get_image_handles(theme: &PieceTheme) -> Vec<Handle> {
    let mut handles = Vec::<Handle>::with_capacity(12);
    let theme_str = &theme.to_string();

    handles.insert(
        PieceWithColor::WhitePawn.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wP.svg"),
    );
    handles.insert(
        PieceWithColor::WhiteRook.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wR.svg"),
    );
    handles.insert(
        PieceWithColor::WhiteKnight.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wN.svg"),
    );
    handles.insert(
        PieceWithColor::WhiteBishop.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wB.svg"),
    );
    handles.insert(
        PieceWithColor::WhiteQueen.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wQ.svg"),
    );
    handles.insert(
        PieceWithColor::WhiteKing.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/wK.svg"),
    );

    handles.insert(
        PieceWithColor::BlackPawn.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bP.svg"),
    );
    handles.insert(
        PieceWithColor::BlackRook.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bR.svg"),
    );
    handles.insert(
        PieceWithColor::BlackKnight.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bN.svg"),
    );
    handles.insert(
        PieceWithColor::BlackBishop.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bB.svg"),
    );
    handles.insert(
        PieceWithColor::BlackQueen.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bQ.svg"),
    );
    handles.insert(
        PieceWithColor::BlackKing.index(),
        Handle::from_path(String::from("pieces/") + theme_str + "/bK.svg"),
    );

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
        && en_passant != &"-"
    {
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
        && piece == Piece::Pawn
        && ((color == Color::White && to.get_rank() == Rank::Eighth)
            || (color == Color::Black && to.get_rank() == Rank::First))
    {
        match promo_piece {
            Piece::Rook => move_made_notation += "r",
            Piece::Knight => move_made_notation += "n",
            Piece::Bishop => move_made_notation += "b",
            _ => move_made_notation += "q",
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
    search_generation: u64,
    window_resize_generation: u64,
    pending_window_resize: Option<PendingWindowResize>,

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
    pgn_tab: PgnTab,
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
                String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            ),
            engine_sender: None,
            engine_move: String::new(),

            downloading_db: false,
            download_progress: String::new(),
            puzzle_status: lang::tr(&config::SETTINGS.lang, "use_search"),
            puzzle_number_ui: String::from("1"),
            current_favorite: None,
            favorite_generation: 0,
            search_generation: 0,
            window_resize_generation: 0,
            pending_window_resize: None,
            search_tab: SearchTab::new(),
            settings_tab: SettingsTab::new(),
            puzzle_tab: PuzzleTab::new(),
            project_tab: ProjectTab::new(),
            pgn_tab: PgnTab::new(),
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
        let side = match self.game_mode {
            config::GameMode::Analysis => self.analysis.side_to_move(),
            config::GameMode::Puzzle => self.board.side_to_move(),
        };
        let color = match self.game_mode {
            config::GameMode::Analysis => self.analysis.current_position().color_on(to),
            config::GameMode::Puzzle => self.board.color_on(to),
        };
        // If the user clicked on another piece of his own side,
        // just replace the previous selection and exit
        if self.puzzle_tab.game_status == GameStatus::Playing && color == Some(side) {
            self.from_square = Some(to);
            return false;
        }
        self.from_square = None;

        if self.game_mode == config::GameMode::Analysis {
            let move_made_notation = get_notation_string(
                self.analysis.current_position(),
                self.search_tab.piece_to_promote_to,
                from,
                to,
            );

            let move_made = ChessMove::new(
                Square::from_str(&String::from(&move_made_notation[..2])).unwrap(),
                Square::from_str(&String::from(&move_made_notation[2..4])).unwrap(),
                PuzzleTab::check_promotion(&move_made_notation),
            );

            if self.analysis.make_move(move_made) {
                self.analysis_history.push(self.analysis.current_position());
                self.engine.position = self.analysis.current_position().to_string();
                if let Some(sender) = &self.engine_sender
                    && let Err(e) = sender
                        .blocking_send(san_correct_ep(self.analysis.current_position().to_string()))
                {
                    self.record_engine_failure(format!("lost contact with engine: {e}"));
                }
                if self.settings_tab.saved_configs.play_sound {
                    play_audio_if_available(self.sound_playback.as_ref(), AudioCue::OnePiece);
                }
            }
        } else if !self.puzzle_tab.puzzles.is_empty() {
            let movement;
            let move_made_notation =
                get_notation_string(self.board, self.search_tab.piece_to_promote_to, from, to);

            let move_made = ChessMove::new(
                Square::from_str(&String::from(&move_made_notation[..2])).unwrap(),
                Square::from_str(&String::from(&move_made_notation[2..4])).unwrap(),
                PuzzleTab::check_promotion(&move_made_notation),
            );

            let is_mate = self.board.legal(move_made)
                && self.board.make_move_new(move_made).status() == BoardStatus::Checkmate;

            let correct_moves: Vec<&str> = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle]
                .moves
                .split_whitespace()
                .collect::<Vec<&str>>();
            let correct_move = ChessMove::new(
                Square::from_str(&String::from(
                    &correct_moves[self.puzzle_tab.current_puzzle_move][..2],
                ))
                .unwrap(),
                Square::from_str(&String::from(
                    &correct_moves[self.puzzle_tab.current_puzzle_move][2..4],
                ))
                .unwrap(),
                PuzzleTab::check_promotion(correct_moves[self.puzzle_tab.current_puzzle_move]),
            );

            // If the move is correct we can apply it to the board
            if is_mate || (move_made == correct_move) {
                self.board = self.board.make_move_new(move_made);
                self.analysis_history.push(self.board);

                self.puzzle_tab.current_puzzle_move += 1;

                if self.puzzle_tab.current_puzzle_move == correct_moves.len() {
                    if self.settings_tab.saved_configs.play_sound {
                        play_audio_if_available(self.sound_playback.as_ref(), AudioCue::OnePiece);
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
                    if self.settings_tab.saved_configs.play_sound {
                        play_audio_if_available(self.sound_playback.as_ref(), AudioCue::TwoPieces);
                    }
                    movement = ChessMove::new(
                        Square::from_str(&String::from(
                            &correct_moves[self.puzzle_tab.current_puzzle_move][..2],
                        ))
                        .unwrap(),
                        Square::from_str(&String::from(
                            &correct_moves[self.puzzle_tab.current_puzzle_move][2..4],
                        ))
                        .unwrap(),
                        PuzzleTab::check_promotion(
                            correct_moves[self.puzzle_tab.current_puzzle_move],
                        ),
                    );

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
        let puzzle_moves: Vec<&str> = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle]
            .moves
            .split_whitespace()
            .collect();

        // The opponent's last move (before the puzzle starts)
        // is in the "moves" field of the cvs, so we need to apply it.
        self.board =
            Board::from_str(&self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle].fen).unwrap();

        let movement = ChessMove::new(
            Square::from_str(&String::from(&puzzle_moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&puzzle_moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(puzzle_moves[0]),
        );

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

    fn replace_puzzle_batch(
        &mut self,
        mut puzzles: Vec<config::Puzzle>,
        shuffle: bool,
    ) -> Result<(), String> {
        validate_puzzle_batch(&puzzles)?;
        if shuffle {
            puzzles.shuffle(&mut rng());
        }
        self.puzzle_tab.puzzles = puzzles;
        self.puzzle_tab.current_puzzle = 0;
        self.puzzle_number_ui = String::from("1");
        self.load_puzzle(false);
        Ok(())
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

    fn next_search_generation(&mut self) -> u64 {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_generation
    }

    fn search_response_is_current(&self, generation: u64) -> bool {
        self.search_generation == generation
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

    fn clear_engine_state(&mut self) {
        self.engine_state = EngineStatus::TurnedOff;
        self.engine_sender = None;
        self.engine_eval = String::new();
        self.engine_move = String::new();
    }

    fn record_engine_failure(&mut self, reason: impl std::fmt::Display) {
        self.clear_engine_state();
        self.puzzle_status = format!("Engine error: {reason}");
    }

    fn handle_engine_failure(&mut self, reason: String, exit_requested: bool) -> Task<Message> {
        self.record_engine_failure(reason);
        self.final_exit_task(exit_requested)
    }

    fn final_exit_task(&self, exit_requested: bool) -> Task<Message> {
        match Self::exit_persistence_action(exit_requested) {
            ExitPersistenceAction::ResolveMaximizedStatus => {
                Task::done(Message::ResolveMaximizedStatusBeforeExit)
            }
            ExitPersistenceAction::StayOpen => Task::none(),
        }
    }

    fn exit_persistence_action(exit_requested: bool) -> ExitPersistenceAction {
        if exit_requested {
            ExitPersistenceAction::ResolveMaximizedStatus
        } else {
            ExitPersistenceAction::StayOpen
        }
    }

    fn resolve_maximized_status_before_exit(&self) -> Task<Message> {
        if let Some(window_id) = self.window_id {
            window::is_maximized(window_id).map(Message::SaveMaximizedStatusAndExit)
        } else {
            Task::none()
        }
    }

    fn next_window_resize_generation(&mut self) -> u64 {
        self.window_resize_generation = self.window_resize_generation.wrapping_add(1);
        self.window_resize_generation
    }

    fn resolve_window_resize(&mut self, size: Size) -> Task<Message> {
        let generation = self.remember_pending_window_resize(size);
        if let Some(window_id) = self.window_id {
            iced::window::is_maximized(window_id).map(move |maximized| {
                Message::WindowResizeStateResolved {
                    size,
                    maximized,
                    generation,
                }
            })
        } else {
            self.pending_window_resize = None;
            Task::none()
        }
    }

    fn remember_pending_window_resize(&mut self, size: Size) -> u64 {
        let generation = self.next_window_resize_generation();
        self.pending_window_resize = Some(PendingWindowResize { size, generation });
        generation
    }

    fn apply_window_resize_resolution(
        &mut self,
        size: Size,
        maximized: bool,
        generation: u64,
    ) -> bool {
        if generation != self.window_resize_generation
            || self.pending_window_resize.map(|pending| pending.generation) != Some(generation)
        {
            return false;
        }

        self.pending_window_resize = None;
        self.settings_tab.record_window_resize(size, maximized);
        true
    }

    fn consolidate_pending_window_resize_before_exit(&mut self, maximized: bool) {
        if let Some(pending) = self.pending_window_resize.take()
            && pending.generation == self.window_resize_generation
        {
            self.settings_tab
                .record_window_resize(pending.size, maximized);
            return;
        }

        self.settings_tab.maximized = maximized;
    }

    fn persist_settings_before_exit(&self) {
        let path = self.search_tab.settings_path().to_path_buf();
        self.persist_settings_before_exit_to_paths(&path, &path);
    }

    fn persist_settings_before_exit_to_paths(
        &self,
        search_settings_path: &Path,
        window_settings_path: &Path,
    ) {
        if self
            .search_tab
            .save_current_search_settings_to_path(search_settings_path)
            .is_err()
        {
            eprintln!("Error saving search settings.");
        }
        if self
            .settings_tab
            .save_window_size_to_path(window_settings_path)
            .is_err()
        {
            eprintln!("Error saving config file.");
        }
    }

    fn send_engine_command(&self, command: &str) -> Result<(), String> {
        let sender = self
            .engine_sender
            .as_ref()
            .ok_or_else(|| String::from("engine control channel is unavailable"))?;
        sender
            .blocking_send(command.to_string())
            .map_err(|error| format!("lost contact with engine: {error}"))
    }
    // Old Iced application trait stuff
    fn init() -> (Self, Task<Message>) {
        let has_lichess_db = config::puzzle_source_exists(&config::SETTINGS);
        (
            Self::new(has_lichess_db),
            window::latest().map(Message::WindowInitialized),
        )
    }

    fn update(&mut self, message: self::Message) -> Task<Message> {
        match (self.from_square, message) {
            (None, Message::SelectSquare(pos)) => {
                let side = match self.game_mode {
                    config::GameMode::Analysis => self.analysis.side_to_move(),
                    config::GameMode::Puzzle => self.board.side_to_move(),
                };
                let color = match self.game_mode {
                    config::GameMode::Analysis => self.analysis.current_position().color_on(pos),
                    config::GameMode::Puzzle => self.board.color_on(pos),
                };

                if (self.puzzle_tab.game_status == GameStatus::Playing
                    || self.game_mode == config::GameMode::Analysis)
                    && color == Some(side)
                {
                    self.hint_square = None;
                    self.from_square = Some(pos);
                }
                Task::none()
            }
            (Some(from), Message::SelectSquare(to)) if from != to => {
                if self.verify_and_make_move(from, to) {
                    self.refresh_current_favorite_status()
                } else {
                    Task::none()
                }
            }
            (Some(_), Message::SelectSquare(to)) => {
                self.from_square = Some(to);
                Task::none()
            }
            (_, Message::TabSelected(selected)) => {
                self.active_tab = selected;
                Task::none()
            }
            (_, Message::Settings(message)) => self.settings_tab.update(message),
            (_, Message::Project(message)) => {
                let task = self.project_tab.update(message);
                self.refresh_current_puzzle_review();
                task
            }
            (_, Message::PuzzleSqliteSourceSelected) => {
                self.has_db = config::puzzle_source_exists(&self.settings_tab.saved_configs);
                Task::none()
            }
            (_, Message::SelectMode(message)) => {
                self.game_mode = message;
                if message == config::GameMode::Analysis {
                    self.analysis = Game::new_with_board(self.board);
                } else {
                    if self.engine_state != EngineStatus::TurnedOff
                        && self.engine_sender.is_some()
                        && let Err(error) = self.send_engine_command(eval::STOP_COMMAND)
                    {
                        return self.handle_engine_failure(error, false);
                    }
                    self.analysis_history
                        .truncate(self.puzzle_tab.current_puzzle_move);
                }
                Task::none()
            }
            (_, Message::ShowHint) => {
                let moves = self.puzzle_tab.puzzles[self.puzzle_tab.current_puzzle]
                    .moves
                    .split_whitespace()
                    .collect::<Vec<&str>>();
                if !moves.is_empty() && moves.len() > self.puzzle_tab.current_puzzle_move {
                    self.hint_square = Some(
                        Square::from_str(&moves[self.puzzle_tab.current_puzzle_move][..2]).unwrap(),
                    );
                } else {
                    self.hint_square = None;
                }

                Task::none()
            }
            (_, Message::ShowNextPuzzle) => {
                self.inc_puzzle_counter();
                self.load_puzzle(false);
                self.refresh_current_favorite_status()
            }
            (_, Message::ShowPreviousPuzzle) => {
                if self.puzzle_tab.current_puzzle > 0 && self.game_mode == config::GameMode::Puzzle
                {
                    self.dec_puzzle_counter();
                    self.load_puzzle(false);
                    self.refresh_current_favorite_status()
                } else {
                    Task::none()
                }
            }
            (_, Message::GoBackMove) => {
                if self.game_mode == config::GameMode::Analysis
                    && self.analysis_history.len() > self.puzzle_tab.current_puzzle_move
                {
                    self.analysis_history.pop();
                    self.analysis = Game::new_with_board(*self.analysis_history.last().unwrap());
                    if let Some(sender) = &self.engine_sender
                        && let Err(e) = sender.blocking_send(san_correct_ep(
                            self.analysis.current_position().to_string(),
                        ))
                    {
                        self.record_engine_failure(format!("lost contact with engine: {e}"));
                    }
                }
                Task::none()
            }
            (_, Message::RedoPuzzle) => {
                self.load_puzzle(false);
                Task::none()
            }
            (_, Message::LoadProjectPuzzles(puzzles_vec)) => {
                if puzzles_vec.is_empty() {
                    return Task::none();
                }
                if let Err(error) = self.replace_puzzle_batch(puzzles_vec, false) {
                    self.puzzle_status = error;
                    return Task::none();
                }
                self.from_square = None;
                if self.engine_state != EngineStatus::TurnedOff
                    && self.engine_sender.is_some()
                    && let Err(error) = self.send_engine_command(eval::STOP_COMMAND)
                {
                    return self.handle_engine_failure(error, false);
                }
                self.refresh_current_favorite_status()
            }
            (_, Message::LoadPuzzle { generation, result }) => {
                if !self.search_response_is_current(generation) {
                    return Task::none();
                }
                self.search_tab.show_searching_msg = false;
                let puzzles_vec = match result {
                    Ok(puzzles_vec) => puzzles_vec,
                    Err(error) => {
                        self.puzzle_status = format!("{}: {error}", lang::tr(&self.lang, "search"));
                        return Task::none();
                    }
                };
                if !puzzles_vec.is_empty() {
                    if let Err(error) = self.replace_puzzle_batch(puzzles_vec, true) {
                        self.puzzle_status = format!("{}: {error}", lang::tr(&self.lang, "search"));
                        return Task::none();
                    }
                    self.from_square = None;
                    if self.engine_state != EngineStatus::TurnedOff
                        && self.engine_sender.is_some()
                        && let Err(error) = self.send_engine_command(eval::STOP_COMMAND)
                    {
                        return self.handle_engine_failure(error, false);
                    }
                    return self.refresh_current_favorite_status();
                } else {
                    self.from_square = None;
                    self.game_mode = config::GameMode::Puzzle;
                    // Just putting the default position to make it obvious the search ended.
                    self.board = Board::default();
                    self.last_move_from = None;
                    self.last_move_to = None;
                    self.puzzle_tab.game_status = GameStatus::NoPuzzles;
                    self.puzzle_status = lang::tr(&self.lang, "no_puzzle_found");
                    self.current_favorite = None;
                }
                self.refresh_current_puzzle_review();
                Task::none()
            }
            (_, Message::LoadFavorites { generation, result }) => {
                if !self.search_response_is_current(generation) {
                    return Task::none();
                }
                self.search_tab.show_searching_msg = false;
                match result {
                    Ok(puzzles_vec) => {
                        if !puzzles_vec.is_empty() {
                            if let Err(error) = self.replace_puzzle_batch(puzzles_vec, true) {
                                self.puzzle_status = self.favorite_error_status(&error);
                                return Task::none();
                            }
                            self.from_square = None;
                            if self.engine_state != EngineStatus::TurnedOff
                                && self.engine_sender.is_some()
                                && let Err(error) = self.send_engine_command(eval::STOP_COMMAND)
                            {
                                return self.handle_engine_failure(error, false);
                            }
                            self.refresh_current_favorite_status()
                        } else {
                            self.from_square = None;
                            self.game_mode = config::GameMode::Puzzle;
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
            }
            (
                _,
                Message::FavoriteStatusLoaded {
                    puzzle_id,
                    generation,
                    result,
                },
            ) => {
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
            }
            (
                _,
                Message::FavoriteToggled {
                    puzzle_id,
                    generation,
                    result,
                },
            ) => {
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
            }
            (_, Message::ChangeSettings(message)) => {
                if let Some(settings) = message {
                    self.search_tab.piece_theme_promotion = self.settings_tab.piece_theme;
                    self.engine.engine_path = self.settings_tab.engine_path.clone();
                    self.lang = settings.lang;
                    self.search_tab.lang = self.lang;
                    self.search_tab.theme.lang = self.lang;
                    self.search_tab.opening.lang = self.lang;
                    self.puzzle_tab.lang = self.lang;
                    self.project_tab.lang = self.lang;
                    self.pgn_tab.lang = self.lang;
                    self.settings_tab.saved_configs = settings;
                    self.piece_imgs = get_image_handles(&self.settings_tab.piece_theme);
                    self.search_tab.promotion_piece_img =
                        search_tab::gen_piece_vec(&self.settings_tab.piece_theme);
                }
                Task::none()
            }
            (_, Message::PuzzleInfo(message)) => self.puzzle_tab.update(message),
            (_, Message::Pgn(message)) => self.pgn_tab.update(message),
            (_, Message::Search(SearchMesssage::ClickSearch)) => {
                let generation = self.next_search_generation();
                let task = if self.search_tab.is_favorites() {
                    self.search_tab.start_favorites_search(generation)
                } else {
                    match self.project_tab.reviewed_puzzle_ids_for_active_chapter() {
                        Ok(excluded_ids) => self
                            .search_tab
                            .start_lichess_search(generation, excluded_ids.unwrap_or_default()),
                        Err(error) => {
                            self.search_tab.show_searching_msg = false;
                            self.puzzle_status =
                                format!("{}: {error}", lang::tr(&self.lang, "review_error"));
                            return Task::none();
                        }
                    }
                };
                match task {
                    Ok(task) => task,
                    Err(status_key) => {
                        self.search_tab.show_searching_msg = false;
                        self.puzzle_status = lang::tr(&self.lang, status_key);
                        Task::none()
                    }
                }
            }
            (_, Message::Search(message)) => self.search_tab.update(message),
            (_, Message::PuzzleInputIndexChange(puzzle_input)) => {
                self.puzzle_number_ui = puzzle_input;
                Task::none()
            }
            (_, Message::JumpToPuzzle) => {
                // Test if puzzle index typed is valid
                let puzzle_index = self.puzzle_number_ui.parse::<usize>();
                if let Ok(index) = puzzle_index
                    && index > 0
                    && index <= self.puzzle_tab.puzzles.len()
                {
                    // The user typed value starts on 1, not zero, so we subtract 1
                    self.puzzle_tab.current_puzzle = index - 1;
                }
                self.load_puzzle(false);
                self.refresh_current_favorite_status()
            }
            (_, Message::SetPuzzleReview(decision)) => {
                if let Some(puzzle) = self.current_reviewable_puzzle().cloned() {
                    self.project_tab.set_puzzle_review(&puzzle, decision);
                }
                Task::none()
            }
            (_, Message::ClearPuzzleReview) => {
                if let Some(puzzle) = self.current_reviewable_puzzle().cloned() {
                    self.project_tab.clear_puzzle_review(&puzzle);
                }
                Task::none()
            }
            (_, Message::ScreenshotCreated(screenshot)) => {
                Task::perform(screenshot_save_dialog(screenshot), Message::SaveScreenshot)
            }
            (_, Message::SaveScreenshot(img_and_path)) => {
                match img_and_path {
                    Some((screenshot, path)) => match screenshot_crop_rectangle(
                        self.settings_tab.show_coordinates,
                        self.settings_tab.window_height,
                    )
                    .and_then(|crop| save_screenshot_to_path(&screenshot, crop, &path))
                    {
                        Ok(()) => self.puzzle_status = lang::tr(&self.lang, "screenshot_saved"),
                        Err(error) => {
                            self.puzzle_status =
                                format!("{}: {error}", lang::tr(&self.lang, "screenshot_failed"));
                        }
                    },
                    None => self.puzzle_status = lang::tr(&self.lang, "screenshot_cancelled"),
                }
                Task::none()
            }
            (_, Message::ExportPDF(file_path)) => {
                match file_path {
                    Some(file_path) => match export::to_pdf(
                        &self.puzzle_tab.puzzles,
                        self.settings_tab.export_pgs.parse::<i32>().unwrap(),
                        &self.lang,
                        file_path,
                    ) {
                        Ok(()) => self.puzzle_status = lang::tr(&self.lang, "normal_pdf_exported"),
                        Err(error) => {
                            self.puzzle_status = format!(
                                "{}: {error}",
                                lang::tr(&self.lang, "normal_pdf_export_failed")
                            );
                        }
                    },
                    None => {
                        self.puzzle_status = lang::tr(&self.lang, "normal_pdf_export_cancelled")
                    }
                }
                Task::none()
            }
            (_, Message::ExportPGN(file_path)) => {
                match file_path {
                    Some(file_path) => {
                        match export::to_pgn(&self.puzzle_tab.puzzles, &self.lang, file_path) {
                            Ok(()) => {
                                self.puzzle_status = lang::tr(&self.lang, "normal_pgn_exported")
                            }
                            Err(error) => {
                                self.puzzle_status = format!(
                                    "{}: {error}",
                                    lang::tr(&self.lang, "normal_pgn_export_failed")
                                );
                            }
                        }
                    }
                    None => {
                        self.puzzle_status = lang::tr(&self.lang, "normal_pgn_export_cancelled")
                    }
                }
                Task::none()
            }
            (_, Message::ScreenshotFailed(error)) => {
                self.puzzle_status =
                    format!("{}: {error}", lang::tr(&self.lang, "screenshot_failed"));
                Task::none()
            }
            (_, Message::EventOccurred(event)) => {
                if let Event::Window(window::Event::CloseRequested) = event {
                    match self.engine_state {
                        EngineStatus::TurnedOff => self.final_exit_task(true),
                        _ => {
                            if let Err(error) = self.send_engine_command(eval::EXIT_APP_COMMAND) {
                                return self.handle_engine_failure(error, true);
                            }
                            Task::none()
                        }
                    }
                } else if let Event::Window(window::Event::Resized(size)) = event {
                    if !self.mini_ui {
                        self.resolve_window_resize(size)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            (
                _,
                Message::WindowResizeStateResolved {
                    size,
                    maximized,
                    generation,
                },
            ) => {
                self.apply_window_resize_resolution(size, maximized, generation);
                Task::none()
            }
            (_, Message::ResolveMaximizedStatusBeforeExit) => {
                self.resolve_maximized_status_before_exit()
            }
            (_, Message::SaveMaximizedStatusAndExit(is_maximized)) => {
                self.consolidate_pending_window_resize_before_exit(is_maximized);
                self.persist_settings_before_exit();
                if let Some(window_id) = self.window_id {
                    window::close(window_id)
                } else {
                    Task::none()
                }
            }
            (_, Message::EngineFileChosen(engine_path)) => {
                if let Some(engine_path) = engine_path {
                    self.settings_tab.engine_path = engine_path.clone();
                    self.engine.engine_path = engine_path;
                }
                Task::none()
            }
            (_, Message::StartEngine) => {
                match self.engine_state {
                    EngineStatus::TurnedOff => {
                        if self.engine.engine_path.is_empty() {
                            self.record_engine_failure("engine path is empty");
                        } else if !Path::new(&self.engine.engine_path).exists() {
                            self.record_engine_failure("engine executable does not exist");
                        } else {
                            self.engine.position =
                                san_correct_ep(self.analysis.current_position().to_string());
                            self.engine_state = EngineStatus::Started;
                        }
                    }
                    _ => {
                        if let Err(error) = self.send_engine_command(eval::STOP_COMMAND) {
                            return self.handle_engine_failure(error, false);
                        }
                        self.engine_sender = None;
                    }
                }
                Task::none()
            }
            (_, Message::EngineStopped(exit)) => {
                self.clear_engine_state();
                self.final_exit_task(exit)
            }
            (
                _,
                Message::EngineFailed {
                    reason,
                    exit_requested,
                },
            ) => self.handle_engine_failure(reason, exit_requested),
            (_, Message::EngineReady(sender)) => {
                if self.engine_state != EngineStatus::TurnedOff {
                    self.engine_sender = Some(sender);
                }
                Task::none()
            }
            (_, Message::UpdateEval(eval)) => {
                match self.engine_state {
                    EngineStatus::TurnedOff => Task::none(),
                    _ => {
                        let (eval, best_move) = eval;
                        if let Some(eval_str) = eval {
                            if eval_str.contains("Mate") {
                                let tokens: Vec<&str> = eval_str.split_whitespace().collect();
                                let distance_to_mate_num = tokens[2].parse::<i32>().unwrap();
                                match distance_to_mate_num {
                                    1.. => {
                                        self.engine_eval = lang::tr(&self.lang, "mate_in")
                                            + &distance_to_mate_num.to_string();
                                    }
                                    0 => {
                                        self.engine_eval = lang::tr(&self.lang, "mate");
                                        self.engine_move = String::from("");
                                        return Task::none();
                                    }
                                    _ => {
                                        self.engine_eval = lang::tr(&self.lang, "mate_in")
                                            + &(-distance_to_mate_num).to_string();
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
                            && let Some(best_move) = config::coord_to_san(
                                &self.analysis.current_position(),
                                best_move,
                                &self.lang,
                            )
                        {
                            self.engine_move = best_move;
                        }
                        Task::none()
                    }
                }
            }
            (_, Message::StartDBDownload) => {
                if self.settings_tab.is_using_sqlite_puzzles() {
                    let _ = self.settings_tab.update(SettingsMessage::UseCsvPuzzles);
                    if self.settings_tab.is_using_sqlite_puzzles() {
                        return Task::none();
                    }
                }
                self.downloading_db = true;
                self.download_progress.clear();
                Task::none()
            }
            (_, Message::DBDownloadFinished) => {
                self.downloading_db = false;
                self.has_db = true;
                Task::none()
            }
            (_, Message::DBDownloadFailed(error)) => {
                self.downloading_db = false;
                self.download_progress =
                    format!("{}: {error}", lang::tr(&self.lang, "db_download_failed"));
                Task::none()
            }
            (_, Message::DownloadProgress(progress)) => {
                self.download_progress = progress;
                Task::none()
            }
            (_, Message::FavoritePuzzle) => {
                let Some(puzzle) = self.current_reviewable_puzzle().cloned() else {
                    return Task::none();
                };
                if self.current_favorite.is_none() {
                    return Task::none();
                }

                let puzzle_id = puzzle.puzzle_id.clone();
                let generation = self.next_favorite_generation();
                self.current_favorite = None;
                Task::perform(async move { db::toggle_favorite(puzzle) }, move |result| {
                    Message::FavoriteToggled {
                        puzzle_id,
                        generation,
                        result,
                    }
                })
            }
            (_, Message::WindowInitialized(id)) => {
                self.window_id = id;
                self.puzzle_tab.window_id = id;
                iced::window::maximize(self.window_id.unwrap(), self.settings_tab.maximized)
            }
            (_, Message::MinimizeUI) => {
                if self.mini_ui {
                    self.mini_ui = false;
                    let new_size = Size::new(
                        self.settings_tab.window_width,
                        self.settings_tab.window_height,
                    );
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
            }
            (_, Message::DropPiece(square, cursor_pos, _bounds)) => {
                if self.puzzle_tab.game_status == GameStatus::Playing
                    || self.game_mode == config::GameMode::Analysis
                {
                    iced_drop::zones_on_point(
                        move |zones| Message::HandleDropZones(square, zones),
                        cursor_pos,
                        None,
                        None,
                    )
                } else {
                    Task::none()
                }
            }
            (_, Message::HandleDropZones(from, zones)) => {
                if !zones.is_empty() {
                    let id: &GenericId = &zones[0].0.clone();
                    if let Some(to) = self.square_ids.get(id)
                        && self.verify_and_make_move(from, *to)
                    {
                        return self.refresh_current_favorite_status();
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
                        event::listen().map(Message::EventOccurred),
                    ])
                } else {
                    event::listen().map(Message::EventOccurred)
                }
            }
            _ => Subscription::batch(vec![
                Engine::run_engine(self.engine.clone()),
                event::listen().map(Message::EventOccurred),
            ]),
        }
    }

    fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        if self.has_db {
            let has_previous =
                !self.puzzle_tab.puzzles.is_empty() && self.puzzle_tab.current_puzzle > 0;
            let has_more_puzzles = !self.puzzle_tab.puzzles.is_empty()
                && self.puzzle_tab.current_puzzle < self.puzzle_tab.puzzles.len() - 1;
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
                    self.pgn_tab.tab_label(),
                    self.search_tab.view(),
                    self.settings_tab.view(),
                    self.puzzle_tab.view(),
                    self.project_tab.view(),
                    self.pgn_tab.view(),
                    &self.lang,
                    size,
                    self.mini_ui,
                    &self.board_btn_ids,
                    &self.piece_imgs,
                )
            });
            Container::new(resp).padding(1).into()
        } else {
            let mut col = Column::new()
                .push(container(
                    Text::new(lang::tr(&self.lang, "db_not_found"))
                        .size(30)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center),
                ))
                .push(
                    Text::new(lang::tr(&self.lang, "do_you_wanna_download"))
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center),
                )
                .push(
                    Text::new(lang::tr(&self.lang, "download_size_info"))
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center),
                );
            if self.downloading_db {
                col = col
                    .push(
                        container(button(Text::new(lang::tr(&self.lang, "downloading"))))
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .padding(20),
                    )
                    .push(
                        Text::new(&self.download_progress)
                            .size(20)
                            .width(Length::Fill)
                            .align_x(alignment::Horizontal::Center),
                    );
            } else {
                col = col
                    .push(
                        container(
                            button(Text::new(lang::tr(&self.lang, "download_btn")))
                                .on_press(Message::StartDBDownload),
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
                                )),
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
                if !self.download_progress.is_empty() {
                    col = col.push(
                        Text::new(&self.download_progress)
                            .width(Length::Fill)
                            .align_x(alignment::Horizontal::Center),
                    );
                }
            };
            center(col).padding(1).into()
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
#[allow(
    clippy::items_after_test_module,
    reason = "Tests remain beside their behavior to avoid a large source-order-only move."
)]
mod tests {
    use super::*;

    #[test]
    fn synthesized_audio_cues_have_distinct_specs() {
        assert_eq!(cue_spec(AudioCue::OnePiece), CueSpec::new(660.0, 70, 0.12));
        assert_eq!(
            cue_spec(AudioCue::TwoPieces),
            CueSpec::new(880.0, 100, 0.12)
        );
    }

    #[test]
    fn synthesized_audio_cues_are_finite() {
        let one_piece_samples = cue_source(AudioCue::OnePiece).count();
        let two_piece_samples = cue_source(AudioCue::TwoPieces).count();

        assert!(one_piece_samples > 0);
        assert!(two_piece_samples > one_piece_samples);
        assert!(two_piece_samples < 10_000);
    }

    #[test]
    fn unavailable_audio_device_is_a_no_op() {
        assert!(!play_audio_if_available(None, AudioCue::OnePiece));
        assert!(!play_audio_if_available(None, AudioCue::TwoPieces));
    }
    use crate::lang::PickListWrapper;
    use crate::openings::{Openings, Variation};
    use crate::search_tab::{OpeningSide, SearchBase, TacticalThemes};
    use chess_material_studio::models::Puzzle as PersistentPuzzle;
    use chess_material_studio::project::{create_chapter, create_project, set_puzzle_decision};
    use std::collections::HashSet;
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

    struct TempSettingsFile {
        path: PathBuf,
        directory: PathBuf,
    }

    impl TempSettingsFile {
        fn new(label: &str) -> Self {
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_h4_main_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
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

    struct TempScreenshotFile {
        path: PathBuf,
        directory: PathBuf,
    }

    impl TempScreenshotFile {
        fn new(label: &str) -> Self {
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_h9_screenshot_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
            std::fs::create_dir_all(&directory)
                .expect("screenshot test directory should be created");
            Self {
                path: directory.join("screenshot.jpg"),
                directory,
            }
        }
    }

    impl Drop for TempScreenshotFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn screenshot(width: u32, height: u32) -> Screenshot {
        Screenshot::new(
            vec![255; width as usize * height as usize * 4],
            Size::new(width, height),
            1.0,
        )
    }

    fn isolate_search_settings(app: &mut OfflinePuzzles, label: &str) -> TempSettingsFile {
        let settings_file = TempSettingsFile::new(label);
        app.search_tab
            .set_settings_path_for_test(settings_file.path.clone());
        settings_file
    }

    fn navigation_puzzle(id: &str) -> config::Puzzle {
        config::Puzzle {
            puzzle_id: id.into(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".into(),
            moves: "a1a2 h1h2".into(),
            rating: 1500,
            rating_deviation: 80,
            popularity: 50,
            nb_plays: 10,
            themes: "hangingPiece".into(),
            game_url: "https://lichess.org/game".into(),
            opening: String::new(),
        }
    }

    #[test]
    fn engine_failure_resets_coordination_state_and_shows_feedback() {
        let mut app = OfflinePuzzles::new(false);
        let (sender, _receiver) = mpsc::channel(1);
        app.engine_state = EngineStatus::Started;
        app.engine_sender = Some(sender);
        app.engine_eval = "0.42".into();
        app.engine_move = "e4".into();

        let _ = app.update(Message::EngineFailed {
            reason: "stdout closed".into(),
            exit_requested: false,
        });

        assert!(app.engine_state == EngineStatus::TurnedOff);
        assert!(app.engine_sender.is_none());
        assert!(app.engine_eval.is_empty());
        assert!(app.engine_move.is_empty());
        assert!(app.puzzle_status.contains("stdout closed"));
    }

    #[test]
    fn normal_engine_stop_clears_the_sender() {
        let mut app = OfflinePuzzles::new(false);
        let (sender, _receiver) = mpsc::channel(1);
        app.engine_state = EngineStatus::Started;
        app.engine_sender = Some(sender);
        app.engine_eval = "0.42".into();
        app.engine_move = "e4".into();

        let _ = app.update(Message::EngineStopped(false));

        assert!(app.engine_state == EngineStatus::TurnedOff);
        assert!(app.engine_sender.is_none());
        assert!(app.engine_eval.is_empty());
        assert!(app.engine_move.is_empty());
    }

    #[test]
    fn close_after_engine_exit_requires_maximized_state_resolution() {
        assert_eq!(
            OfflinePuzzles::exit_persistence_action(true),
            ExitPersistenceAction::ResolveMaximizedStatus
        );
        assert_eq!(
            OfflinePuzzles::exit_persistence_action(false),
            ExitPersistenceAction::StayOpen
        );
    }

    #[test]
    fn mini_ui_resize_does_not_contaminate_windowed_geometry_used_for_exit_persistence() {
        let mut app = OfflinePuzzles::new(false);
        let settings_file = isolate_search_settings(&mut app, "mini-ui-window-geometry");

        let windowed_generation = app.remember_pending_window_resize(Size::new(1200.0, 800.0));
        assert!(app.apply_window_resize_resolution(
            Size::new(1200.0, 800.0),
            false,
            windowed_generation,
        ));
        app.mini_ui = true;
        app.settings_tab.maximized = true;
        app.persist_settings_before_exit();

        let restored = config::load_config_from_path(&settings_file.path);
        assert_eq!(restored.window_width, 1200.0);
        assert_eq!(restored.window_height, 800.0);
        assert!(restored.maximized);
    }

    #[test]
    fn resize_without_a_window_id_is_ignored_without_panicking() {
        let mut app = OfflinePuzzles::new(false);
        let original_width = app.settings_tab.window_width;
        let original_height = app.settings_tab.window_height;

        let _ = app.update(Message::EventOccurred(Event::Window(
            window::Event::Resized(Size::new(1200.0, 800.0)),
        )));

        assert_eq!(app.window_resize_generation, 1);
        assert!(app.pending_window_resize.is_none());
        assert_eq!(app.settings_tab.window_width, original_width);
        assert_eq!(app.settings_tab.window_height, original_height);
    }

    #[test]
    fn accepted_normal_resize_survives_a_later_mini_ui_transition() {
        let mut app = OfflinePuzzles::new(false);
        let generation = app.remember_pending_window_resize(Size::new(1200.0, 800.0));
        app.mini_ui = true;

        assert!(app.apply_window_resize_resolution(Size::new(1200.0, 800.0), false, generation,));
        assert_eq!(app.settings_tab.window_width, 1200.0);
        assert_eq!(app.settings_tab.window_height, 800.0);
    }

    #[test]
    fn stale_window_resize_resolution_cannot_overwrite_the_latest_resize() {
        let mut app = OfflinePuzzles::new(false);
        let stale_generation = app.remember_pending_window_resize(Size::new(1200.0, 800.0));
        let latest_generation = app.remember_pending_window_resize(Size::new(1250.0, 820.0));

        assert!(app.apply_window_resize_resolution(
            Size::new(1250.0, 820.0),
            false,
            latest_generation,
        ));
        assert!(!app.apply_window_resize_resolution(
            Size::new(1200.0, 800.0),
            false,
            stale_generation,
        ));
        assert_eq!(app.settings_tab.window_width, 1250.0);
        assert_eq!(app.settings_tab.window_height, 820.0);
    }

    #[test]
    fn final_close_consolidates_a_pending_normal_resize_before_persisting() {
        let mut app = OfflinePuzzles::new(false);
        let settings_file = isolate_search_settings(&mut app, "pending-normal-resize-close");
        app.settings_tab
            .record_window_resize(Size::new(1200.0, 800.0), false);
        let generation = app.remember_pending_window_resize(Size::new(1250.0, 820.0));

        app.consolidate_pending_window_resize_before_exit(false);
        assert!(!app.apply_window_resize_resolution(Size::new(1250.0, 820.0), false, generation,));
        app.persist_settings_before_exit();

        let restored = config::load_config_from_path(&settings_file.path);
        assert_eq!(restored.window_width, 1250.0);
        assert_eq!(restored.window_height, 820.0);
        assert!(!restored.maximized);
    }

    #[test]
    fn final_close_preserves_windowed_size_for_a_pending_maximized_resize() {
        let mut app = OfflinePuzzles::new(false);
        let settings_file = isolate_search_settings(&mut app, "pending-maximized-resize-close");
        app.settings_tab
            .record_window_resize(Size::new(1200.0, 800.0), false);
        app.remember_pending_window_resize(Size::new(1900.0, 1000.0));

        app.consolidate_pending_window_resize_before_exit(true);
        app.persist_settings_before_exit();

        let restored = config::load_config_from_path(&settings_file.path);
        assert_eq!(app.settings_tab.window_width, 1900.0);
        assert_eq!(app.settings_tab.window_height, 1000.0);
        assert_eq!(restored.window_width, 1200.0);
        assert_eq!(restored.window_height, 800.0);
        assert!(restored.maximized);
    }

    #[test]
    fn exit_persistence_keeps_live_search_filters_and_window_geometry() {
        let mut app = OfflinePuzzles::new(false);
        let settings_file = isolate_search_settings(&mut app, "exit-persistence");
        let existing = config::OfflinePuzzlesConfig {
            engine_limit: "nodes 77".into(),
            ..config::OfflinePuzzlesConfig::default()
        };
        config::persist_config_to_path(&existing, &settings_file.path)
            .expect("test settings should persist");
        app.settings_tab
            .record_window_resize(Size::new(1234.0, 567.0), false);
        app.settings_tab
            .record_window_resize(Size::new(1900.0, 1000.0), true);

        let _ = app.update(Message::Search(SearchMesssage::SliderMinRatingChanged(
            1500,
        )));
        let _ = app.update(Message::Search(SearchMesssage::SliderMaxRatingChanged(
            2500,
        )));
        let _ = app.update(Message::Search(SearchMesssage::SliderMinPopularityChanged(
            33,
        )));
        let _ = app.update(Message::Search(SearchMesssage::SelectTheme(
            PickListWrapper::new_theme(app.lang, TacticalThemes::Fork),
        )));
        let _ = app.update(Message::Search(SearchMesssage::SelectOpening(
            PickListWrapper::new_opening(app.lang, Openings::Sicilian),
        )));
        let _ = app.update(Message::Search(SearchMesssage::SelectVariation(
            PickListWrapper::new_variation(
                app.lang,
                Variation {
                    name: std::borrow::Cow::Borrowed("Sicilian_Defense_Najdorf_Variation"),
                    family: Openings::Sicilian,
                },
            ),
        )));
        let _ = app.update(Message::Search(SearchMesssage::SelectOpeningSide(
            OpeningSide::Black,
        )));

        app.persist_settings_before_exit();

        let restored = config::load_config_from_path(&settings_file.path);
        assert_eq!(restored.last_min_rating, 1500);
        assert_eq!(restored.last_max_rating, 2500);
        assert_eq!(restored.last_min_popularity, 33);
        assert_eq!(restored.last_theme, TacticalThemes::Fork);
        assert_eq!(restored.last_opening, Openings::Sicilian);
        assert_eq!(
            restored.last_variation,
            Variation {
                name: std::borrow::Cow::Borrowed("Sicilian_Defense_Najdorf_Variation"),
                family: Openings::Sicilian,
            }
        );
        assert_eq!(restored.last_opening_side, Some(OpeningSide::Black));
        assert_eq!(restored.window_width, 1234.0);
        assert_eq!(restored.window_height, 567.0);
        assert!(restored.maximized);
        assert_eq!(restored.engine_limit, "nodes 77");
    }

    #[test]
    fn exit_persistence_attempts_window_save_after_filter_save_failure() {
        let mut app = OfflinePuzzles::new(false);
        let settings_file = TempSettingsFile::new("exit-filter-save-failure");
        let filter_path = settings_file.directory.join("filters.json");
        std::fs::create_dir(filter_path.with_file_name("filters.json.tmp"))
            .expect("filter temporary path should block the first persistence");

        app.settings_tab
            .record_window_resize(Size::new(1234.0, 567.0), false);
        app.settings_tab
            .record_window_resize(Size::new(1900.0, 1000.0), true);

        app.persist_settings_before_exit_to_paths(&filter_path, &settings_file.path);

        let restored = config::load_config_from_path(&settings_file.path);
        assert_eq!(restored.window_width, 1234.0);
        assert_eq!(restored.window_height, 567.0);
        assert!(restored.maximized);
    }

    #[test]
    fn missing_engine_path_keeps_engine_off_and_shows_feedback() {
        let mut app = OfflinePuzzles::new(false);
        let missing_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("h3-engine-path-that-does-not-exist");
        assert!(!missing_path.exists());
        app.engine.engine_path = missing_path.display().to_string();

        let _ = app.update(Message::StartEngine);

        assert!(app.engine_state == EngineStatus::TurnedOff);
        assert!(app.engine_sender.is_none());
        assert!(app.puzzle_status.contains("does not exist"));
    }

    #[test]
    fn database_download_failure_restores_retryable_state_and_shows_detail() {
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::StartDBDownload);
        let _ = app.update(Message::DBDownloadFailed("network unavailable".into()));

        assert!(!app.downloading_db);
        assert!(!app.has_db);
        assert!(
            app.download_progress
                .contains(&lang::tr(&app.lang, "db_download_failed"))
        );
        assert!(app.download_progress.contains("network unavailable"));
    }

    #[test]
    fn database_download_success_marks_database_as_available() {
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::StartDBDownload);
        let _ = app.update(Message::DBDownloadFinished);

        assert!(!app.downloading_db);
        assert!(app.has_db);
    }

    #[test]
    fn database_download_can_restart_after_a_failure() {
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::StartDBDownload);
        let _ = app.update(Message::DBDownloadFailed("temporary failure".into()));
        let _ = app.update(Message::StartDBDownload);

        assert!(app.downloading_db);
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
        assert_eq!(
            app.current_reviewable_puzzle().unwrap().puzzle_id,
            puzzle_id
        );
        assert_eq!(app.project_tab.cached_review_puzzle_id(), Some(puzzle_id));
        assert_eq!(
            app.project_tab.review_view().unwrap().decision,
            Some(decision)
        );
    }

    type PuzzleBatch = Vec<(
        String,
        String,
        String,
        i32,
        i32,
        i32,
        i32,
        String,
        String,
        String,
    )>;

    #[derive(Debug, PartialEq, Eq)]
    struct NormalExportState {
        puzzle_batch: PuzzleBatch,
        current_puzzle: usize,
        current_puzzle_move: usize,
        current_puzzle_side: Color,
        current_puzzle_fen: String,
        puzzle_number_ui: String,
        board: Board,
        game_status: GameStatus,
        game_mode: config::GameMode,
        last_move_from: Option<Square>,
        last_move_to: Option<Square>,
        current_favorite: Option<bool>,
        favorite_generation: u64,
        review_puzzle_id: Option<String>,
        review_view: Option<PuzzleReviewView>,
        reviewed_puzzle_ids: Option<HashSet<String>>,
    }

    fn normal_export_state(app: &OfflinePuzzles) -> NormalExportState {
        NormalExportState {
            puzzle_batch: app
                .puzzle_tab
                .puzzles
                .iter()
                .map(|puzzle| {
                    (
                        puzzle.puzzle_id.clone(),
                        puzzle.fen.clone(),
                        puzzle.moves.clone(),
                        puzzle.rating,
                        puzzle.rating_deviation,
                        puzzle.popularity,
                        puzzle.nb_plays,
                        puzzle.themes.clone(),
                        puzzle.game_url.clone(),
                        puzzle.opening.clone(),
                    )
                })
                .collect(),
            current_puzzle: app.puzzle_tab.current_puzzle,
            current_puzzle_move: app.puzzle_tab.current_puzzle_move,
            current_puzzle_side: app.puzzle_tab.current_puzzle_side,
            current_puzzle_fen: app.puzzle_tab.current_puzzle_fen.clone(),
            puzzle_number_ui: app.puzzle_number_ui.clone(),
            board: app.board,
            game_status: app.puzzle_tab.game_status,
            game_mode: app.game_mode,
            last_move_from: app.last_move_from,
            last_move_to: app.last_move_to,
            current_favorite: app.current_favorite,
            favorite_generation: app.favorite_generation,
            review_puzzle_id: app.project_tab.cached_review_puzzle_id().map(str::to_owned),
            review_view: app.project_tab.review_view(),
            reviewed_puzzle_ids: app
                .project_tab
                .reviewed_puzzle_ids_for_active_chapter()
                .unwrap(),
        }
    }

    fn assert_normal_export_state_preserved(
        app: &OfflinePuzzles,
        expected: &NormalExportState,
        route: &str,
    ) {
        assert_eq!(
            normal_export_state(app).puzzle_batch,
            expected.puzzle_batch,
            "{route}: puzzle batch changed"
        );
        assert_eq!(
            app.puzzle_tab.current_puzzle, expected.current_puzzle,
            "{route}: current puzzle changed"
        );
        assert_eq!(
            app.puzzle_tab.current_puzzle_move, expected.current_puzzle_move,
            "{route}: current puzzle move changed"
        );
        assert_eq!(
            app.puzzle_tab.current_puzzle_side, expected.current_puzzle_side,
            "{route}: current puzzle side changed"
        );
        assert_eq!(
            app.puzzle_tab.current_puzzle_fen, expected.current_puzzle_fen,
            "{route}: current puzzle FEN changed"
        );
        assert_eq!(
            app.puzzle_number_ui, expected.puzzle_number_ui,
            "{route}: puzzle number changed"
        );
        assert_eq!(app.board, expected.board, "{route}: board changed");
        assert_eq!(
            app.puzzle_tab.game_status, expected.game_status,
            "{route}: game status changed"
        );
        assert_eq!(
            app.game_mode, expected.game_mode,
            "{route}: game mode changed"
        );
        assert_eq!(
            app.last_move_from, expected.last_move_from,
            "{route}: last-move source changed"
        );
        assert_eq!(
            app.last_move_to, expected.last_move_to,
            "{route}: last-move destination changed"
        );
        assert_eq!(
            app.current_favorite, expected.current_favorite,
            "{route}: favorite state changed"
        );
        assert_eq!(
            app.favorite_generation, expected.favorite_generation,
            "{route}: favorite generation changed"
        );
        assert_eq!(
            app.project_tab.cached_review_puzzle_id().map(str::to_owned),
            expected.review_puzzle_id,
            "{route}: reviewed puzzle context changed"
        );
        assert_eq!(
            app.project_tab.review_view(),
            expected.review_view,
            "{route}: review view changed"
        );
        assert_eq!(
            app.project_tab
                .reviewed_puzzle_ids_for_active_chapter()
                .unwrap(),
            expected.reviewed_puzzle_ids,
            "{route}: active project or chapter changed"
        );
    }

    fn app_with_normal_export_review_context() -> (OfflinePuzzles, TempProjectDb) {
        let project = TempProjectDb::new("normal-export-review-context");
        create_project(&project.path, "Export context").unwrap();
        let chapter = create_chapter(&project.path, "Current chapter", None).unwrap();
        let puzzles = vec![
            navigation_puzzle("normal-export-first"),
            navigation_puzzle("normal-export-current"),
        ];
        set_puzzle_decision(
            &project.path,
            chapter.id,
            &persistent_puzzle(&puzzles[1]),
            ProjectPuzzleDecision::Selected,
        )
        .unwrap();

        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));
        app.puzzle_tab.puzzles = puzzles;
        app.puzzle_tab.current_puzzle = 1;
        app.load_puzzle(false);
        app.puzzle_number_ui = String::from("2");
        app.current_favorite = Some(true);
        app.favorite_generation = 73;
        (app, project)
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
            set_puzzle_decision(
                &project.path,
                chapter.id,
                &persistent_puzzle(puzzle),
                ProjectPuzzleDecision::Selected,
            )
            .unwrap();
        }
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));

        let _ = app.update(Message::LoadProjectPuzzles(puzzles.clone()));

        assert_eq!(
            app.puzzle_tab
                .puzzles
                .iter()
                .map(|puzzle| puzzle.puzzle_id.as_str())
                .collect::<Vec<_>>(),
            vec!["cms-023h-first", "cms-023h-second", "cms-023h-third"]
        );
        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.puzzle_number_ui, "1");
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert_eq!(
            app.board,
            Board::from_str(&puzzles[0].fen)
                .unwrap()
                .make_move_new(ChessMove::new(Square::A1, Square::A2, None))
        );
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
        let puzzles = vec![
            navigation_puzzle("cms-023h-mutation-first"),
            navigation_puzzle("cms-023h-mutation-second"),
        ];
        for puzzle in &puzzles {
            set_puzzle_decision(
                &project.path,
                chapter.id,
                &persistent_puzzle(puzzle),
                ProjectPuzzleDecision::Selected,
            )
            .unwrap();
        }
        let mut app = OfflinePuzzles::new(false);
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));
        let _ = app.update(Message::LoadProjectPuzzles(puzzles.clone()));

        let _ = app.update(Message::SetPuzzleReview(ProjectPuzzleDecision::Discarded));

        assert_eq!(app.puzzle_tab.puzzles.len(), puzzles.len());
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, puzzles[0].puzzle_id);
        assert_eq!(app.puzzle_tab.puzzles[1].puzzle_id, puzzles[1].puzzle_id);
        assert_current_review(
            &app,
            "cms-023h-mutation-first",
            ProjectPuzzleDecision::Discarded,
        );

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
        let _settings_file = isolate_search_settings(&mut app, "search-preparation");
        let loaded = navigation_puzzle("already-loaded");
        app.puzzle_tab.puzzles = vec![loaded.clone()];
        app.puzzle_tab.current_puzzle = 0;
        app.puzzle_tab.game_status = GameStatus::Playing;
        let _ = app.update(Message::Project(ProjectMessage::ProjectToOpenChosen(Some(
            project.path.clone(),
        ))));
        std::fs::remove_file(&project.path).unwrap();
        let stale_generation = app.search_generation;

        let _ = app.update(Message::Search(SearchMesssage::ClickSearch));

        assert!(!app.search_tab.show_searching_msg);
        assert_ne!(app.search_generation, stale_generation);
        assert_eq!(app.puzzle_tab.puzzles.len(), 1);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "review_error"))
        );

        let status_after_failed_preparation = app.puzzle_status.clone();
        let _ = app.update(Message::LoadPuzzle {
            generation: stale_generation,
            result: Ok(vec![navigation_puzzle("stale-after-failed-preparation")]),
        });
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
        assert_eq!(app.puzzle_status, status_after_failed_preparation);

        let _ = app.update(Message::Search(SearchMesssage::SelectBase(
            SearchBase::Favorites,
        )));
        let _ = app.update(Message::Search(SearchMesssage::ClickSearch));
        assert!(app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles.len(), 1);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, loaded.puzzle_id);
    }

    #[test]
    fn lichess_search_without_a_project_starts_normally() {
        let mut app = OfflinePuzzles::new(false);
        let _settings_file = isolate_search_settings(&mut app, "lichess-search");

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

    fn normal_export_test_path(extension: &str) -> PathBuf {
        let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_normal_export_main_tests");
        std::fs::create_dir_all(&directory)
            .expect("normal export test directory should be created");
        directory.join(format!(
            "normal-export-{}-{sequence}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn normal_exports_report_success_cancellation_and_errors_without_changing_the_batch() {
        let (mut app, _project) = app_with_normal_export_review_context();
        let original_state = normal_export_state(&app);
        assert_eq!(original_state.current_puzzle, 1);
        assert_current_review(
            &app,
            "normal-export-current",
            ProjectPuzzleDecision::Selected,
        );

        let _ = app.update(Message::ExportPGN(None));
        assert_eq!(
            app.puzzle_status,
            lang::tr(&app.lang, "normal_pgn_export_cancelled")
        );
        assert_normal_export_state_preserved(&app, &original_state, "PGN cancellation");

        let pgn_path = normal_export_test_path("pgn");
        let _ = app.update(Message::ExportPGN(Some(pgn_path.display().to_string())));
        assert_eq!(
            app.puzzle_status,
            lang::tr(&app.lang, "normal_pgn_exported")
        );
        assert!(pgn_path.is_file());
        assert_normal_export_state_preserved(&app, &original_state, "PGN success");
        let _ = std::fs::remove_file(&pgn_path);

        let pgn_missing_parent = normal_export_test_path("missing").join("output.pgn");
        let _ = app.update(Message::ExportPGN(Some(
            pgn_missing_parent.display().to_string(),
        )));
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "normal_pgn_export_failed"))
        );
        assert!(app.puzzle_status.contains("Error writing PGN file"));
        assert_normal_export_state_preserved(&app, &original_state, "PGN error");

        let _ = app.update(Message::ExportPDF(None));
        assert_eq!(
            app.puzzle_status,
            lang::tr(&app.lang, "normal_pdf_export_cancelled")
        );
        assert_normal_export_state_preserved(&app, &original_state, "PDF cancellation");

        let pdf_path = normal_export_test_path("pdf");
        let _ = app.update(Message::ExportPDF(Some(pdf_path.display().to_string())));
        assert_eq!(
            app.puzzle_status,
            lang::tr(&app.lang, "normal_pdf_exported")
        );
        assert!(pdf_path.is_file());
        assert_normal_export_state_preserved(&app, &original_state, "PDF success");
        let _ = std::fs::remove_file(&pdf_path);

        let pdf_missing_parent = normal_export_test_path("missing").join("output.pdf");
        let _ = app.update(Message::ExportPDF(Some(
            pdf_missing_parent.display().to_string(),
        )));
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "normal_pdf_export_failed"))
        );
        assert!(app.puzzle_status.contains("Error writing PDF file"));
        assert_normal_export_state_preserved(&app, &original_state, "PDF error");
    }

    #[test]
    fn screenshot_helper_saves_a_jpeg() {
        let output = TempScreenshotFile::new("successful-save");

        save_screenshot_to_path(
            &screenshot(20, 20),
            Rectangle::<u32> {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            &output.path,
        )
        .expect("valid screenshot should save");

        assert!(output.path.is_file());
        assert!(std::fs::metadata(&output.path).unwrap().len() > 0);
    }

    #[test]
    fn screenshot_helper_propagates_write_failure() {
        let output = TempScreenshotFile::new("write-failure");
        let missing_parent_path = output
            .directory
            .join("missing-parent")
            .join("screenshot.jpg");

        let error = save_screenshot_to_path(
            &screenshot(20, 20),
            Rectangle::<u32> {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            &missing_parent_path,
        )
        .expect_err("a missing destination parent must fail");

        assert!(error.contains("failed to save JPEG"));
    }

    #[test]
    fn screenshot_helper_propagates_crop_failure() {
        let output = TempScreenshotFile::new("crop-failure");

        let error = save_screenshot_to_path(
            &screenshot(10, 10),
            Rectangle::<u32> {
                x: 0,
                y: 0,
                width: 11,
                height: 10,
            },
            &output.path,
        )
        .expect_err("an out-of-bounds crop must fail");

        assert!(error.contains("failed to crop screenshot"));
    }

    #[test]
    fn invalid_screenshot_crop_dimensions_are_rejected_before_conversion() {
        let error = screenshot_crop_rectangle(false, 135.0)
            .expect_err("zero-sized screenshot crops are unusable");

        assert!(error.contains("crop dimensions are invalid"));
    }

    #[test]
    fn invalid_screenshot_rgba_buffer_is_propagated() {
        let invalid = Screenshot::new(vec![0; 3], Size::new(1, 1), 1.0);

        let error = screenshot_rgba_image(invalid)
            .expect_err("an invalid RGBA buffer must not be accepted");

        assert!(error.contains("RGBA buffer does not match"));
    }

    #[test]
    fn screenshot_statuses_preserve_normal_puzzle_state() {
        let (mut app, _project) = app_with_normal_export_review_context();
        app.settings_tab.window_height = 200.0;
        app.settings_tab.show_coordinates = false;
        let original_state = normal_export_state(&app);
        let output = TempScreenshotFile::new("status-success");

        let _ = app.update(Message::SaveScreenshot(None));
        assert_eq!(
            app.puzzle_status,
            lang::tr(&app.lang, "screenshot_cancelled")
        );
        assert_normal_export_state_preserved(&app, &original_state, "screenshot cancellation");

        let _ = app.update(Message::SaveScreenshot(Some((
            screenshot(100, 100),
            output.path.clone(),
        ))));
        assert_eq!(app.puzzle_status, lang::tr(&app.lang, "screenshot_saved"));
        assert!(output.path.is_file());
        assert_normal_export_state_preserved(&app, &original_state, "screenshot success");

        let _ = app.update(Message::SaveScreenshot(Some((
            screenshot(1, 1),
            output.directory.join("crop-failure.jpg"),
        ))));
        assert!(
            app.puzzle_status
                .starts_with(&lang::tr(&app.lang, "screenshot_failed"))
        );
        assert!(app.puzzle_status.contains("failed to crop screenshot"));
        assert_normal_export_state_preserved(&app, &original_state, "screenshot failure");

        let _ = app.update(Message::ScreenshotFailed(
            "screenshot window is not initialized".into(),
        ));
        assert!(
            app.puzzle_status
                .starts_with(&lang::tr(&app.lang, "screenshot_failed"))
        );
        assert!(
            app.puzzle_status
                .contains("screenshot window is not initialized")
        );
        assert_normal_export_state_preserved(&app, &original_state, "missing screenshot window");
    }

    #[test]
    fn normal_export_status_translation_keys_exist_in_every_language() {
        const NORMAL_EXPORT_KEYS: [&str; 9] = [
            "normal_pgn_exported",
            "normal_pgn_export_cancelled",
            "normal_pgn_export_failed",
            "normal_pdf_exported",
            "normal_pdf_export_cancelled",
            "normal_pdf_export_failed",
            "screenshot_saved",
            "screenshot_cancelled",
            "screenshot_failed",
        ];

        for language in lang::Language::ALL {
            for key in NORMAL_EXPORT_KEYS {
                assert!(!lang::tr(&language, key).is_empty(), "missing {key}");
            }
        }
    }

    #[test]
    fn favorite_search_error_preserves_the_loaded_puzzle() {
        let mut app = app_with_current_puzzle("loaded-favorite");
        let board = app.board;
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadFavorites {
            generation: app.search_generation,
            result: Err("controlled search failure".into()),
        });

        assert!(!app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "loaded-favorite");
        assert_eq!(app.board, board);
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "my_favories"))
        );
        assert!(app.puzzle_status.contains("controlled search failure"));
    }

    #[test]
    fn empty_favorite_search_keeps_the_historical_no_puzzles_behavior() {
        let mut app = app_with_current_puzzle("loaded-favorite");

        let _ = app.update(Message::LoadFavorites {
            generation: app.search_generation,
            result: Ok(Vec::new()),
        });

        assert_eq!(app.puzzle_tab.game_status, GameStatus::NoPuzzles);
        assert_eq!(app.puzzle_status, lang::tr(&app.lang, "no_puzzle_found"));
        assert_eq!(app.current_favorite, None);
    }

    #[test]
    fn normal_search_error_preserves_the_loaded_puzzle() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let board = app.board;
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadPuzzle {
            generation: app.search_generation,
            result: Err("controlled search failure".into()),
        });

        assert!(!app.search_tab.show_searching_msg);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "loaded-normal");
        assert_eq!(app.board, board);
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert!(app.puzzle_status.contains(&lang::tr(&app.lang, "search")));
        assert!(app.puzzle_status.contains("controlled search failure"));
    }

    #[test]
    fn invalid_normal_search_batch_preserves_the_loaded_context() {
        let (mut app, _project) = app_with_normal_export_review_context();
        let expected = normal_export_state(&app);
        app.search_tab.show_searching_msg = true;
        let mut invalid = navigation_puzzle("invalid-normal-search");
        invalid.moves = "a1a2 i1i2".into();

        let _ = app.update(Message::LoadPuzzle {
            generation: app.search_generation,
            result: Ok(vec![navigation_puzzle("valid-normal-search"), invalid]),
        });

        assert!(!app.search_tab.show_searching_msg);
        assert_normal_export_state_preserved(&app, &expected, "normal search invalid batch");
        assert!(app.puzzle_status.contains("invalid-normal-search"));
        assert!(app.puzzle_status.contains("invalid source square"));
    }

    #[test]
    fn valid_normal_search_batch_replaces_the_loaded_context() {
        let mut app = app_with_current_puzzle("old-normal-search");

        let _ = app.update(Message::LoadPuzzle {
            generation: app.search_generation,
            result: Ok(vec![navigation_puzzle("new-normal-search")]),
        });

        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "new-normal-search");
        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert_eq!(app.puzzle_number_ui, "1");
    }

    #[test]
    fn invalid_favorite_batch_preserves_the_loaded_context() {
        let (mut app, _project) = app_with_normal_export_review_context();
        let expected = normal_export_state(&app);
        let mut invalid = navigation_puzzle("invalid-favorite");
        invalid.moves = "a1a2 h1h2 a2a4".into();

        let _ = app.update(Message::LoadFavorites {
            generation: app.search_generation,
            result: Ok(vec![invalid]),
        });

        assert_normal_export_state_preserved(&app, &expected, "favorites invalid batch");
        assert!(app.puzzle_status.contains("invalid-favorite"));
        assert!(app.puzzle_status.contains("illegal move 'a2a4'"));
    }

    #[test]
    fn valid_favorite_batch_replaces_the_loaded_context() {
        let mut app = app_with_current_puzzle("old-favorite");

        let _ = app.update(Message::LoadFavorites {
            generation: app.search_generation,
            result: Ok(vec![navigation_puzzle("new-favorite")]),
        });

        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "new-favorite");
        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.puzzle_tab.game_status, GameStatus::Playing);
        assert_eq!(app.puzzle_number_ui, "1");
    }

    #[test]
    fn invalid_project_selected_snapshot_preserves_the_loaded_context() {
        let (mut app, _project) = app_with_normal_export_review_context();
        let expected = normal_export_state(&app);
        let mut invalid = navigation_puzzle("invalid-project-selected");
        invalid.fen = "invalid FEN".into();

        let _ = app.update(Message::LoadProjectPuzzles(vec![
            navigation_puzzle("valid-project-selected"),
            invalid,
        ]));

        assert_normal_export_state_preserved(&app, &expected, "project selected invalid batch");
        assert!(app.puzzle_status.contains("invalid-project-selected"));
        assert!(app.puzzle_status.contains("invalid FEN"));
    }

    #[test]
    fn empty_normal_search_keeps_the_historical_no_puzzles_behavior() {
        let mut app = app_with_current_puzzle("loaded-normal");

        let _ = app.update(Message::LoadPuzzle {
            generation: app.search_generation,
            result: Ok(Vec::new()),
        });

        assert_eq!(app.puzzle_tab.game_status, GameStatus::NoPuzzles);
        assert_eq!(app.puzzle_status, lang::tr(&app.lang, "no_puzzle_found"));
        assert_eq!(app.current_favorite, None);
    }

    #[test]
    fn stale_normal_search_result_cannot_replace_a_newer_result() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let first_generation = app.next_search_generation();
        let second_generation = app.next_search_generation();

        let _ = app.update(Message::LoadPuzzle {
            generation: second_generation,
            result: Ok(vec![navigation_puzzle("newer-normal-search")]),
        });
        let _ = app.update(Message::LoadPuzzle {
            generation: first_generation,
            result: Ok(vec![navigation_puzzle("stale-normal-search")]),
        });

        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "newer-normal-search");
    }

    #[test]
    fn stale_search_result_cannot_hide_a_newer_pending_search() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let first_generation = app.next_search_generation();
        let _second_generation = app.next_search_generation();
        let expected = normal_export_state(&app);
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadPuzzle {
            generation: first_generation,
            result: Ok(vec![navigation_puzzle("stale-normal-search")]),
        });

        assert!(app.search_tab.show_searching_msg);
        assert_normal_export_state_preserved(&app, &expected, "stale pending search");
    }

    #[test]
    fn stale_search_error_is_invisible() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let first_generation = app.next_search_generation();
        let second_generation = app.next_search_generation();

        let _ = app.update(Message::LoadPuzzle {
            generation: second_generation,
            result: Ok(vec![navigation_puzzle("newer-normal-search")]),
        });
        let status_before_stale_error = app.puzzle_status.clone();
        let board_before_stale_error = app.board;
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadPuzzle {
            generation: first_generation,
            result: Err("stale search failure".into()),
        });

        assert_eq!(app.puzzle_status, status_before_stale_error);
        assert_eq!(app.puzzle_tab.puzzles[0].puzzle_id, "newer-normal-search");
        assert_eq!(app.board, board_before_stale_error);
        assert!(app.search_tab.show_searching_msg);
    }

    #[test]
    fn stale_lichess_result_cannot_replace_newer_favorites_result() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let lichess_generation = app.next_search_generation();
        let favorites_generation = app.next_search_generation();

        let _ = app.update(Message::LoadFavorites {
            generation: favorites_generation,
            result: Ok(vec![navigation_puzzle("newer-favorites-search")]),
        });
        let _ = app.update(Message::LoadPuzzle {
            generation: lichess_generation,
            result: Ok(vec![navigation_puzzle("stale-lichess-search")]),
        });

        assert_eq!(
            app.puzzle_tab.puzzles[0].puzzle_id,
            "newer-favorites-search"
        );
    }

    #[test]
    fn only_the_current_search_response_can_clear_searching_message() {
        let mut app = app_with_current_puzzle("loaded-normal");
        let stale_generation = app.next_search_generation();
        let current_generation = app.next_search_generation();
        app.search_tab.show_searching_msg = true;

        let _ = app.update(Message::LoadFavorites {
            generation: stale_generation,
            result: Ok(Vec::new()),
        });
        assert!(app.search_tab.show_searching_msg);

        let _ = app.update(Message::LoadFavorites {
            generation: current_generation,
            result: Ok(Vec::new()),
        });
        assert!(!app.search_tab.show_searching_msg);
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
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "my_favories"))
        );
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
        assert!(
            app.puzzle_status
                .contains(&lang::tr(&app.lang, "my_favories"))
        );
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

        assert_eq!(
            app.current_reviewable_puzzle().unwrap().puzzle_id,
            puzzle_id_before_redo
        );
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
            vec![(
                GenericId::new(config::BTN_IDS[Square::A2.to_index()]),
                Rectangle::default(),
            )],
        ));

        assert_eq!(app.puzzle_tab.current_puzzle, 1);
        assert_eq!(
            app.current_reviewable_puzzle().unwrap().puzzle_id,
            "drag-second"
        );
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
            vec![(
                GenericId::new(config::BTN_IDS[Square::B1.to_index()]),
                Rectangle::default(),
            )],
        ));

        assert_eq!(app.puzzle_tab.current_puzzle, 0);
        assert_eq!(app.current_favorite, Some(true));
        assert_eq!(app.favorite_generation, 41);
    }
}

fn screenshot_crop_rectangle(
    show_coordinates: bool,
    window_height: f32,
) -> Result<Rectangle<u32>, String> {
    let (crop_height, crop_width) = if show_coordinates {
        (window_height - 125., window_height - 130.)
    } else {
        (window_height - 135., window_height - 135.)
    };

    if !crop_width.is_finite()
        || !crop_height.is_finite()
        || crop_width <= 0.
        || crop_height <= 0.
        || crop_width > u32::MAX as f32
        || crop_height > u32::MAX as f32
    {
        return Err("screenshot crop dimensions are invalid".into());
    }

    Ok(Rectangle::<u32> {
        x: 0,
        y: 0,
        width: crop_width as u32,
        height: crop_height as u32,
    })
}

fn save_screenshot_to_path(
    screenshot: &Screenshot,
    crop: Rectangle<u32>,
    path: &Path,
) -> Result<(), String> {
    validate_screenshot_rgba_buffer(screenshot)?;
    let screenshot = screenshot
        .crop(crop)
        .map_err(|error| format!("failed to crop screenshot: {error}"))?;
    let image = screenshot_rgba_image(screenshot)?;
    let rgb_img = DynamicImage::ImageRgba8(image).into_rgb8();
    rgb_img
        .save_with_format(path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("failed to save JPEG: {error}"))
}

fn validate_screenshot_rgba_buffer(screenshot: &Screenshot) -> Result<(), String> {
    let expected_len = (screenshot.size.width as usize)
        .checked_mul(screenshot.size.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "screenshot dimensions are too large".to_string())?;

    if screenshot.rgba.len() != expected_len {
        return Err("screenshot RGBA buffer does not match its dimensions".into());
    }

    Ok(())
}

fn screenshot_rgba_image(screenshot: Screenshot) -> Result<RgbaImage, String> {
    RgbaImage::from_raw(
        screenshot.size.width,
        screenshot.size.height,
        screenshot.rgba.to_vec(),
    )
    .ok_or_else(|| "screenshot RGBA buffer does not match its dimensions".to_string())
}

pub async fn screenshot_save_dialog(img: Screenshot) -> Option<(Screenshot, PathBuf)> {
    let file_path = AsyncFileDialog::new()
        .add_filter("jpg", &["jpg", "jpeg"])
        .save_file()
        .await;
    file_path.map(|file_path| (img, file_path.path().to_path_buf()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "View construction mirrors application state; restructuring it is outside this baseline."
)]
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
    _piece_theme: styles::PieceTheme,
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
    pgn_tab_label: TabLabel,
    search_tab: Element<'a, Message, Theme, iced::Renderer>,
    settings_tab: Element<'a, Message, Theme, iced::Renderer>,
    puzzle_tab: Element<'a, Message, Theme, iced::Renderer>,
    project_tab: Element<'a, Message, Theme, iced::Renderer>,
    pgn_tab: Element<'a, Message, Theme, iced::Renderer>,
    lang: &lang::Language,
    size: Size,
    mini_ui: bool,
    board_ids: &[GenericId],
    imgs: &[Handle],
) -> Element<'a, Message, Theme, iced::Renderer> {
    let mut board_col = Column::new().spacing(0).align_x(Alignment::Center);
    let mut board_row = Row::new().spacing(0).align_y(Alignment::Center);

    let is_white = (current_puzzle_side == Color::White) ^ flip_board;

    let board_controls_height = 135.
        + if show_coordinates { 10. } else { 0. }
        + if engine_eval.is_empty() { 0. } else { 30. }
        + if puzzle_review.is_some() { 55. } else { 0. };
    let board_height = (size.height - board_controls_height) / 8.;

    let (ranks, files) = if is_white {
        (
            (0..8).rev().collect::<Vec<i32>>(),
            (0..8).collect::<Vec<i32>>(),
        )
    } else {
        (
            (0..8).collect::<Vec<i32>>(),
            (0..8).rev().collect::<Vec<i32>>(),
        )
    };
    for rank in ranks {
        for file in &files {
            let pos = Square::make_square(
                Rank::from_index(rank as usize),
                File::from_index(*file as usize),
            );

            let (piece, color) = match game_mode {
                config::GameMode::Analysis => (analysis.piece_on(pos), analysis.color_on(pos)),
                config::GameMode::Puzzle => (board.piece_on(pos), board.color_on(pos)),
            };

            let light_square = (rank + file) % 2 != 0;

            let selected =
                if game_mode == config::GameMode::Puzzle && game_status == GameStatus::Playing {
                    from_square == Some(pos)
                        || last_move_from == Some(pos)
                        || last_move_to == Some(pos)
                        || hint_square == Some(pos)
                } else {
                    from_square == Some(pos)
                };
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
                    square_style =
                        styles::board_button_style(board_theme, styles::BoardSquareStyle::Light);
                    container_style =
                        styles::board_container_style(board_theme, styles::BoardSquareStyle::Light);
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
                    square_style =
                        styles::board_button_style(board_theme, styles::BoardSquareStyle::Dark);
                    container_style =
                        styles::board_container_style(board_theme, styles::BoardSquareStyle::Dark);
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
                            Svg::new(imgs[piece_index].clone())
                                .width(board_height)
                                .height(board_height),
                        )
                        .drag_hide(true)
                        .drag_center(true)
                        .on_drop(move |point, rect| Message::DropPiece(pos, point, rect))
                        .on_click(Message::SelectSquare(pos)),
                    )
                    .style(container_style)
                    .id(board_ids[pos.to_index()].clone()),
                );
            } else {
                board_row = board_row.push(
                    container(
                        Button::new(Text::new(""))
                            .width(board_height)
                            .height(board_height)
                            .on_press(Message::SelectSquare(pos))
                            .style(square_style),
                    )
                    .id(board_ids[pos.to_index()].clone()),
                );
            }
        }

        if show_coordinates {
            board_row = board_row.push(
                Container::new(Text::new((rank + 1).to_string()).size(15))
                    .align_y(iced::alignment::Vertical::Bottom)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(3)
                    .height(board_height),
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
        Radio::new(
            lang::tr(lang, "mode_puzzle"),
            config::GameMode::Puzzle,
            Some(game_mode),
            Message::SelectMode
        )
        .style(styles::radio_style),
        Radio::new(
            lang::tr(lang, "mode_analysis"),
            config::GameMode::Analysis,
            Some(game_mode),
            Message::SelectMode
        )
        .style(styles::radio_style)
    ]
    .spacing(10)
    .padding(10)
    .align_y(Alignment::Center);

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
            navigation_row = navigation_row.push(
                Button::new(Text::new(lang::tr(lang, "takeback")))
                    .on_press(Message::GoBackMove)
                    .style(btn_style_simple),
            );
        } else {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "takeback"))).style(btn_style_simple));
        }
        if engine_started {
            navigation_row = navigation_row.push(
                Button::new(Text::new(lang::tr(lang, "stop_engine")))
                    .on_press(Message::StartEngine)
                    .style(btn_style_simple),
            );
        } else {
            navigation_row = navigation_row.push(
                Button::new(Text::new(lang::tr(lang, "start_engine")))
                    .on_press(Message::StartEngine)
                    .style(btn_style_simple),
            );
        }
    } else {
        if has_previous {
            navigation_row = navigation_row.push(
                Button::new(Text::new(lang::tr(lang, "previous")))
                    .on_press(Message::ShowPreviousPuzzle)
                    .style(btn_style_simple),
            )
        } else {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "previous"))).style(btn_style_simple));
        }
        if has_more_puzzles {
            navigation_row = navigation_row.push(
                Button::new(Text::new(lang::tr(lang, "next")))
                    .on_press(Message::ShowNextPuzzle)
                    .style(btn_style_simple),
            )
        } else {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "next"))).style(btn_style_simple));
        }
        if game_status == GameStatus::NoPuzzles {
            navigation_row = navigation_row
                .push(Button::new(Text::new(lang::tr(lang, "redo"))).style(btn_style_simple))
                .push(favorite_button)
                .push(Button::new(Text::new(lang::tr(lang, "hint"))).style(btn_style_simple));
        } else if game_status == GameStatus::PuzzleEnded {
            navigation_row = navigation_row
                .push(
                    Button::new(Text::new(lang::tr(lang, "redo")))
                        .on_press(Message::RedoPuzzle)
                        .style(btn_style_simple),
                )
                .push(favorite_button)
                .push(Button::new(Text::new(lang::tr(lang, "hint"))).style(btn_style_simple));
        } else {
            navigation_row = navigation_row
                .push(
                    Button::new(Text::new(lang::tr(lang, "redo")))
                        .on_press(Message::RedoPuzzle)
                        .style(btn_style_simple),
                )
                .push(favorite_button)
                .push(
                    Button::new(Text::new(lang::tr(lang, "hint")))
                        .on_press(Message::ShowHint)
                        .style(btn_style_simple),
                );
        }
    }

    let (input_index, btn_go) = if game_status == GameStatus::Playing {
        (
            text_input(puzzle_number_ui, puzzle_number_ui)
                .on_input(Message::PuzzleInputIndexChange)
                .width(Length::Fixed(150.)),
            button(text(lang::tr(lang, "go")))
                .on_press(Message::JumpToPuzzle)
                .style(btn_style_simple),
        )
    } else {
        (
            text_input(puzzle_number_ui, puzzle_number_ui).width(Length::Fixed(150.)),
            button(text(lang::tr(lang, "go"))).style(btn_style_simple),
        )
    };

    let pagination_row = row![
        text(lang::tr(lang, "puzzle")),
        input_index,
        text(lang::tr(lang, "of") + &total_puzzles.to_string()),
        btn_go
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    board_col = board_col
        .push(Text::new(puzzle_status))
        .push(game_mode_row)
        .push(navigation_row)
        .push(pagination_row);
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
        let mut select_button =
            Button::new(Text::new(lang::tr(lang, "select_puzzle"))).style(btn_style_simple);
        let mut discard_button =
            Button::new(Text::new(lang::tr(lang, "discard_puzzle"))).style(btn_style_simple);
        let mut clear_button =
            Button::new(Text::new(lang::tr(lang, "clear_review"))).style(btn_style_simple);
        if review.decision_loaded && review.decision != Some(ProjectPuzzleDecision::Selected) {
            select_button =
                select_button.on_press(Message::SetPuzzleReview(ProjectPuzzleDecision::Selected));
        }
        if review.decision_loaded && review.decision != Some(ProjectPuzzleDecision::Discarded) {
            discard_button =
                discard_button.on_press(Message::SetPuzzleReview(ProjectPuzzleDecision::Discarded));
        }
        if review.decision_loaded && review.decision.is_some() {
            clear_button = clear_button.on_press(Message::ClearPuzzleReview);
        }
        board_col = board_col.push(
            Column::new()
                .spacing(5)
                .push(Text::new(format!(
                    "{}: {}   {}: {}",
                    lang::tr(lang, "active_chapter"),
                    review.chapter_name,
                    lang::tr(lang, "review_status"),
                    review_status
                )))
                .push(
                    Row::new()
                        .spacing(10)
                        .push(select_button)
                        .push(discard_button)
                        .push(clear_button),
                )
                .push(Text::new(review.status)),
        );
    }
    if !engine_eval.is_empty() {
        board_col = board_col.push(
            row![
                Text::new(lang::tr(lang, "eval") + engine_eval),
                Text::new(lang::tr(lang, "best_move") + engine_move)
            ]
            .padding(5)
            .spacing(15),
        );
    }
    if mini_ui {
        let button_mini = Button::new(Text::new(">"))
            .on_press(Message::MinimizeUI)
            .style(btn_style_simple);
        row![board_col, button_mini]
            .spacing(5)
            .align_y(Alignment::Start)
            .into()
    } else {
        let button_mini = Button::new(Text::new("<"))
            .on_press(Message::MinimizeUI)
            .style(btn_style_simple);
        let tabs = Tabs::new(Message::TabSelected)
            .push(TabId::Search, search_tab_label, search_tab)
            .push(TabId::Settings, settings_tab_label, settings_tab)
            .push(TabId::CurrentPuzzle, puzzle_tab_label, puzzle_tab)
            .push(TabId::Project, project_tab_label, project_tab)
            .push(TabId::Pgn, pgn_tab_label, pgn_tab)
            .tab_bar_position(iced_aw::TabBarPosition::Top)
            .tab_bar_style(styles::tab_style)
            .set_active_tab(active_tab);

        row![board_col, button_mini, tabs]
            .spacing(5)
            .align_y(Alignment::Start)
            .into()
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
            height: config::SETTINGS.window_height, //(config::SETTINGS.square_size * 8) as u32 + 120,
        },
        resizable: true,
        exit_on_close_request: false,
        ..iced::window::Settings::default()
    };

    iced::application(
        OfflinePuzzles::init,
        OfflinePuzzles::update,
        OfflinePuzzles::view,
    )
    .theme(OfflinePuzzles::theme)
    .subscription(OfflinePuzzles::subscription)
    .window(window_settings)
    .title("Chess Material Studio")
    .run()
}
