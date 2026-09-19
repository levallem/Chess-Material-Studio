use iced::widget::{Button, Column, Container, Scrollable, Text, row};
use iced::{Alignment, Element, Length, Task, Theme, alignment};
use iced_aw::TabLabel;
use rfd::AsyncFileDialog;
use std::path::PathBuf;

use chess::Board;
use chess_material_studio::pgn_import::parse_pgn;
use chess_material_studio::pgn_review::PgnReviewSession;

use crate::styles::btn_style_simple;
use crate::{Message, Tab, config, lang};

#[derive(Debug, Clone)]
pub enum PgnMessage {
    ChooseFile,
    FileSelected(Option<PathBuf>),
    FileRead {
        generation: u64,
        path: PathBuf,
        content: Result<String, String>,
    },
    PreviousGame,
    NextGame,
    PreviousPly,
    NextPly,
}

pub struct PgnTab {
    session: Option<PgnReviewSession>,
    source: Option<PathBuf>,
    status: Option<String>,
    load_generation: u64,
    pub lang: lang::Language,
}

impl PgnTab {
    pub fn new() -> Self {
        Self {
            session: None,
            source: None,
            status: None,
            load_generation: 0,
            lang: config::SETTINGS.lang,
        }
    }

    pub fn current_board(&self) -> Option<&Board> {
        self.session.as_ref().map(PgnReviewSession::current_board)
    }

    pub fn update(&mut self, message: PgnMessage) -> Task<Message> {
        match message {
            PgnMessage::ChooseFile => Task::perform(Self::choose_pgn_file(), |path| {
                Message::Pgn(PgnMessage::FileSelected(path))
            }),
            PgnMessage::FileSelected(Some(path)) => {
                let generation = self.next_load_generation();
                Task::perform(Self::read_pgn_file(path), move |(path, content)| {
                    Message::Pgn(PgnMessage::FileRead {
                        generation,
                        path,
                        content,
                    })
                })
            }
            PgnMessage::FileSelected(None) => Task::none(),
            PgnMessage::FileRead {
                generation,
                path,
                content,
            } => {
                if generation != self.load_generation {
                    return Task::none();
                }
                match content.and_then(|content| self.load_from_text(path, &content)) {
                    Ok(()) => self.status = None,
                    Err(error) => {
                        self.status = Some(format!(
                            "{}: {error}",
                            lang::tr(&self.lang, "pgn_load_error")
                        ));
                    }
                }
                Task::none()
            }
            PgnMessage::PreviousGame => {
                if let Some(session) = self.session.as_mut() {
                    session.previous_game();
                }
                Task::none()
            }
            PgnMessage::NextGame => {
                if let Some(session) = self.session.as_mut() {
                    session.next_game();
                }
                Task::none()
            }
            PgnMessage::PreviousPly => {
                if let Some(session) = self.session.as_mut() {
                    session.previous_ply();
                }
                Task::none()
            }
            PgnMessage::NextPly => {
                if let Some(session) = self.session.as_mut() {
                    session.next_ply();
                }
                Task::none()
            }
        }
    }

    async fn choose_pgn_file() -> Option<PathBuf> {
        AsyncFileDialog::new()
            .add_filter("PGN", &["pgn"])
            .pick_file()
            .await
            .map(|file| file.path().to_path_buf())
    }

    async fn read_pgn_file(path: PathBuf) -> (PathBuf, Result<String, String>) {
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read '{}': {error}", path.display()));
        (path, content)
    }

    fn load_from_text(&mut self, source: PathBuf, content: &str) -> Result<(), String> {
        let session = PgnReviewSession::new(parse_pgn(content)?)?;
        self.session = Some(session);
        self.source = Some(source);
        Ok(())
    }

    fn next_load_generation(&mut self) -> u64 {
        self.load_generation = self.load_generation.wrapping_add(1);
        self.load_generation
    }

    fn header_or_dash(value: Option<&String>) -> String {
        value.cloned().unwrap_or_else(|| "-".to_string())
    }
}

impl Tab for PgnTab {
    type Message = Message;

    fn title(&self) -> String {
        lang::tr(&self.lang, "pgn_review")
    }

    fn tab_label(&self) -> TabLabel {
        TabLabel::Text(self.title())
    }

    fn content(&self) -> Element<'_, Message> {
        let mut content = Column::new().spacing(10).align_x(Alignment::Center).push(
            Button::new(Text::new(lang::tr(&self.lang, "open_pgn")))
                .on_press(PgnMessage::ChooseFile)
                .style(btn_style_simple),
        );

        if let Some(status) = &self.status {
            content = content.push(Text::new(status));
        }

