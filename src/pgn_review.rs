use crate::pgn_import::{ImportedGame, ImportedGameHeaders};
use chess::Board;

/// An immutable in-memory snapshot of one position in a reviewed PGN game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnPositionCandidate {
    pub game_index: usize,
    pub ply_index: usize,
    pub board: Board,
    pub headers: ImportedGameHeaders,
}

/// Navigates validated PGN games without coupling them to the puzzle workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnReviewSession {
    games: Vec<ImportedGame>,
    current_game_index: usize,
    current_ply_index: usize,
}

impl PgnReviewSession {
    /// Creates a session positioned at the first game's initial board.
    pub fn new(games: Vec<ImportedGame>) -> Result<Self, String> {
        if games.is_empty() {
            return Err("PGN review session contains no games".to_string());
        }

        for (index, game) in games.iter().enumerate() {
            let game_number = index + 1;
            if game.positions.is_empty() {
                return Err(format!("PGN review game {game_number} has no positions"));
            }

            let expected_positions = game.moves.len() + 1;
            if game.positions.len() != expected_positions {
                return Err(format!(
                    "PGN review game {game_number} has {} positions for {} moves; expected {expected_positions} positions",
                    game.positions.len(),
                    game.moves.len()
                ));
            }
        }

        Ok(Self {
            games,
            current_game_index: 0,
            current_ply_index: 0,
        })
    }

    pub fn game_count(&self) -> usize {
        self.games.len()
    }

    pub fn current_game_index(&self) -> usize {
        self.current_game_index
    }

    pub fn current_ply_index(&self) -> usize {
        self.current_ply_index
    }

    pub fn current_game(&self) -> &ImportedGame {
        &self.games[self.current_game_index]
    }

    pub fn current_board(&self) -> &Board {
        &self.current_game().positions[self.current_ply_index]
    }

    /// Captures the current game, ply, board, and headers as an independent value.
    pub fn capture_current_position(&self) -> PgnPositionCandidate {
        PgnPositionCandidate {
            game_index: self.current_game_index(),
            ply_index: self.current_ply_index(),
            board: *self.current_board(),
            headers: self.current_game().headers.clone(),
        }
    }

    pub fn next_ply(&mut self) -> bool {
        if self.current_ply_index + 1 < self.current_game().positions.len() {
            self.current_ply_index += 1;
            true
        } else {
            false
        }
    }

    pub fn previous_ply(&mut self) -> bool {
        if self.current_ply_index > 0 {
            self.current_ply_index -= 1;
            true
        } else {
            false
        }
    }

    pub fn next_game(&mut self) -> bool {
        if self.current_game_index + 1 < self.games.len() {
            self.current_game_index += 1;
            self.current_ply_index = 0;
            true
        } else {
            false
        }
    }

