use chess::{Board, ChessMove};
use std::str::FromStr;

/// The standard PGN tag-pairs retained by the first import layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedGameHeaders {
    pub event: Option<String>,
    pub site: Option<String>,
    pub date: Option<String>,
    pub round: Option<String>,
    pub white: Option<String>,
    pub black: Option<String>,
    pub result: Option<String>,
    pub set_up: Option<String>,
    pub fen: Option<String>,
}

/// A validated PGN main line independent from the puzzle model.
///
/// `positions[0]` is the initial position and every entry after it is the
/// resulting board after the move at the preceding index in `moves`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedGame {
    pub headers: ImportedGameHeaders,
    pub moves: Vec<ChessMove>,
    pub positions: Vec<Board>,
}

enum PgnEvent {
    Header(String, String),
    Token(String),
}

#[derive(Clone, Copy)]
enum SanSuffix {
    None,
    Check,
    Checkmate,
}

#[derive(Default)]
struct GameBuilder {
    headers: ImportedGameHeaders,
    moves: Vec<ChessMove>,
    positions: Vec<Board>,
    has_content: bool,
    saw_result: bool,
}

/// Parses one or more PGN games and replays only their validated main lines.
pub fn parse_pgn(input: &str) -> Result<Vec<ImportedGame>, String> {
    if input.trim().is_empty() {
        return Err("PGN input is empty or contains no games".to_string());
    }

    let events = scan_pgn(input)?;
    let mut games = Vec::new();
    let mut current: Option<GameBuilder> = None;

    for event in events {
        match event {
            PgnEvent::Header(name, value) => {
                if let Some(builder) = current.take() {
                    if builder.has_moves() || builder.saw_result {
                        finish_game(builder, games.len() + 1, &mut games)?;
                    } else {
                        current = Some(builder);
                    }
                }

                let builder = current.get_or_insert_with(GameBuilder::default);
                builder.has_content = true;
                set_header(&mut builder.headers, &name, value);
            }
            PgnEvent::Token(token) => {
                let token = strip_move_number(&token);
                if token.is_empty() || is_nag(token) {
                    continue;
                }

                if is_result(token) {
                    if let Some(builder) = current.as_mut() {
                        if builder.has_content {
                            builder.saw_result = true;
                        }
                    }
                    continue;
                }

                if current.as_ref().is_some_and(|builder| builder.saw_result) {
                    if let Some(completed_game) = current.take() {
                        finish_game(completed_game, games.len() + 1, &mut games)?;
                    }
                }

                let builder = current.get_or_insert_with(GameBuilder::default);
                builder.has_content = true;
                ensure_initial_position(builder, games.len() + 1)?;
                let board = *builder
                    .positions
                    .last()
                    .ok_or_else(|| "internal PGN position initialization failure".to_string())?;
                let chess_move =
                    parse_san_move(&board, token, games.len() + 1, builder.moves.len() + 1)?;
                if !board.legal(chess_move) {
                    return Err(format!(
                        "invalid SAN move '{token}' in game {} at ply {}: move is illegal",
                        games.len() + 1,
                        builder.moves.len() + 1
                    ));
                }

                builder.moves.push(chess_move);
                builder.positions.push(board.make_move_new(chess_move));
            }
        }
    }

    if let Some(builder) = current {
        finish_game(builder, games.len() + 1, &mut games)?;
    }

    if games.is_empty() {
        Err("PGN input contains no usable games".to_string())
    } else {
        Ok(games)
    }
}

impl GameBuilder {
    fn has_moves(&self) -> bool {
        !self.moves.is_empty()
    }

    fn is_usable(&self) -> bool {
        self.has_moves() || self.saw_result
    }
}

fn finish_game(
    mut builder: GameBuilder,
    game_number: usize,
    games: &mut Vec<ImportedGame>,
) -> Result<(), String> {
    if !builder.is_usable() {
        return Ok(());
    }

    ensure_initial_position(&mut builder, game_number)?;
    games.push(ImportedGame {
        headers: builder.headers,
        moves: builder.moves,
        positions: builder.positions,
    });
    Ok(())
}

fn ensure_initial_position(builder: &mut GameBuilder, game_number: usize) -> Result<(), String> {
    if !builder.positions.is_empty() {
        return Ok(());
    }

    let board = match (&builder.headers.set_up, &builder.headers.fen) {
        (Some(set_up), Some(fen)) if set_up == "1" => Board::from_str(fen)
            .map_err(|error| format!("invalid initial FEN in game {game_number}: {error:?}"))?,
        (Some(set_up), None) if set_up == "1" => {
            return Err(format!(
                "PGN structure error in game {game_number}: SetUp \"1\" requires a FEN tag"
            ));
        }
        (_, Some(_)) => {
            return Err(format!(
                "PGN structure error in game {game_number}: FEN requires SetUp \"1\""
            ));
        }
        _ => Board::default(),
    };

    builder.positions.push(board);
    Ok(())
}

