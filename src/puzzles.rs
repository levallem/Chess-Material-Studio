use iced::widget::{Container, column as col, row, Scrollable, Text, TextInput, Button};
use iced::window::Id;
use iced::{alignment, Alignment, Element, Length, Task, Theme};
use chess::{Board, ChessMove, Color, Piece, Square};
use std::str::FromStr;
use iced_aw::TabLabel;
use rfd::AsyncFileDialog;

use crate::styles::btn_style_simple;
use crate::{Message, Tab, config, lang};

#[derive(Debug, Clone)]
pub enum PuzzleMessage {
    ChangeTextInputs(String),
    CopyText(String),
    OpenLink(String),
    TakeScreenshot,
    ExportToPDF,
    ExportToPGN
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GameStatus {
    Playing, PuzzleEnded, NoPuzzles,
}

#[derive(Debug, Clone)]
pub struct PuzzleTab {
    pub window_id: Option<Id>,
    pub puzzles: Vec<config::Puzzle>,
    pub current_puzzle: usize,
    pub current_puzzle_move: usize,
    pub current_puzzle_side: Color,
    pub game_status: GameStatus,
    pub current_puzzle_fen: String,
    pub lang: lang::Language,
}

impl PuzzleTab {
    pub fn new() -> Self {
        PuzzleTab {
            window_id: None,
            puzzles: Vec::new(),
            current_puzzle: 0,
            current_puzzle_move: 1,
            current_puzzle_side: Color::White,
            game_status: GameStatus::NoPuzzles,
            current_puzzle_fen: String::new(),
            lang: config::SETTINGS.lang,
        }
    }

    pub fn update(&mut self, message: PuzzleMessage) -> Task<Message> {
        match message {
            PuzzleMessage::ChangeTextInputs(_) => {
                Task::none()
            } PuzzleMessage::CopyText(text) => {
                iced::clipboard::write::<Message>(text)
            } PuzzleMessage::OpenLink(link) => {
                let _ = open::that_detached(link);
                Task::none()
            } PuzzleMessage::TakeScreenshot => {
                iced::window::screenshot(self.window_id.unwrap()).map(Message::ScreenshotCreated)
            } PuzzleMessage::ExportToPDF => {
                Task::perform(PuzzleTab::export("pdf"), Message::ExportPDF)
            } PuzzleMessage::ExportToPGN => {
                Task::perform(PuzzleTab::export("pgn"), Message::ExportPGN)
            }
        }
    }

    pub async fn export(format: &str) -> Option<String> {
        let file_path = AsyncFileDialog::new().add_filter(format,&[format]).set_file_name(String::from("puzzles.") + format).save_file().await;
        file_path.map(|file_path| file_path.path().display().to_string())
    }

