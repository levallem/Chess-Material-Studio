use crate::pgn_import::{ImportedGame, ImportedGameHeaders};
use chess::{Board, ChessMove};
use std::str::FromStr;

/// An immutable in-memory snapshot of one position in a reviewed PGN game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnPositionCandidate {
    pub game_index: usize,
    pub ply_index: usize,
    pub board: Board,
    pub headers: ImportedGameHeaders,
}

/// A self-contained, reproducible PGN position prepared for future persistence.
///
/// The FEN values are canonical `Board` representations and `main_line_uci`
/// contains one UCI move per main-line ply. The snapshot intentionally keeps
/// no UI, project, or persistence state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnPositionSnapshot {
    pub source_game_index: usize,
    pub ply_index: usize,
    pub selected_fen: String,
    pub initial_fen: String,
    pub main_line_uci: Vec<String>,
    pub headers: ImportedGameHeaders,
}

impl PgnPositionSnapshot {
    fn reconstruct_game(&self) -> Result<ImportedGame, String> {
        let mut board = Board::from_str(&self.initial_fen)
            .map_err(|error| format!("snapshot initial FEN is invalid: {error:?}"))?;
        if self.ply_index > self.main_line_uci.len() {
            return Err(format!(
                "snapshot ply index {} is outside the main line of {} moves",
                self.ply_index,
                self.main_line_uci.len()
            ));
        }

        let mut moves = Vec::with_capacity(self.main_line_uci.len());
        let mut positions = Vec::with_capacity(self.main_line_uci.len() + 1);
        positions.push(board);
        for (index, uci) in self.main_line_uci.iter().enumerate() {
            let chess_move = ChessMove::from_str(uci).map_err(|_| {
                format!(
                    "snapshot main line has invalid UCI at ply {}: {uci}",
                    index + 1
                )
            })?;
            if !board.legal(chess_move) {
                return Err(format!(
                    "snapshot main line has illegal move at ply {}: {uci}",
                    index + 1
                ));
            }
            board = board.make_move_new(chess_move);
            moves.push(chess_move);
            positions.push(board);
        }

        let selected_fen_board = Board::from_str(&self.selected_fen)
            .map_err(|error| format!("snapshot selected FEN is invalid: {error:?}"))?;
        if positions[self.ply_index] != selected_fen_board {
            return Err("snapshot selected FEN does not match the reconstructed board".into());
        }

        Ok(ImportedGame {
            headers: self.headers.clone(),
            moves,
            positions,
        })
    }

    /// Replays the complete main line and returns the board at `ply_index`.
    pub fn reconstruct_selected_board(&self) -> Result<Board, String> {
        let game = self.reconstruct_game()?;
        Ok(game.positions[self.ply_index])
    }

    pub fn validate(&self) -> Result<(), String> {
        self.reconstruct_game().map(|_| ())
    }

    pub fn previous_uci(&self) -> Option<&str> {
        self.ply_index
            .checked_sub(1)
            .and_then(|index| self.main_line_uci.get(index))
            .map(String::as_str)
    }

    pub fn next_uci(&self) -> Option<&str> {
        self.main_line_uci.get(self.ply_index).map(String::as_str)
    }
}