fn set_header(headers: &mut ImportedGameHeaders, name: &str, value: String) {
    match name {
        "Event" => headers.event = Some(value),
        "Site" => headers.site = Some(value),
        "Date" => headers.date = Some(value),
        "Round" => headers.round = Some(value),
        "White" => headers.white = Some(value),
        "Black" => headers.black = Some(value),
        "Result" => headers.result = Some(value),
        "SetUp" => headers.set_up = Some(value),
        "FEN" => headers.fen = Some(value),
        _ => {}
    }
}

fn parse_san_move(
    board: &Board,
    token: &str,
    game_number: usize,
    ply_number: usize,
) -> Result<ChessMove, String> {
    let (normalized, suffix) = normalize_san(token);
    if normalized.is_empty() {
        return Err(format!(
            "invalid SAN move '{token}' in game {game_number} at ply {ply_number}"
        ));
    }

    let chess_move = ChessMove::from_san(board, &normalized).map_err(|_| {
        format!("invalid SAN move '{token}' in game {game_number} at ply {ply_number}")
    })?;
    let resulting_board = board.make_move_new(chess_move);

    match suffix {
        SanSuffix::None => {}
        SanSuffix::Check if resulting_board.checkers().popcnt() != 0 => {}
        SanSuffix::Check => {
            return Err(format!(
                "invalid SAN check suffix '+' in game {game_number} at ply {ply_number}"
            ));
        }
        SanSuffix::Checkmate if resulting_board.status() == chess::BoardStatus::Checkmate => {}
        SanSuffix::Checkmate => {
            return Err(format!(
                "invalid SAN mate suffix '#' in game {game_number} at ply {ply_number}"
            ));
        }
    }

    Ok(chess_move)
}

fn normalize_san(token: &str) -> (String, SanSuffix) {
    let mut san = token.trim();
    while matches!(san.chars().last(), Some('!' | '?')) {
        san = &san[..san.len() - 1];
    }

    let suffix = match san.chars().last() {
        Some('+') => {
            san = &san[..san.len() - 1];
            SanSuffix::Check
        }
        Some('#') => {
            san = &san[..san.len() - 1];
            SanSuffix::Checkmate
        }
        _ => SanSuffix::None,
    };

    let san = match san {
        "0-0" => "O-O",
        "0-0-0" => "O-O-O",
        _ => san,
    };
    (normalize_promotion(san), suffix)
}

fn normalize_promotion(san: &str) -> String {
    let Some((prefix, promotion)) = san.rsplit_once('=') else {
        return san.to_string();
    };

    if prefix.contains('=')
        || !matches!(promotion, "Q" | "R" | "B" | "N")
        || !is_promotion_prefix(prefix)
    {
        return san.to_string();
    }

    format!("{prefix}{promotion}")
}

fn is_promotion_prefix(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    match bytes {
        [file, rank] => is_file(*file) && is_promotion_rank(*rank),
        [source_file, b'x', destination_file, rank] => {
            is_file(*source_file) && is_file(*destination_file) && is_promotion_rank(*rank)
        }
        _ => false,
    }
}

fn is_file(byte: u8) -> bool {
    (b'a'..=b'h').contains(&byte)
}

fn is_promotion_rank(byte: u8) -> bool {
    matches!(byte, b'1' | b'8')
}

fn is_nag(token: &str) -> bool {
    token.strip_prefix('$').is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn is_result(token: &str) -> bool {
    matches!(token, "1-0" | "0-1" | "1/2-1/2" | "*")
}

fn strip_move_number(token: &str) -> &str {
    let bytes = token.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }

    if index == 0 {
        return if token.bytes().all(|byte| byte == b'.') {
            ""
        } else {
            token
        };
    }

    let dot_start = index;
    while index < bytes.len() && bytes[index] == b'.' {
        index += 1;
    }
    if index == dot_start {
        token
    } else {
        &token[index..]
    }
}

fn scan_pgn(input: &str) -> Result<Vec<PgnEvent>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut events = Vec::new();
    let mut index = 0;
    let mut line_start = true;

    while index < chars.len() {
        let current = chars[index];
        if current.is_whitespace() {
            if current == '\n' {
                line_start = true;
            }
            index += 1;
            continue;
        }

        if line_start && current == '[' {
            let (name, value, next_index) = parse_header(&chars, index)?;
            events.push(PgnEvent::Header(name, value));
            index = next_index;
            line_start = false;
            continue;
        }

        line_start = false;
        match current {
            '{' => skip_brace_comment(&chars, &mut index)?,
            ';' => skip_line_comment(&chars, &mut index),
            '(' => skip_variation(&chars, &mut index)?,
            ')' | '}' => {
                return Err(format!("PGN structure error: unexpected '{current}'"));
            }
            _ => {
                let start = index;
                while index < chars.len()
                    && !chars[index].is_whitespace()
                    && !matches!(chars[index], '{' | ';' | '(' | ')')
                {
                    index += 1;
                }
                events.push(PgnEvent::Token(chars[start..index].iter().collect()));
            }
        }
    }

    Ok(events)
}