    // Checks if the notation indicates a promotion and return the piece
    // if that's the case.
    pub fn check_promotion(notation: &str) -> Option<Piece> {
        match notation.as_bytes().get(4) {
            Some(b'r') => Some(Piece::Rook),
            Some(b'n') => Some(Piece::Knight),
            Some(b'b') => Some(Piece::Bishop),
            Some(b'q') => Some(Piece::Queen),
            _ => None,
        }
    }
}

/// Parses the narrow UCI subset used by the puzzle resolver without relying on
/// byte slices unless the token has already been proven ASCII and fixed-width.
pub fn parse_uci_move(token: &str) -> Result<ChessMove, String> {
    if !token.is_ascii() || !(token.len() == 4 || token.len() == 5) {
        return Err(format!("invalid move token '{token}'"));
    }

    let source = Square::from_str(&token[..2])
        .map_err(|_| format!("invalid move token '{token}': invalid source square"))?;
    let destination = Square::from_str(&token[2..4])
        .map_err(|_| format!("invalid move token '{token}': invalid destination square"))?;
    let promotion = match token.as_bytes().get(4) {
        None => None,
        Some(b'q') => Some(Piece::Queen),
        Some(b'r') => Some(Piece::Rook),
        Some(b'b') => Some(Piece::Bishop),
        Some(b'n') => Some(Piece::Knight),
        Some(_) => return Err(format!("invalid move token '{token}': invalid promotion")),
    };

    Ok(ChessMove::new(source, destination, promotion))
}

/// Validates every invariant assumed by puzzle loading and subsequent solver
/// navigation. It only mutates a local board, so callers can fail closed before
/// replacing their active batch.
pub fn validate_puzzle(puzzle: &config::Puzzle) -> Result<(), String> {
    let mut board = Board::from_str(&puzzle.fen).map_err(|_| String::from("invalid FEN"))?;
    let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
    if moves.len() < 2 {
        return Err(String::from("moves are insufficient: need at least 2 moves"));
    }

    for token in moves {
        let movement = parse_uci_move(token)?;
        if !board.legal(movement) {
            return Err(format!("illegal move '{token}'"));
        }
        board = board.make_move_new(movement);
    }

    Ok(())
}

pub fn validate_puzzle_batch(puzzles: &[config::Puzzle]) -> Result<(), String> {
    for puzzle in puzzles {
        validate_puzzle(puzzle)
            .map_err(|error| format!("Invalid puzzle data for {}: {error}", puzzle.puzzle_id))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puzzle(id: &str, moves: &str) -> config::Puzzle {
        config::Puzzle {
            puzzle_id: id.into(),
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".into(),
            moves: moves.into(),
            ..config::Puzzle::default()
        }
    }

    #[test]
    fn validate_puzzle_rejects_invalid_fen_without_panicking() {
        let mut invalid = puzzle("bad-fen", "a1a2 h1h2");
        invalid.fen = "not a FEN".into();

        assert_eq!(validate_puzzle(&invalid), Err("invalid FEN".into()));
    }

    #[test]
    fn validate_puzzle_rejects_missing_or_insufficient_moves() {
        assert!(validate_puzzle(&puzzle("empty", "")).unwrap_err().contains("at least 2"));
        assert!(validate_puzzle(&puzzle("short", "a1a2")).unwrap_err().contains("at least 2"));
    }

    #[test]
    fn parse_uci_move_rejects_malformed_tokens_without_panicking() {
        for token in ["e2", "i1i2", "e2e4x", "a1☃2"] {
            assert!(parse_uci_move(token).is_err(), "{token} should be rejected");
        }
    }

    #[test]
    fn validate_puzzle_rejects_illegal_initial_and_later_moves() {
        assert!(validate_puzzle(&puzzle("illegal-first", "a1a3 h1h2"))
            .unwrap_err()
            .contains("illegal move 'a1a3'"));
        assert!(validate_puzzle(&puzzle("illegal-later", "a1a2 h1h2 a2a4"))
            .unwrap_err()
            .contains("illegal move 'a2a4'"));
    }

    #[test]
    fn validate_puzzle_batch_rejects_a_later_invalid_puzzle() {
        let batch = vec![
            puzzle("valid-first", "a1a2 h1h2"),
            puzzle("valid-second", "a1a2 h1h2"),
            puzzle("invalid-third", "a1a2 i1i2"),
        ];

        assert!(validate_puzzle_batch(&batch)
            .unwrap_err()
            .contains("Invalid puzzle data for invalid-third"));
    }

    #[test]
    fn validate_puzzle_accepts_a_complete_legal_batch() {
        let batch = vec![
            puzzle("valid-first", "a1a2 h1h2"),
            puzzle("valid-second", "a1a2 h1h2 a2a3 h2h3"),
        ];

        assert_eq!(validate_puzzle_batch(&batch), Ok(()));
    }

    #[test]
    fn validate_puzzle_rejects_a_promotion_token_without_a_legal_promotion() {
        assert!(validate_puzzle(&puzzle("not-a-promotion", "a1a2q h1h2"))
            .unwrap_err()
            .contains("illegal move 'a1a2q'"));
    }
}

impl Tab for PuzzleTab {
    type Message = Message;

    fn title(&self) -> String {
        lang::tr(&self.lang, "current_puzzle")
    }

    fn tab_label(&self) -> TabLabel {
        TabLabel::Text(self.title())
    }

    fn content(&self) -> Element<'_, Message> {
        let col_puzzle_info = if !self.puzzles.is_empty() && self.current_puzzle < self.puzzles.len() {
            Scrollable::new(col![
                Text::new(lang::tr(&self.lang, "puzzle_link")),
                row![
                    TextInput::new("",
                        &("https://lichess.org/training/".to_owned() + &self.puzzles[self.current_puzzle].puzzle_id),
                    ).on_input(PuzzleMessage::ChangeTextInputs),
                    Button::new(Text::new(lang::tr(&self.lang, "copy"))).on_press(PuzzleMessage::CopyText("https://lichess.org/training/".to_owned() + &self.puzzles[self.current_puzzle].puzzle_id)).style(btn_style_simple),
                    Button::new(Text::new(lang::tr(&self.lang, "open"))).on_press(PuzzleMessage::OpenLink("https://lichess.org/training/".to_owned() + &self.puzzles[self.current_puzzle].puzzle_id)).style(btn_style_simple),

                ],
                Text::new(lang::tr(&self.lang, "fen")),
                row![
                    TextInput::new(
                        &self.current_puzzle_fen,
                        &self.current_puzzle_fen,
                    ).on_input(PuzzleMessage::ChangeTextInputs),
                    Button::new(Text::new(lang::tr(&self.lang, "copy"))).on_press(PuzzleMessage::CopyText(self.current_puzzle_fen.clone())).style(btn_style_simple),
                ],
                Text::new(lang::tr(&self.lang, "rating") + &self.puzzles[self.current_puzzle].rating.to_string()),
                Text::new(lang::tr(&self.lang, "rd") + &self.puzzles[self.current_puzzle].rating_deviation.to_string()),
                Text::new(lang::tr(&self.lang, "popularity") + &self.puzzles[self.current_puzzle].popularity.to_string()),
                Text::new(lang::tr(&self.lang, "times_played") + &self.puzzles[self.current_puzzle].nb_plays.to_string()),
                Text::new(lang::tr(&self.lang, "themes")),
                Text::new(&self.puzzles[self.current_puzzle].themes),
                Text::new(lang::tr(&self.lang, "url")),
                row![
                    TextInput::new(
                        &self.puzzles[self.current_puzzle].game_url,
                        &self.puzzles[self.current_puzzle].game_url,
                    ).on_input(PuzzleMessage::ChangeTextInputs),
                    Button::new(Text::new(lang::tr(&self.lang, "copy"))).on_press(PuzzleMessage::CopyText(self.puzzles[self.current_puzzle].game_url.clone())).style(btn_style_simple),
                    Button::new(Text::new(lang::tr(&self.lang, "open"))).on_press(PuzzleMessage::OpenLink(self.puzzles[self.current_puzzle].game_url.clone())).style(btn_style_simple),
                ],
                Button::new(Text::new(lang::tr(&self.lang, "screenshot"))).on_press(PuzzleMessage::TakeScreenshot).style(btn_style_simple),
                Button::new(Text::new(lang::tr(&self.lang, "export_pdf_btn"))).on_press(PuzzleMessage::ExportToPDF).style(btn_style_simple),
                Button::new(Text::new(lang::tr(&self.lang, "export_pgn"))).padding(5).on_press(PuzzleMessage::ExportToPGN).style(btn_style_simple),
            ].padding([0, 30]).spacing(10).align_x(Alignment::Center))
        } else {
            Scrollable::new(col![
                    Text::new(lang::tr(&self.lang, "no_puzzle"))
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill)
                ].spacing(10))
        };
        let content: Element<PuzzleMessage, Theme, iced::Renderer> = Container::new(col_puzzle_info)
            .align_x(alignment::Horizontal::Center).height(Length::Fill).into();

        content.map(Message::PuzzleInfo)
    }
}