    pub fn previous_game(&mut self) -> bool {
        if self.current_game_index > 0 {
            self.current_game_index -= 1;
            self.current_ply_index = 0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pgn_import::{ImportedGame, ImportedGameHeaders, parse_pgn};
    use chess::Board;

    fn parsed_games(pgn: &str) -> Vec<ImportedGame> {
        parse_pgn(pgn).expect("fixture PGN must parse")
    }

    #[test]
    fn creates_a_session_from_parsed_games_at_the_first_game_and_ply() {
        let games = parsed_games("1. e4 e5 1-0");

        let session = PgnReviewSession::new(games).expect("parsed game must be coherent");

        assert_eq!(session.game_count(), 1);
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &Board::default());
        assert_eq!(session.current_game().moves.len(), 2);
    }

    #[test]
    fn advances_and_rewinds_through_parsed_positions() {
        let games = parsed_games("1. e4 e5 2. Nf3 1-0");
        let expected_positions = games[0].positions.clone();
        let mut session = PgnReviewSession::new(games).expect("parsed game must be coherent");

        assert!(session.next_ply());
        assert_eq!(session.current_ply_index(), 1);
        assert_eq!(session.current_board(), &expected_positions[1]);
        assert!(session.next_ply());
        assert!(session.next_ply());
        assert_eq!(session.current_ply_index(), 3);
        assert_eq!(session.current_board(), &expected_positions[3]);

        assert!(session.previous_ply());
        assert_eq!(session.current_ply_index(), 2);
        assert_eq!(session.current_board(), &expected_positions[2]);
    }

    #[test]
    fn ply_navigation_limits_preserve_indices_and_board() {
        let games = parsed_games("1. e4 1-0");
        let mut session = PgnReviewSession::new(games).expect("parsed game must be coherent");

        let initial_board = *session.current_board();
        assert!(!session.previous_ply());
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &initial_board);

        assert!(session.next_ply());
        let final_board = *session.current_board();
        assert!(!session.next_ply());
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 1);
        assert_eq!(session.current_board(), &final_board);
    }

    #[test]
    fn changes_games_in_source_order_and_resets_the_ply() {
        let games =
            parsed_games("[Event \"First\"]\n1. e4 e5 1-0\n\n[Event \"Second\"]\n1. d4 0-1");
        let second_initial_board = games[1].positions[0];
        let mut session = PgnReviewSession::new(games).expect("parsed games must be coherent");

        assert!(session.next_ply());
        assert_eq!(session.current_ply_index(), 1);
        assert!(session.next_game());
        assert_eq!(session.current_game_index(), 1);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &second_initial_board);

        assert!(session.next_ply());
        assert_eq!(session.current_ply_index(), 1);
        assert!(session.previous_game());
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
    }

    #[test]
    fn game_navigation_limits_preserve_indices_and_board() {
        let games = parsed_games("1. e4 1-0\n\n1. d4 0-1");
        let mut session = PgnReviewSession::new(games).expect("parsed games must be coherent");

        let first_board = *session.current_board();
        assert!(!session.previous_game());
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &first_board);

        assert!(session.next_game());
        let last_board = *session.current_board();
        assert!(!session.next_game());
        assert_eq!(session.current_game_index(), 1);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &last_board);
    }

    #[test]
    fn accepts_a_zero_move_game_produced_by_the_parser() {
        let games = parsed_games("[Event \"Quiet\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2");
        let mut session = PgnReviewSession::new(games).expect("result-only game must be coherent");

        assert_eq!(session.current_game().moves.len(), 0);
        assert_eq!(session.current_game().positions.len(), 1);
        assert_eq!(session.current_ply_index(), 0);
        assert!(!session.next_ply());
        assert!(!session.previous_ply());
        assert_eq!(session.current_board(), &Board::default());
    }

    #[test]
    fn captures_an_initial_position_as_an_independent_snapshot() {
        let games = parsed_games("[Event \"Demo\"]\n[White \"Alice\"]\n\n1. e4 e5 1-0");
        let mut session = PgnReviewSession::new(games).expect("parsed game must be coherent");

        let candidate = session.capture_current_position();
        assert_eq!(candidate.game_index, 0);
        assert_eq!(candidate.ply_index, 0);
        assert_eq!(candidate.board, Board::default());
        assert_eq!(candidate.headers.event.as_deref(), Some("Demo"));
        assert_eq!(candidate.headers.white.as_deref(), Some("Alice"));

        assert!(session.next_ply());
        assert_ne!(session.current_board(), &candidate.board);
        assert_eq!(candidate.board, Board::default());
    }

    #[test]
    fn captures_the_current_game_ply_and_board_after_navigation() {
        let games = parsed_games("1. e4 e5 1-0\n\n1. d4 d5 0-1");
        let mut session = PgnReviewSession::new(games).expect("parsed games must be coherent");

        assert!(session.next_game());
        assert!(session.next_ply());
        let current_board = *session.current_board();

        let candidate = session.capture_current_position();
        assert_eq!(candidate.game_index, 1);
        assert_eq!(candidate.ply_index, 1);
        assert_eq!(candidate.board, current_board);
    }

    #[test]
    fn captures_a_setup_fen_position() {
        let fen = "8/8/8/8/8/8/8/K6k w - - 0 1";
        let games = parsed_games(&format!("[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n1. Kb1"));
        let session = PgnReviewSession::new(games).expect("setup game must be coherent");

        let candidate = session.capture_current_position();
        assert_eq!(
            candidate.board,
            fen.parse::<Board>().expect("fixture FEN is valid")
        );
        assert_eq!(candidate.headers.set_up.as_deref(), Some("1"));
        assert_eq!(candidate.headers.fen.as_deref(), Some(fen));
    }

    #[test]
    fn captures_ply_zero_for_a_zero_move_game() {
        let games = parsed_games("[Event \"Quiet\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2");
        let session = PgnReviewSession::new(games).expect("result-only game must be coherent");

        let candidate = session.capture_current_position();
        assert_eq!(candidate.game_index, 0);
        assert_eq!(candidate.ply_index, 0);
        assert_eq!(candidate.board, Board::default());
        assert_eq!(candidate.headers.event.as_deref(), Some("Quiet"));
    }

    #[test]
    fn rejects_an_empty_game_collection() {
        let error = PgnReviewSession::new(Vec::new()).expect_err("empty sessions must fail");

        assert!(error.contains("no games"));
    }

    #[test]
    fn rejects_games_without_positions_with_their_index() {
        let game = ImportedGame {
            headers: ImportedGameHeaders::default(),
            moves: Vec::new(),
            positions: Vec::new(),
        };

        let error = PgnReviewSession::new(vec![game]).expect_err("game without a board must fail");

        assert!(error.contains("game 1"));
        assert!(error.contains("no positions"));
    }

    #[test]
    fn rejects_games_with_misaligned_moves_and_positions_with_their_index() {
        let game = ImportedGame {
            headers: ImportedGameHeaders::default(),
            moves: Vec::new(),
            positions: vec![Board::default(), Board::default()],
        };

        let error = PgnReviewSession::new(vec![
            ImportedGame {
                headers: ImportedGameHeaders::default(),
                moves: Vec::new(),
                positions: vec![Board::default()],
            },
            game,
        ])
        .expect_err("misaligned game must fail");

        assert!(error.contains("game 2"));
        assert!(error.contains("positions"));
        assert!(error.contains("moves"));
    }
}