fn parse_header(chars: &[char], mut index: usize) -> Result<(String, String, usize), String> {
    index += 1;
    skip_whitespace(chars, &mut index);

    let name_start = index;
    while index < chars.len()
        && (chars[index].is_ascii_alphanumeric() || chars[index] == '_' || chars[index] == '-')
    {
        index += 1;
    }
    if name_start == index {
        return Err("PGN structure error: header name is missing".to_string());
    }
    let name: String = chars[name_start..index].iter().collect();

    if index >= chars.len() || !chars[index].is_whitespace() {
        return Err(format!("PGN structure error: header '{name}' has no value"));
    }
    skip_whitespace(chars, &mut index);
    if chars.get(index) != Some(&'"') {
        return Err(format!(
            "PGN structure error: header '{name}' value must be quoted"
        ));
    }
    index += 1;

    let mut value = String::new();
    while let Some(character) = chars.get(index).copied() {
        index += 1;
        match character {
            '"' => break,
            '\\' => {
                let escaped = chars.get(index).copied().ok_or_else(|| {
                    format!("PGN structure error: incomplete escape in header '{name}'")
                })?;
                value.push(escaped);
                index += 1;
            }
            _ => value.push(character),
        }
    }

    if index == chars.len() && chars.get(index - 1) != Some(&'"') {
        return Err(format!("PGN structure error: unclosed header '{name}'"));
    }

    skip_whitespace(chars, &mut index);
    if chars.get(index) != Some(&']') {
        return Err(format!(
            "PGN structure error: header '{name}' is not closed"
        ));
    }

    Ok((name, value, index + 1))
}

fn skip_whitespace(chars: &[char], index: &mut usize) {
    while *index < chars.len() && chars[*index].is_whitespace() {
        *index += 1;
    }
}

fn skip_brace_comment(chars: &[char], index: &mut usize) -> Result<(), String> {
    *index += 1;
    while *index < chars.len() {
        if chars[*index] == '}' {
            *index += 1;
            return Ok(());
        }
        *index += 1;
    }
    Err("PGN structure error: unclosed brace comment".to_string())
}

fn skip_line_comment(chars: &[char], index: &mut usize) {
    while *index < chars.len() && chars[*index] != '\n' {
        *index += 1;
    }
}