        if let Some(session) = &self.session {
            let game = session.current_game();
            let can_previous_game = session.current_game_index() > 0;
            let can_next_game = session.current_game_index() + 1 < session.game_count();
            let can_previous_ply = session.current_ply_index() > 0;
            let can_next_ply = session.current_ply_index() + 1 < game.positions.len();

            let previous_game = Button::new(Text::new(lang::tr(&self.lang, "pgn_previous_game")))
                .style(btn_style_simple);
            let next_game = Button::new(Text::new(lang::tr(&self.lang, "pgn_next_game")))
                .style(btn_style_simple);
            let previous_ply = Button::new(Text::new(lang::tr(&self.lang, "pgn_previous_ply")))
                .style(btn_style_simple);
            let next_ply = Button::new(Text::new(lang::tr(&self.lang, "pgn_next_ply")))
                .style(btn_style_simple);

            let previous_game = if can_previous_game {
                previous_game.on_press(PgnMessage::PreviousGame)
            } else {
                previous_game
            };
            let next_game = if can_next_game {
                next_game.on_press(PgnMessage::NextGame)
            } else {
                next_game
            };
            let previous_ply = if can_previous_ply {
                previous_ply.on_press(PgnMessage::PreviousPly)
            } else {
                previous_ply
            };
            let next_ply = if can_next_ply {
                next_ply.on_press(PgnMessage::NextPly)
            } else {
                next_ply
            };

            content = content
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "pgn_source"),
                    self.source
                        .as_ref()
                        .map(|source| source.display().to_string())
                        .unwrap_or_else(|| "-".to_string())
                )))
                .push(Text::new(format!(
                    "{}: {}/{}",
                    lang::tr(&self.lang, "pgn_game"),
                    session.current_game_index() + 1,
                    session.game_count()
                )))
                .push(Text::new(format!(
                    "{}: {}/{}",
                    lang::tr(&self.lang, "pgn_ply"),
                    session.current_ply_index(),
                    game.moves.len()
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "white"),
                    Self::header_or_dash(game.headers.white.as_ref())
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "black"),
                    Self::header_or_dash(game.headers.black.as_ref())
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "event"),
                    Self::header_or_dash(game.headers.event.as_ref())
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "date"),
                    Self::header_or_dash(game.headers.date.as_ref())
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "result"),
                    Self::header_or_dash(game.headers.result.as_ref())
                )))
                .push(Text::new(format!(
                    "{}: {}",
                    lang::tr(&self.lang, "fen"),
                    session.current_board()
                )))
                .push(
                    row![previous_game, next_game]
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .push(
                    row![previous_ply, next_ply]
                        .spacing(10)
                        .align_y(Alignment::Center),
                );
        } else {
            content = content.push(Text::new(lang::tr(&self.lang, "no_pgn_loaded")));
        }

        let content: Element<PgnMessage, Theme, iced::Renderer> = Container::new(
            Scrollable::new(content)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .align_x(alignment::Horizontal::Center)
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

        content.map(Message::Pgn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_game() -> &'static str {
        "[Event \"First\"]\n[Date \"2026.09.19\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0"
    }

    #[test]
    fn loads_a_valid_pgn_at_the_first_game_and_ply() {
        let mut tab = PgnTab::new();

        tab.load_from_text(PathBuf::from("first.pgn"), first_game())
            .expect("valid PGN must load");

        let session = tab.session.as_ref().expect("session must be retained");
        assert_eq!(session.game_count(), 1);
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(tab.source, Some(PathBuf::from("first.pgn")));
        assert_eq!(tab.current_board(), Some(&chess::Board::default()));
    }

    #[test]
    fn current_board_is_none_without_a_loaded_session() {
        assert_eq!(PgnTab::new().current_board(), None);
    }

    #[test]
    fn loads_multiple_games_and_resets_the_ply_when_changing_games() {
        let mut tab = PgnTab::new();
        tab.load_from_text(PathBuf::from("matches.pgn"), "1. e4 e5 1-0\n\n1. d4 d5 0-1")
            .expect("multiple games must load");

        let _ = tab.update(PgnMessage::NextPly);
        let _ = tab.update(PgnMessage::NextGame);

        let session = tab.session.as_ref().expect("session must be retained");
        assert_eq!(session.game_count(), 2);
        assert_eq!(session.current_game_index(), 1);
        assert_eq!(session.current_ply_index(), 0);
    }

    #[test]
    fn ply_navigation_updates_the_current_board() {
        let mut tab = PgnTab::new();
        tab.load_from_text(PathBuf::from("first.pgn"), first_game())
            .expect("valid PGN must load");

        let initial_board = *tab
            .session
            .as_ref()
            .expect("session must be retained")
            .current_board();
        let _ = tab.update(PgnMessage::NextPly);
        let after_first_ply = *tab
            .session
            .as_ref()
            .expect("session must be retained")
            .current_board();
        let _ = tab.update(PgnMessage::PreviousPly);

        assert_ne!(after_first_ply, initial_board);
        assert_eq!(
            tab.session
                .as_ref()
                .expect("session must be retained")
                .current_board(),
            &initial_board
        );
    }

    #[test]
    fn accepts_a_zero_move_game() {
        let mut tab = PgnTab::new();
        tab.load_from_text(
            PathBuf::from("quiet.pgn"),
            "[Event \"Quiet\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2",
        )
        .expect("zero-move game must load");

        let session = tab.session.as_ref().expect("session must be retained");
        assert_eq!(session.current_game().moves.len(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert!(!session.current_game().positions.is_empty());
    }

    #[test]
    fn invalid_load_preserves_the_existing_session_and_position() {
        let mut tab = PgnTab::new();
        tab.load_from_text(PathBuf::from("first.pgn"), first_game())
            .expect("valid PGN must load");
        let _ = tab.update(PgnMessage::NextPly);
        let previous_session = tab.session.clone();
        let previous_source = tab.source.clone();

        let _ = tab.update(PgnMessage::FileRead {
            generation: tab.load_generation,
            path: PathBuf::from("invalid.pgn"),
            content: Ok("1. not-a-move 1-0".to_string()),
        });

        assert_eq!(tab.session, previous_session);
        assert_eq!(tab.source, previous_source);
        assert!(
            tab.status
                .as_deref()
                .is_some_and(|status| status.contains("PGN"))
        );
    }

    #[test]
    fn cancellation_preserves_the_existing_session() {
        let mut tab = PgnTab::new();
        tab.load_from_text(PathBuf::from("first.pgn"), first_game())
            .expect("valid PGN must load");
        let previous_session = tab.session.clone();
        let previous_source = tab.source.clone();

        let _ = tab.update(PgnMessage::FileSelected(None));

        assert_eq!(tab.session, previous_session);
        assert_eq!(tab.source, previous_source);
    }

    #[test]
    fn a_later_valid_load_replaces_the_session_at_the_initial_position() {
        let mut tab = PgnTab::new();
        tab.load_from_text(PathBuf::from("first.pgn"), first_game())
            .expect("first PGN must load");
        let _ = tab.update(PgnMessage::NextPly);

        tab.load_from_text(
            PathBuf::from("second.pgn"),
            "[White \"Carol\"]\n[Black \"Dave\"]\n\n1. d4 0-1",
        )
        .expect("replacement PGN must load");

        let session = tab.session.as_ref().expect("session must be retained");
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(
            session.current_game().headers.white.as_deref(),
            Some("Carol")
        );
        assert_eq!(tab.source, Some(PathBuf::from("second.pgn")));
    }

    #[test]
    fn stale_valid_file_result_cannot_replace_a_newer_loaded_session() {
        let mut tab = PgnTab::new();
        let _ = tab.update(PgnMessage::FileSelected(Some(PathBuf::from("first.pgn"))));
        let first_generation = tab.load_generation;
        let _ = tab.update(PgnMessage::FileSelected(Some(PathBuf::from("second.pgn"))));
        let second_generation = tab.load_generation;

        let _ = tab.update(PgnMessage::FileRead {
            generation: second_generation,
            path: PathBuf::from("second.pgn"),
            content: Ok("[White \"Carol\"]\n\n1. d4 0-1".to_string()),
        });
        let session_after_newer_load = tab.session.clone();
        let source_after_newer_load = tab.source.clone();

        let _ = tab.update(PgnMessage::FileRead {
            generation: first_generation,
            path: PathBuf::from("first.pgn"),
            content: Ok("[White \"Alice\"]\n\n1. e4 1-0".to_string()),
        });

        assert_eq!(tab.session, session_after_newer_load);
        assert_eq!(tab.source, source_after_newer_load);
        assert_eq!(tab.source, Some(PathBuf::from("second.pgn")));
    }

    #[test]
    fn stale_file_error_cannot_replace_a_newer_success_status() {
        let mut tab = PgnTab::new();
        let _ = tab.update(PgnMessage::FileSelected(Some(PathBuf::from("first.pgn"))));
        let first_generation = tab.load_generation;
        let _ = tab.update(PgnMessage::FileSelected(Some(PathBuf::from("second.pgn"))));
        let second_generation = tab.load_generation;

        let _ = tab.update(PgnMessage::FileRead {
            generation: second_generation,
            path: PathBuf::from("second.pgn"),
            content: Ok("[White \"Carol\"]\n\n1. d4 0-1".to_string()),
        });
        let session_after_newer_load = tab.session.clone();
        let source_after_newer_load = tab.source.clone();
        assert_eq!(tab.status, None);

        let _ = tab.update(PgnMessage::FileRead {
            generation: first_generation,
            path: PathBuf::from("first.pgn"),
            content: Err("stale read failure".to_string()),
        });

        assert_eq!(tab.session, session_after_newer_load);
        assert_eq!(tab.source, source_after_newer_load);
        assert_eq!(tab.status, None);
    }

    #[test]
    fn pgn_translation_keys_exist_in_every_language() {
        let keys = [
            "pgn_review",
            "open_pgn",
            "pgn_source",
            "pgn_game",
            "pgn_ply",
            "event",
            "date",
            "result",
            "no_pgn_loaded",
            "pgn_load_error",
            "pgn_previous_game",
            "pgn_next_game",
            "pgn_previous_ply",
            "pgn_next_ply",
        ];

        for language in lang::Language::ALL {
            for key in keys {
                assert!(
                    !lang::tr(&language, key).is_empty(),
                    "missing or empty translation for {key}"
                );
            }
        }
    }
}