/// Navigates validated PGN games without coupling them to the puzzle workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnReviewSession {
    games: Vec<ImportedGame>,
    source_game_indices: Vec<usize>,
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
            source_game_indices: (0..games.len()).collect(),
            games,
            current_game_index: 0,
            current_ply_index: 0,
        })
    }

    /// Reconstructs a one-game session positioned at the snapshot's selected ply.
    pub fn from_snapshot(snapshot: &PgnPositionSnapshot) -> Result<Self, String> {
        let game = snapshot.reconstruct_game()?;
        let mut session = Self::new(vec![game])?;
        session.source_game_indices = vec![snapshot.source_game_index];
        session.current_ply_index = snapshot.ply_index;
        Ok(session)
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

    /// Captures the current position with enough validated game context to
    /// reproduce it without the original PGN text.
    pub fn capture_current_snapshot(&self) -> PgnPositionSnapshot {
        let game = self.current_game();
        PgnPositionSnapshot {
            source_game_index: self.source_game_indices[self.current_game_index],
            ply_index: self.current_ply_index(),
            selected_fen: self.current_board().to_string(),
            initial_fen: game.positions[0].to_string(),
            main_line_uci: game.moves.iter().map(ToString::to_string).collect(),
            headers: game.headers.clone(),
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

    #[test]
    fn captures_a_standard_snapshot_that_replays_to_the_selected_middle_ply() {
        let games = parsed_games(
            "[Event \"Demo\"]\n[Site \"Madrid\"]\n[Date \"2026.09.22\"]\n[Round \"3\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0",
        );
        let mut session = PgnReviewSession::new(games).expect("parsed game must be coherent");
        let initial_snapshot = session.capture_current_snapshot();
        assert_eq!(initial_snapshot.ply_index, 0);
        assert_eq!(initial_snapshot.previous_uci(), None);
        assert_eq!(initial_snapshot.next_uci(), Some("e2e4"));
        assert_eq!(
            initial_snapshot.reconstruct_selected_board().unwrap(),
            Board::default()
        );
        assert!(session.next_ply());
        assert!(session.next_ply());

        let snapshot = session.capture_current_snapshot();

        assert_eq!(snapshot.source_game_index, 0);
        assert_eq!(snapshot.ply_index, 2);
        assert_eq!(snapshot.initial_fen, Board::default().to_string());
        assert_eq!(snapshot.main_line_uci, ["e2e4", "e7e5", "g1f3"]);
        assert_eq!(snapshot.headers, session.current_game().headers);
        assert_eq!(snapshot.previous_uci(), Some("e7e5"));
        assert_eq!(snapshot.next_uci(), Some("g1f3"));
        assert_eq!(
            snapshot
                .reconstruct_selected_board()
                .expect("snapshot must replay"),
            *session.current_board()
        );
        snapshot
            .validate()
            .expect("captured snapshot must validate");
    }

    #[test]
    fn captures_a_setup_fen_snapshot_at_the_final_ply() {
        let initial_fen = "8/8/8/8/8/8/8/K6k w - - 0 1";
        let games = parsed_games(&format!(
            "[SetUp \"1\"]\n[FEN \"{initial_fen}\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n\n1. Kb1"
        ));
        let mut session = PgnReviewSession::new(games).expect("setup game must be coherent");
        assert!(session.next_ply());

        let snapshot = session.capture_current_snapshot();

        assert_eq!(snapshot.ply_index, 1);
        assert_eq!(snapshot.initial_fen, initial_fen);
        assert_eq!(snapshot.main_line_uci, ["a1b1"]);
        assert_eq!(snapshot.headers.set_up.as_deref(), Some("1"));
        assert_eq!(snapshot.headers.fen.as_deref(), Some(initial_fen));
        assert_eq!(snapshot.previous_uci(), Some("a1b1"));
        assert_eq!(snapshot.next_uci(), None);
        assert_eq!(
            snapshot.reconstruct_selected_board().unwrap(),
            *session.current_board()
        );
    }

    #[test]
    fn captures_a_zero_move_snapshot_at_ply_zero() {
        let games = parsed_games("[Event \"Quiet\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2");
        let session = PgnReviewSession::new(games).expect("result-only game must be coherent");

        let snapshot = session.capture_current_snapshot();

        assert_eq!(snapshot.ply_index, 0);
        assert!(snapshot.main_line_uci.is_empty());
        assert_eq!(snapshot.previous_uci(), None);
        assert_eq!(snapshot.next_uci(), None);
        assert_eq!(
            snapshot.reconstruct_selected_board().unwrap(),
            Board::default()
        );
        snapshot.validate().unwrap();
    }

    fn snapshot_with(
        initial_fen: &str,
        main_line_uci: &[&str],
        ply_index: usize,
        selected_fen: String,
    ) -> PgnPositionSnapshot {
        PgnPositionSnapshot {
            source_game_index: 0,
            ply_index,
            selected_fen,
            initial_fen: initial_fen.into(),
            main_line_uci: main_line_uci.iter().map(|uci| (*uci).into()).collect(),
            headers: ImportedGameHeaders::default(),
        }
    }

    #[test]
    fn rejects_snapshots_with_an_invalid_initial_fen() {
        let snapshot = snapshot_with("not a FEN", &[], 0, Board::default().to_string());

        assert!(snapshot.validate().unwrap_err().contains("initial FEN"));
    }

    #[test]
    fn rejects_snapshots_with_an_invalid_selected_fen() {
        let snapshot = snapshot_with(&Board::default().to_string(), &[], 0, "not a FEN".into());

        assert!(
            snapshot
                .validate()
                .unwrap_err()
                .contains("selected FEN is invalid")
        );
    }

    #[test]
    fn accepts_a_textually_distinct_selected_fen_for_the_same_board() {
        let canonical_fen = Board::default().to_string();
        let equivalent_fen = format!("{canonical_fen} ");
        assert_ne!(equivalent_fen, canonical_fen);
        assert_eq!(
            equivalent_fen
                .parse::<Board>()
                .expect("fixture FEN is valid"),
            Board::default()
        );

        let snapshot = snapshot_with(&canonical_fen, &[], 0, equivalent_fen);

        snapshot
            .validate()
            .expect("equivalent selected FEN must validate");
    }

    #[test]
    fn rejects_snapshots_with_an_invalid_uci_or_illegal_main_line_move() {
        let invalid_uci = snapshot_with(
            &Board::default().to_string(),
            &["not-uci"],
            0,
            Board::default().to_string(),
        );
        assert!(invalid_uci.validate().unwrap_err().contains("UCI"));

        let after_e4 = parsed_games("1. e4 1-0")[0].positions[1].to_string();
        let illegal_move = snapshot_with(
            &Board::default().to_string(),
            &["e2e4", "e2e5"],
            1,
            after_e4,
        );
        assert!(illegal_move.validate().unwrap_err().contains("illegal"));
    }

    #[test]
    fn rejects_snapshots_with_an_out_of_range_ply_or_mismatched_selected_fen() {
        let out_of_range = snapshot_with(
            &Board::default().to_string(),
            &["e2e4"],
            2,
            Board::default().to_string(),
        );
        assert!(out_of_range.validate().unwrap_err().contains("ply"));

        let mismatched_fen = snapshot_with(
            &Board::default().to_string(),
            &["e2e4"],
            1,
            Board::default().to_string(),
        );
        assert!(
            mismatched_fen
                .validate()
                .unwrap_err()
                .contains("selected FEN")
        );
    }

    #[test]
    fn reconstructs_a_standard_snapshot_into_a_navigable_single_game_session() {
        let games = parsed_games(
            "[Event \"Demo\"]\n[Site \"Madrid\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n\n1. e4 e5 2. Nf3 1-0",
        );
        let expected_game = games[0].clone();
        let mut source_session =
            PgnReviewSession::new(games).expect("parsed game must be coherent");
        assert!(source_session.next_ply());
        assert!(source_session.next_ply());
        let snapshot = source_session.capture_current_snapshot();

        let mut session = PgnReviewSession::from_snapshot(&snapshot)
            .expect("valid snapshot must reconstruct a session");

        assert_eq!(session.game_count(), 1);
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.current_ply_index(), snapshot.ply_index);
        assert_eq!(session.current_game().headers, snapshot.headers);
        assert_eq!(
            session.current_game().moves.len(),
            snapshot.main_line_uci.len()
        );
        assert_eq!(
            session.current_game().positions.len(),
            session.current_game().moves.len() + 1
        );
        assert_eq!(
            session.current_board(),
            &expected_game.positions[snapshot.ply_index]
        );

        assert!(session.previous_ply());
        assert!(session.previous_ply());
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_board(), &expected_game.positions[0]);
        assert!(!session.previous_ply());

        for expected_ply in 1..expected_game.positions.len() {
            assert!(session.next_ply());
            assert_eq!(session.current_ply_index(), expected_ply);
            assert_eq!(
                session.current_board(),
                &expected_game.positions[expected_ply]
            );
        }
        assert!(!session.next_ply());
    }

    #[test]
    fn reconstructed_session_preserves_source_provenance_but_keeps_local_indices() {
        let games = parsed_games("1. e4 1-0\n\n1. d4 d5 0-1");
        let mut source_session =
            PgnReviewSession::new(games).expect("parsed games must be coherent");
        assert!(source_session.next_game());
        assert!(source_session.next_ply());
        let snapshot = source_session.capture_current_snapshot();
        assert_eq!(snapshot.source_game_index, 1);

        let mut session = PgnReviewSession::from_snapshot(&snapshot)
            .expect("valid snapshot must reconstruct a session");

        assert_eq!(session.game_count(), 1);
        assert_eq!(session.current_game_index(), 0);
        assert_eq!(session.capture_current_position().game_index, 0);
        assert_eq!(
            session.capture_current_snapshot().source_game_index,
            snapshot.source_game_index
        );

        assert!(session.previous_ply());
        assert_eq!(
            session.capture_current_snapshot().source_game_index,
            snapshot.source_game_index
        );
    }

    #[test]
    fn normal_sessions_capture_their_local_game_index_as_source_provenance() {
        let games = parsed_games("1. e4 1-0\n\n1. d4 0-1");
        let mut session = PgnReviewSession::new(games).expect("parsed games must be coherent");

        assert!(session.next_game());
        assert_eq!(session.capture_current_snapshot().source_game_index, 1);
    }

    #[test]
    fn reconstructs_a_setup_fen_snapshot_and_navigates_from_its_selected_ply() {
        let initial_fen = "8/8/8/8/8/8/8/K6k w - - 0 1";
        let games = parsed_games(&format!("[SetUp \"1\"]\n[FEN \"{initial_fen}\"]\n\n1. Kb1"));
        let expected_game = games[0].clone();
        let mut source_session =
            PgnReviewSession::new(games).expect("parsed game must be coherent");
        assert!(source_session.next_ply());
        let snapshot = source_session.capture_current_snapshot();

        let mut session = PgnReviewSession::from_snapshot(&snapshot)
            .expect("valid setup snapshot must reconstruct a session");

        assert_eq!(session.current_game().headers, snapshot.headers);
        assert_eq!(session.current_board(), &expected_game.positions[1]);
        assert!(session.previous_ply());
        assert_eq!(session.current_board(), &expected_game.positions[0]);
        assert!(session.next_ply());
        assert_eq!(session.current_board(), &expected_game.positions[1]);
    }

    #[test]
    fn reconstructs_a_zero_move_snapshot_at_its_only_position() {
        let games = parsed_games("[Event \"Quiet\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2");
        let source_session = PgnReviewSession::new(games).expect("parsed game must be coherent");
        let snapshot = source_session.capture_current_snapshot();

        let mut session = PgnReviewSession::from_snapshot(&snapshot)
            .expect("valid zero-move snapshot must reconstruct a session");

        assert_eq!(session.game_count(), 1);
        assert_eq!(session.current_ply_index(), 0);
        assert_eq!(session.current_game().moves.len(), 0);
        assert_eq!(session.current_game().positions.len(), 1);
        assert!(!session.next_ply());
        assert!(!session.previous_ply());
    }

    #[test]
    fn rejects_invalid_snapshots_when_reconstructing_a_review_session() {
        let snapshot = snapshot_with(
            &Board::default().to_string(),
            &["not-uci"],
            0,
            Board::default().to_string(),
        );

        assert!(PgnReviewSession::from_snapshot(&snapshot).is_err());
    }
}