fn skip_variation(chars: &[char], index: &mut usize) -> Result<(), String> {
    let mut depth = 0;
    while *index < chars.len() {
        match chars[*index] {
            '(' => {
                depth += 1;
                *index += 1;
            }
            ')' => {
                depth -= 1;
                *index += 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            '{' => skip_brace_comment(chars, index)?,
            ';' => skip_line_comment(chars, index),
            _ => *index += 1,
        }
    }
    Err("PGN structure error: unclosed variation".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(pgn: &str) -> ImportedGame {
        let mut games = parse_pgn(pgn).expect("valid PGN must parse");
        assert_eq!(games.len(), 1);
        games.remove(0)
    }

    #[test]
    fn parses_a_normal_game_with_headers_and_positions() {
        let game = parse_one(
            r#"[Event "Demo"]
[Site "Madrid"]
[Date "2026.09.18"]
[Round "1"]
[White "White"]
[Black "Black"]
[Result "1/2-1/2"]

1. e4 e5 2. Nf3 Nc6 1/2-1/2"#,
        );

        assert_eq!(game.headers.event.as_deref(), Some("Demo"));
        assert_eq!(game.headers.site.as_deref(), Some("Madrid"));
        assert_eq!(game.headers.result.as_deref(), Some("1/2-1/2"));
        assert_eq!(game.moves.len(), 4);
        assert_eq!(game.positions.len(), 5);
        assert_eq!(game.positions[0], Board::default());
        assert_eq!(
            game.positions[4].to_string(),
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 1"
        );
    }

    #[test]
    fn preserves_multiple_games_in_source_order() {
        let games = parse_pgn(
            r#"[Event "First"]
1. e4 e5 1-0

[Event "Second"]
1. d4 d5 0-1"#,
        )
        .expect("multiple games must parse");

        assert_eq!(games.len(), 2);
        assert_eq!(games[0].headers.event.as_deref(), Some("First"));
        assert_eq!(games[1].headers.event.as_deref(), Some("Second"));
        assert_eq!(games[0].moves.len(), 2);
        assert_eq!(games[1].moves.len(), 2);
    }

    #[test]
    fn separates_consecutive_games_without_headers_after_a_result() {
        let games = parse_pgn("1. e4 e5 1-0\n\n1. d4 d5 0-1")
            .expect("consecutive headerless games must parse");

        assert_eq!(games.len(), 2);
        assert_eq!(games[0].moves.len(), 2);
        assert_eq!(games[1].moves.len(), 2);
    }

    #[test]
    fn ignores_brace_and_line_comments() {
        let game = parse_one("1. e4 { a comment\n[not a header] } e5 ; a line comment\n2. Nf3 Nc6");

        assert_eq!(game.moves.len(), 4);
        assert_eq!(game.positions.len(), 5);
    }

    #[test]
    fn ignores_nags() {
        let game = parse_one("1. e4 $1 e5 $6 2. Nf3 Nc6");

        assert_eq!(game.moves.len(), 4);
    }

    #[test]
    fn ignores_variations_and_replays_only_the_main_line() {
        let game = parse_one("1. e4 e5 (1... c5 (1... e6)) 2. Nf3");

        assert_eq!(game.moves.len(), 3);
        assert_eq!(game.positions.len(), 4);
        assert_eq!(
            game.positions[3].to_string(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 0 1"
        );
    }

    #[test]
    fn starts_from_a_setup_fen() {
        let initial_fen = "8/8/8/8/8/8/8/K6k w - - 0 1";
        let game = parse_one(&format!("[SetUp \"1\"]\n[FEN \"{initial_fen}\"]\n\n1. Kb1"));

        assert_eq!(game.headers.set_up.as_deref(), Some("1"));
        assert_eq!(game.headers.fen.as_deref(), Some(initial_fen));
        assert_eq!(
            game.positions[0],
            Board::from_str(initial_fen).expect("fixture FEN is valid")
        );
        assert_eq!(game.positions.len(), 2);
    }

    #[test]
    fn normalizes_standard_promotion_and_zero_castling_for_validation() {
        let promotion =
            parse_one("[SetUp \"1\"]\n[FEN \"8/P7/8/8/8/8/7k/K7 w - - 0 1\"]\n\n1. a8=Q");
        assert_eq!(promotion.moves.len(), 1);

        let castling = parse_one("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. 0-0");
        assert_eq!(castling.moves.len(), 7);
    }

    #[test]
    fn rejects_a_malformed_promotion_instead_of_removing_arbitrary_equals_signs() {
        let error = parse_pgn("[SetUp \"1\"]\n[FEN \"8/P7/8/8/8/8/7k/K7 w - - 0 1\"]\n\n1. a8==Q")
            .expect_err("malformed promotion must fail");

        assert!(error.contains("invalid SAN move 'a8==Q'"));
    }

    #[test]
    fn rejects_an_incorrect_check_suffix() {
        let error = parse_pgn("1. e4+").expect_err("e4 does not give check");

        assert!(error.contains("check suffix"));
    }

    #[test]
    fn accepts_a_real_check_and_checkmate_suffix() {
        let check = parse_one("[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/3QK3 w - - 0 1\"]\n\n1. Qh5+");
        assert_eq!(check.moves.len(), 1);

        let mate = parse_one("[SetUp \"1\"]\n[FEN \"7k/5Q2/6K1/8/8/8/8/8 w - - 0 1\"]\n\n1. Qg7#");
        assert_eq!(mate.moves.len(), 1);
    }

    #[test]
    fn rejects_an_illegal_or_invalid_san_move() {
        let error = parse_pgn("1. e5").expect_err("illegal SAN must fail");

        assert!(error.contains("invalid SAN move 'e5'"));
    }

    #[test]
    fn rejects_an_invalid_initial_fen() {
        let error = parse_pgn("[SetUp \"1\"]\n[FEN \"not a FEN\"]\n\n1. e4")
            .expect_err("invalid FEN must fail");

        assert!(error.contains("invalid initial FEN"));
    }

    #[test]
    fn rejects_empty_or_unusable_pgn_input() {
        for input in ["", "   \n\t", "{ comment only }\n; line comment\n$1 1."] {
            assert!(parse_pgn(input).is_err(), "{input:?} must fail");
        }
    }

    #[test]
    fn rejects_an_isolated_header_block_as_unusable() {
        let error = parse_pgn("[Event \"Incompleta\"]")
            .expect_err("headers without movetext or a result must fail");

        assert!(error.contains("no usable games"));
    }

    #[test]
    fn requires_setup_and_fen_together_for_custom_positions() {
        let error = parse_pgn("[SetUp \"1\"]\n\n1. e4").expect_err("missing FEN must fail");

        assert!(error.contains("requires a FEN tag"));
    }
}
