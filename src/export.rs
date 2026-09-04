use std::collections::VecDeque;
use std::str::FromStr;
use lopdf::dictionary;
use lopdf::{Document, Object, Stream};
use lopdf::content::{Content, Operation};
use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece, Rank, Square};

use crate::{config, PuzzleTab, lang};

// ─── Private helpers for PGN generation ────────────────────────────────────

/// Parse a UCI move string and verify it is legal on the given board.
fn parse_legal_uci_move(board: &Board, uci: &str) -> Result<ChessMove, String> {
    let (source_str, dest_str, promo_char) = if uci.len() == 5 {
        (&uci[0..2], &uci[2..4], Some(uci[4..5].to_lowercase()))
    } else if uci.len() == 4 {
        (&uci[0..2], &uci[2..4], None)
    } else {
        return Err(format!("Invalid UCI length: {}", uci));
    };

    let source = Square::from_str(source_str)
        .map_err(|e| format!("Invalid source square '{}': {:?}", source_str, e))?;
    let dest = Square::from_str(dest_str)
        .map_err(|e| format!("Invalid destination square '{}': {:?}", dest_str, e))?;

    let promotion = match promo_char.as_deref() {
        Some("q") => Some(Piece::Queen),
        Some("r") => Some(Piece::Rook),
        Some("b") => Some(Piece::Bishop),
        Some("n") => Some(Piece::Knight),
        Some("") | None => None,
        Some(other) => return Err(format!("Invalid promotion piece: '{}'", other)),
    };

    let chess_move = ChessMove::new(source, dest, promotion);
    if board.legal(chess_move) {
        Ok(chess_move)
    } else {
        Err(format!("Illegal UCI move: {} on board {}", uci, board))
    }
}

/// Generate standard (language-independent) SAN for a legal move.
///
/// Uses KQRBN letters, O-O / O-O-O for castling, x for captures,
/// =Q/=R/=B/=N for promotions, + for check, # for checkmate.
fn move_to_standard_san(board: &Board, chess_move: ChessMove) -> Result<String, String> {
    if !board.legal(chess_move) {
        return Err(format!("Illegal move on board {}", board));
    }

    let source = chess_move.get_source();
    let dest = chess_move.get_dest();
    let piece = board.piece_on(source).ok_or("No piece on source square")?;
    let is_capture = board.piece_on(dest).is_some()
        || (piece == Piece::Pawn && source.get_file() != dest.get_file());

    // Castling
    if piece == Piece::King {
        let file_diff = (source.get_file().to_index() as i8) - (dest.get_file().to_index() as i8);
        if file_diff == 2 {
            let mut san = "O-O-O".to_string();
            let next = board.make_move_new(chess_move);
            match next.status() {
                BoardStatus::Checkmate => san.push('#'),
                _ if next.checkers().popcnt() != 0 => san.push('+'),
                _ => {}
            }
            return Ok(san);
        } else if file_diff == -2 {
            let mut san = "O-O".to_string();
            let next = board.make_move_new(chess_move);
            match next.status() {
                BoardStatus::Checkmate => san.push('#'),
                _ if next.checkers().popcnt() != 0 => san.push('+'),
                _ => {}
            }
            return Ok(san);
        }
    }

    let mut san = String::new();

    // Piece letter (pawns have none)
    match piece {
        Piece::King => san.push('K'),
        Piece::Queen => san.push('Q'),
        Piece::Rook => san.push('R'),
        Piece::Bishop => san.push('B'),
        Piece::Knight => san.push('N'),
        Piece::Pawn => {}
    }

    // Disambiguation for non-pawn, non-king pieces
    if piece != Piece::King && piece != Piece::Pawn {
        let mut has_ambiguity = false;
        let mut same_file = false;
        let mut same_rank = false;
        for legal_m in MoveGen::new_legal(board) {
            if legal_m == chess_move {
                continue;
            }
            if board.piece_on(legal_m.get_source()) == Some(piece)
                && legal_m.get_dest() == dest
            {
                has_ambiguity = true;
                if legal_m.get_source().get_file() == source.get_file() {
                    same_file = true;
                }
                if legal_m.get_source().get_rank() == source.get_rank() {
                    same_rank = true;
                }
            }
        }
        if has_ambiguity {
            if !same_file {
                let file_char = (b'a' + source.get_file().to_index() as u8) as char;
                san.push(file_char);
            } else if !same_rank {
                let rank_char = (b'1' + source.get_rank().to_index() as u8) as char;
                san.push(rank_char);
            } else {
                let file_char = (b'a' + source.get_file().to_index() as u8) as char;
                let rank_char = (b'1' + source.get_rank().to_index() as u8) as char;
                san.push(file_char);
                san.push(rank_char);
            }
        }
    }

    // Capture
    if is_capture {
        if piece == Piece::Pawn {
            let file_char = (b'a' + source.get_file().to_index() as u8) as char;
            // Insert file before 'x' for pawn captures
            let mut capture_str = String::new();
            capture_str.push(file_char);
            capture_str.push('x');
            capture_str.push_str(&dest.to_string());
            san.push_str(&capture_str);
        } else {
            san.push('x');
            san.push_str(&dest.to_string());
        }
    } else {
        san.push_str(&dest.to_string());
    }

    // Promotion
    if let Some(promo) = chess_move.get_promotion() {
        let promo_char = match promo {
            Piece::Queen => "Q",
            Piece::Rook => "R",
            Piece::Bishop => "B",
            Piece::Knight => "N",
            _ => return Err("Invalid promotion piece".to_string()),
        };
        san.push('=');
        san.push_str(promo_char);
    }

    // Check / checkmate
    let next = board.make_move_new(chess_move);
    match next.status() {
        BoardStatus::Checkmate => san.push('#'),
        _ if next.checkers().popcnt() != 0 => san.push('+'),
        _ => {}
    }

    Ok(san)
}

/// Convert a board to a PGN-compatible FEN string.
///
/// The `chess` crate stores en passant as the destination of the double-pushed
/// pawn (e.g. e5 after e7-e5), but standard FEN requires the capture-target
/// square (e.g. e6). This function corrects that and normalizes counters to `0 1`.
fn board_to_pgn_fen(board: &Board) -> Result<String, String> {
    let fen = board.to_string();
    let fields: Vec<&str> = fen.split_whitespace().collect();
    if fields.len() != 6 {
        return Err(format!("Unexpected FEN field count: {}", fields.len()));
    }

    // Correct en passant: the chess crate stores the double-push destination,
    // but standard FEN uses the capture-target square (one rank further).
    let ep_field = match board.en_passant() {
        None => "-".to_string(),
        Some(sq) => {
            let rank = sq.get_rank().to_index();
            let file = sq.get_file();
            let target_rank = if board.side_to_move() == Color::White {
                // Black just moved: target is one rank further toward rank 8
                Rank::from_index(rank + 1)
            } else {
                // White just moved: target is one rank further toward rank 1
                Rank::from_index(rank - 1)
            };
            Square::make_square(target_rank, file).to_string()
        }
    };

    Ok(format!(
        "{} {} {} {} 0 1",
        fields[0], fields[1], fields[2], ep_field
    ))
}

/// Build the PGN text for a single puzzle as a complete game.
fn build_pgn_game(puzzle: &config::Puzzle, date: &str) -> Result<String, String> {
    let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
    if moves.is_empty() {
        return Err("Puzzle has no moves".to_string());
    }

    // Parse original FEN
    let original_board = Board::from_str(&puzzle.fen)
        .map_err(|e| format!("Invalid FEN '{}': {:?}", puzzle.fen, e))?;

    // Apply trigger move (moves[0]) — it is NOT part of the solution
    let trigger = parse_legal_uci_move(&original_board, moves[0])
        .map_err(|e| format!("Trigger move error: {}", e))?;
    let puzzle_board = original_board.make_move_new(trigger);

    // The solution moves start at index 1
    let solution_moves = if moves.len() >= 2 { &moves[1..] } else { &[] };

    // Determine side to move AFTER the trigger
    let solver_is_white = puzzle_board.side_to_move() == Color::White;

    // Build FEN
    let fen = board_to_pgn_fen(&puzzle_board)?;

    // Build headers
    let mut pgn = String::new();
    pgn.push_str("[Event \"Chess Puzzle\"]\n");
    pgn.push_str(&format!(
        "[Site \"https://lichess.org/training/{}\"]\n",
        puzzle.puzzle_id
    ));
    pgn.push_str(&format!("[Date \"{}\"]\n", date));
    pgn.push_str("[Round \"-\"]\n");
    pgn.push_str(&format!(
        "[White \"{}\"]\n",
        if solver_is_white {
            "Player"
        } else {
            "Opponent"
        }
    ));
    pgn.push_str(&format!(
        "[Black \"{}\"]\n",
        if solver_is_white {
            "Opponent"
        } else {
            "Player"
        }
    ));
    pgn.push_str("[Result \"*\"]\n");
    pgn.push_str(&format!("[GameID \"{}\"]\n", puzzle.game_url));
    pgn.push_str(&format!("[FEN \"{}\"]\n", fen));
    pgn.push_str("[SetUp \"1\"]\n");
    if !puzzle.opening.is_empty() {
        pgn.push_str(&format!("[Opening \"{}\"]\n", puzzle.opening));
    }
    pgn.push_str(&format!("[PuzzleRating \"{}\"]\n", puzzle.rating));
    pgn.push_str(&format!(
        "[PuzzleRatingDeviation \"{}\"]\n",
        puzzle.rating_deviation
    ));
    pgn.push_str(&format!("[PuzzlePopularity \"{}\"]\n", puzzle.popularity));
    pgn.push_str(&format!("[PuzzleNbPlays \"{}\"]\n", puzzle.nb_plays));
    pgn.push_str(&format!("[PuzzleThemes \"{}\"]\n", puzzle.themes));
    pgn.push('\n');

    // Build move text
    let mut board = puzzle_board;
    let mut move_number: usize = 1;
    let mut move_text = String::new();
    let mut first_move = true;

    for uci_move in solution_moves {
        let chess_move = parse_legal_uci_move(&board, uci_move)
            .map_err(|e| format!("Solution move error: {}", e))?;
        let san = move_to_standard_san(&board, chess_move)?;

        if board.side_to_move() == Color::White {
            if !move_text.is_empty() {
                move_text.push(' ');
            }
            move_text.push_str(&format!("{}. ", move_number));
        } else if first_move {
            move_text.push_str(&format!("{}... ", move_number));
        } else {
            move_text.push(' ');
        }

        move_text.push_str(&san);

        board = board.make_move_new(chess_move);

        if board.side_to_move() == Color::White {
            move_number += 1;
        }
        first_move = false;
    }

    if move_text.is_empty() {
        pgn.push_str("*\n");
    } else {
        move_text.push_str(" *\n");
        pgn.push_str(&move_text);
    }

    Ok(pgn)
}

/// Build the full PGN content for multiple puzzles.
fn build_pgn_content(
    puzzles: &[config::Puzzle],
    date: &str,
) -> Result<String, String> {
    let mut content = String::new();
    for (i, puzzle) in puzzles.iter().enumerate() {
        let game = build_pgn_game(puzzle, date)?;
        content.push_str(&game);
        if i + 1 < puzzles.len() {
            content.push('\n');
        }
    }
    Ok(content)
}

// ─── Public API ────────────────────────────────────────────────────────────

pub fn to_pgn(puzzles: &[config::Puzzle], _lang: &lang::Language, path: String) {
    let date = chrono::Local::now().format("%Y.%m.%d").to_string();
    match build_pgn_content(puzzles, &date) {
        Ok(content) => {
            if let Err(e) = std::fs::write(&path, &content) {
                eprintln!("Error writing PGN file '{}': {}", path, e);
            }
        }
        Err(e) => {
            eprintln!("Error building PGN content: {}", e);
        }
    }
}

// ─── PDF figurine spans ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum PdfSolutionFont {
    Regular,
    Figurine,
}

#[derive(Debug, Clone, PartialEq)]
struct PdfSolutionSpan {
    font: PdfSolutionFont,
    text: String,
}

/// Map SAN piece letter to Chess Alpha light-square glyph (lowercase).
///
/// Chess Alpha uses uppercase for dark-square pieces and lowercase for
/// light-square pieces. For solution figurines we always use the clean
/// light-square variant (no striped background).
fn chess_alpha_light_square_glyph(san_piece: char) -> char {
    match san_piece {
        'K' => 'k',
        'Q' => 'q',
        'R' => 'r',
        'B' => 'b',
        'N' => 'h',
        other => other,
    }
}

/// Convert standard SAN to PDF figurine spans.
///
/// Piece letters K/Q/R/B/N become Figurine spans rendered with "Chess Alpha"
/// using the light-square glyph variant (lowercase, no striped background).
/// Everything else (files, ranks, x, +, #, =, O-O) becomes Regular spans.
fn standard_san_to_pdf_spans(san: &str) -> Vec<PdfSolutionSpan> {
    if san.starts_with("O-O") {
        return vec![PdfSolutionSpan { font: PdfSolutionFont::Regular, text: san.to_string() }];
    }

    let chars: Vec<char> = san.chars().collect();
    let mut spans = Vec::new();
    let mut regular_buf = String::new();

    for (i, &ch) in chars.iter().enumerate() {
        let is_piece_start = i == 0 && matches!(ch, 'K' | 'Q' | 'R' | 'B' | 'N');
        let is_promo_piece = i > 0 && chars[i - 1] == '=' && matches!(ch, 'Q' | 'R' | 'B' | 'N');

        if is_piece_start || is_promo_piece {
            if !regular_buf.is_empty() {
                spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: regular_buf.clone() });
                regular_buf.clear();
            }
            let fig_char = chess_alpha_light_square_glyph(ch);
            spans.push(PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: fig_char.to_string() });
        } else {
            regular_buf.push(ch);
        }
    }

    if !regular_buf.is_empty() {
        spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: regular_buf });
    }

    spans
}

/// Convert a UCI move to PDF figurine spans via language-independent standard SAN.
fn uci_move_to_pdf_spans(board: &Board, uci: &str) -> Result<Vec<PdfSolutionSpan>, String> {
    let chess_move = parse_legal_uci_move(board, uci)?;
    let san = move_to_standard_san(board, chess_move)?;
    Ok(standard_san_to_pdf_spans(&san))
}

/// Append PDF operations for figurine spans to the operations list.
///
/// Each span emits Tf (font switch), Ts (text rise), then Tj (show text).
/// Figurine spans use Ts -2 to lower the glyph 2pt; Regular spans use Ts 0.
fn append_pdf_solution_spans(ops: &mut Vec<Operation>, spans: &[PdfSolutionSpan]) {
    for span in spans {
        let (font_name, rise) = match span.font {
            PdfSolutionFont::Regular => ("Regular", 0),
            PdfSolutionFont::Figurine => ("Chess Alpha", -1),
        };
        ops.push(Operation::new("Tf", vec![font_name.into(), 12.into()]));
        ops.push(Operation::new("Ts", vec![rise.into()]));
        ops.push(Operation::new("Tj", vec![Object::string_literal(span.text.clone())]));
    }
    ops.push(Operation::new("Ts", vec![0.into()]));
}

// ─── PDF (unchanged) ───────────────────────────────────────────────────────

pub fn to_pdf(puzzles: &[config::Puzzle], number_of_pages: i32, lang: &lang::Language, path: String) {

    // Create a document object and add the font and font descriptor to it
    let mut doc = Document::with_version("1.5");

    let regular_font_id = doc.add_object(dictionary! {
        // type of dictionary
        "Type" => "Font",
        // type of font, type1 is simple postscript font
        "Subtype" => "TrueType",
        // basefont is postscript name of font for type1 font.
        // See PDF reference document for more details
        "BaseFont" => "Arial",
    });
  
    // pages is the root node of the page tree
    let pages_id = doc.new_object_id();
    let font_name = "Chess Alpha".to_string();
    let mut font_data = lopdf::FontData::new(config::CHESS_ALPHA_BYTES, font_name.clone());
    font_data
        .set_flags(33)
        .set_font_bbox((0, 0, 1000, 1000))
        .set_first_char(32)
        .set_last_char(255)
        .set_widths(vec![1000.into();223])
        .set_encoding("WinAnsiEncoding".to_string());
    let font_id = doc.add_font(font_data).unwrap();
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            font_name => font_id,
            "Regular" => regular_font_id,
        },
    });

    let num_of_puzzles_to_print;
    let num_of_pages;
    if (6 * number_of_pages) as usize > puzzles.len() {
        num_of_puzzles_to_print = puzzles.len();
        num_of_pages = (puzzles.len() as f32 / 6.0).ceil() as usize;
    } else {
        num_of_puzzles_to_print = (6 * number_of_pages) as usize;
        num_of_pages = number_of_pages as usize;
    };

    //let number_of_pages: i64 = 100;//(puzzles.len() / 6).try_into().unwrap();
    let mut page_ids = vec![];
    let mut puzzle_index = 0;
    for _ in 0..num_of_pages {
        let mut ops: Vec<Operation> = vec![];
        let mut pos_x = 750;
        let mut pos_y = 75;
        for i in 0..6 {
            if puzzle_index == puzzles.len() { break };
            ops.append(&mut gen_diagram_operations(puzzle_index + 1, &puzzles[puzzle_index], pos_x, pos_y, lang));
            if i % 2 == 0 {
                pos_y = 325;
            } else {
                pos_y = 75;
                pos_x -= 250;
            };
            puzzle_index += 1;
        }

        // Content is a wrapper struct around an operations struct that contains a vector of operations
        // The operations struct contains a vector of operations that match up with a particular PDF
        // operator and operands.
        // Reference the PDF reference for more details on these operators and operands.
        // Note, the operators and operands are specified in a reverse order than they
        // actually appear in the PDF file itself.
        let content = Content {
            operations: ops,
        };

        // Streams are a dictionary followed by a sequence of bytes. What that sequence of bytes
        // represents depends on context
        // The stream dictionary is set internally to lopdf and normally doesn't
        // need to be manually nanipulated. It contains keys such as
        // Length, Filter, DecodeParams, etc
        //
        // content is a stream of encoded content data.
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));

        // Page is a dictionary that represents one page of a PDF file.
        // It has a type, parent and contents
        //let page_id = doc.add_object(dictionary! {
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
        }).into());
    }

    let mut ops: Vec<Operation> = vec![];
    let mut pos_x = 800;
    let pos_y = 75;
    for (puzzle_number, puzzle) in puzzles.iter().enumerate().take(num_of_puzzles_to_print) {
        // need to start by making the 1st move in the list, because it's only then that
        // the puzzle starts.
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let mut puzzle_moves: VecDeque<&str> = puzzles[puzzle_number].moves.split_whitespace().collect();
        let movement = ChessMove::new(
            Square::from_str(&String::from(&puzzle_moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&puzzle_moves[0][2..4])).unwrap(), PuzzleTab::check_promotion(puzzle_moves[0]));
        board = board.make_move_new(movement);

        // Remove the opponent's first move, it's not part of the solution.
        puzzle_moves.pop_front();

        let mut move_spans: Vec<PdfSolutionSpan> = Vec::new();
        move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: format!("{})", puzzle_number + 1) });
        let mut half_move_number = 1;
        let mut move_label = 1;
        if board.side_to_move() == Color::Black {
            move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: " 1. ... ".to_string() });
            half_move_number = 2;
            move_label = 2;
        }
        for chess_move in puzzle_moves {
            if half_move_number % 2 == 0 {
                move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: " ".to_string() });
            } else {
                move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: format!(" {}. ", move_label) });
                move_label += 1;
            }
            let spans = uci_move_to_pdf_spans(&board, chess_move).unwrap();
            move_spans.extend(spans);
            half_move_number += 1;
            let movement = ChessMove::new(
                Square::from_str(&String::from(&chess_move[..2])).unwrap(),
                Square::from_str(&String::from(&chess_move[2..4])).unwrap(), PuzzleTab::check_promotion(chess_move));
            board = board.make_move_new(movement);
        }
        ops.append(&mut vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["Regular".into(), 12.into()]),
                Operation::new("rg", vec![0.into(),0.into(),0.into()]),
                Operation::new("Td", vec![pos_y.into(), pos_x.into()]),
        ]);
        append_pdf_solution_spans(&mut ops, &move_spans);
        ops.push(Operation::new("ET", vec![]));
        pos_x -= 18;

        // We need a page break
        if pos_x < 18 {
            pos_x = 800;

            let content = Content {
                operations: ops,
            };

            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            page_ids.push(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            }).into());
            ops = vec![];
        }
    }
    let content = Content {
        operations: ops,
    };

    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    page_ids.push(doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
    }).into());

    // Again, pages is the root of the page tree. The ID was already created
    // at the top of the page, since we needed it to assign to the parent element of the page
    // dictionary
    //
    // This is just the basic requirements for a page tree root object. There are also many
    // additional entries that can be added to the dictionary if needed. Some of these can also be
    // defined on the page dictionary itself, and not inherited from the page tree root.
    let pages = dictionary! {
        // Type of dictionary
        "Type" => "Pages",
        // Page count
        "Count" => Object::Integer(page_ids.len() as i64),
        // Vector of page IDs in document. Normally would contain more than one ID and be produced
        // using a loop of some kind
        "Kids" => page_ids,
        // ID of resources dictionary, defined earlier
        "Resources" => resources_id,
        // a rectangle that defines the boundaries of the physical or digital media. This is the
        // "Page Size"
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 850.into()],
    };

    // using insert() here, instead of add_object() since the id is already known.
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    // Creating document catalog.
    // There are many more entries allowed in the catalog dictionary.
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });

    // Root key in trailer is set here to ID of document catalog,
    // remainder of trailer is set during doc.save().
    doc.trailer.set("Root", catalog_id);
    doc.compress();

    // Store file in current working directory.
    let _ = doc.save(path);
}

fn pdf_board_labels(white_at_bottom: bool) -> ([i32; 8], [i32; 8]) {
    if white_at_bottom {
        ([0, 1, 2, 3, 4, 5, 6, 7], [7, 6, 5, 4, 3, 2, 1, 0])
    } else {
        ([7, 6, 5, 4, 3, 2, 1, 0], [0, 1, 2, 3, 4, 5, 6, 7])
    }
}

fn pdf_draw_side_circle(ops: &mut Vec<Operation>, cx: i32, cy: i32, r: i32, white_side: bool) {
    let c = ((0.5522847498 * r as f64) as i32).max(1);

    ops.push(Operation::new("q", vec![]));
    if white_side {
        ops.push(Operation::new("rg", vec![1.into(), 1.into(), 1.into()]));
        ops.push(Operation::new("RG", vec![0.into(), 0.into(), 0.into()]));
        ops.push(Operation::new("w", vec![0.5.into()]));
    } else {
        ops.push(Operation::new("rg", vec![0.into(), 0.into(), 0.into()]));
    }
    ops.push(Operation::new("m", vec![(cx + r).into(), cy.into()]));
    ops.push(Operation::new("c", vec![
        (cx + r).into(), (cy + c).into(),
        (cx + c).into(), (cy + r).into(),
        cx.into(), (cy + r).into()
    ]));
    ops.push(Operation::new("c", vec![
        (cx - c).into(), (cy + r).into(),
        (cx - r).into(), (cy + c).into(),
        (cx - r).into(), cy.into()
    ]));
    ops.push(Operation::new("c", vec![
        (cx - r).into(), (cy - c).into(),
        (cx - c).into(), (cy - r).into(),
        cx.into(), (cy - r).into()
    ]));
    ops.push(Operation::new("c", vec![
        (cx + c).into(), (cy - r).into(),
        (cx + r).into(), (cy - c).into(),
        (cx + r).into(), cy.into()
    ]));
    ops.push(Operation::new("h", vec![]));
    if white_side {
        ops.push(Operation::new("B", vec![]));
    } else {
        ops.push(Operation::new("f", vec![]));
    }
    ops.push(Operation::new("Q", vec![]));
}

fn gen_diagram_operations(index: usize, puzzle: &config::Puzzle, start_x:i32, start_y:i32, _lang: &lang::Language) -> Vec<Operation> {
    let mut board = Board::from_str(&puzzle.fen).unwrap();
    let puzzle_moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
    let movement = ChessMove::new(
        Square::from_str(&String::from(&puzzle_moves[0][..2])).unwrap(),
        Square::from_str(&String::from(&puzzle_moves[0][2..4])).unwrap(), PuzzleTab::check_promotion(puzzle_moves[0]));
    board = board.make_move_new(movement);

    let white_at_bottom = board.side_to_move() == Color::White;
    let (files, ranks) = pdf_board_labels(white_at_bottom);

    let mut ops = vec![];

    let number_str = index.to_string();
    let num_width = number_str.len() as i32 * 7;
    let icon_diameter = 10;
    let gap = 4;
    let total_width = num_width + gap + icon_diameter;
    let header_x = start_y + 100 - total_width / 2;
    let header_y = start_x + 30;
    let icon_cx = header_x + num_width + gap + icon_diameter / 2;
    let icon_cy = header_y + 5;

    ops.extend_from_slice(&[
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["Regular".into(), 12.into()]),
        Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
        Operation::new("Td", vec![header_x.into(), header_y.into()]),
        Operation::new("Tj", vec![Object::string_literal(number_str)]),
        Operation::new("ET", vec![]),
    ]);

    pdf_draw_side_circle(&mut ops, icon_cx, icon_cy, 5, white_at_bottom);

    for (i, &rank) in ranks.iter().enumerate() {
        let label_y = start_x - (i as i32) * 25 + 5;
        let label_x = start_y - 10;
        let rank_char = (b'0' + (rank + 1) as u8) as char;
        ops.extend_from_slice(&[
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["Regular".into(), 8.into()]),
            Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
            Operation::new("Td", vec![label_x.into(), label_y.into()]),
            Operation::new("Tj", vec![Object::string_literal(rank_char.to_string())]),
            Operation::new("ET", vec![]),
        ]);
    }

    ops.extend_from_slice(&[
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["Chess Alpha".into(), 25.into()]),
        Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
        Operation::new("Td", vec![start_y.into(), start_x.into()]),
    ]);

    for rank in ranks {
        let mut rank_string = String::new();
        for file in &files {
            let mut new_piece;
            let light_square = (rank + file) % 2 != 0;
            let square = chess::Square::make_square(chess::Rank::from_index(rank as usize),chess::File::from_index(*file as usize));
            let (piece, color) =
                (board.piece_on(square),
                board.color_on(square));

            if let Some(piece) = piece {
                if color.unwrap() == Color::White {
                    match piece {
                        Piece::Pawn => new_piece = 'P',
                        Piece::Rook => new_piece = 'R',
                        Piece::Knight => new_piece = 'H',
                        Piece::Bishop => new_piece = 'B',
                        Piece::Queen => new_piece = 'Q',
                        Piece::King => new_piece = 'K',
                    }
                    if light_square {
                        new_piece = new_piece.to_lowercase().collect::<Vec<_>>()[0];
                    }
                } else {
                    match piece {
                        Piece::Rook => new_piece = 'T',
                        Piece::Knight => new_piece = 'J',
                        Piece::Bishop => new_piece = 'N',
                        Piece::Queen => new_piece = 'W',
                        Piece::King => new_piece = 'L',
                        Piece::Pawn => new_piece = 'O',
                    }
                    if light_square {
                        new_piece = new_piece.to_lowercase().collect::<Vec<_>>()[0];
                    }
                }
            } else if light_square {
                new_piece = ' ';
            } else {
                new_piece = '+';
            }
            rank_string.push(new_piece);
        }
        ops.push(Operation::new("Tj", vec![Object::string_literal(rank_string)]));
        ops.push(Operation::new("Td", vec![0.into(), Object::Integer(-25)]));
    }
    ops.push(Operation::new("ET", vec![]));

    for (i, &file) in files.iter().enumerate() {
        let label_x = start_y + (i as i32) * 25 + 10;
        let label_y = start_x - 190;
        let file_char = (b'a' + file as u8) as char;
        ops.extend_from_slice(&[
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["Regular".into(), 8.into()]),
            Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
            Operation::new("Td", vec![label_x.into(), label_y.into()]),
            Operation::new("Tj", vec![Object::string_literal(file_char.to_string())]),
            Operation::new("ET", vec![]),
        ]);
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

    fn read_fixture_puzzles() -> Vec<config::Puzzle> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(FIXTURE.as_bytes());
        reader
            .deserialize::<config::Puzzle>()
            .map(|r| r.expect("fixture row should deserialize"))
            .collect()
    }

    fn fixture_puzzle_00010() -> config::Puzzle {
        let puzzles = read_fixture_puzzles();
        puzzles.into_iter().find(|p| p.puzzle_id == "00010").expect("fixture must contain puzzle 00010")
    }

    // ── parse_legal_uci_move ──

    #[test]
    fn test_parse_legal_uci_move_valid() {
        let board = Board::default();
        let mv = parse_legal_uci_move(&board, "e2e4").unwrap();
        assert_eq!(mv.get_source(), Square::E2);
        assert_eq!(mv.get_dest(), Square::E4);
    }

    #[test]
    fn test_parse_legal_uci_move_illegal() {
        let board = Board::default();
        let result = parse_legal_uci_move(&board, "e2e5");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_legal_uci_move_invalid_square() {
        let board = Board::default();
        let result = parse_legal_uci_move(&board, "z9z9");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_legal_uci_move_wrong_length() {
        let board = Board::default();
        let result = parse_legal_uci_move(&board, "e2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_legal_uci_move_with_promotion() {
        let board = Board::from_str("8/4P3/8/8/8/8/8/4K2k w - - 0 1").unwrap();
        let mv = parse_legal_uci_move(&board, "e7e8q").unwrap();
        assert_eq!(mv.get_promotion(), Some(Piece::Queen));
    }

    // ── move_to_standard_san ──

    #[test]
    fn test_san_pawn() {
        let board = Board::default();
        let mv = ChessMove::new(Square::E2, Square::E4, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "e4");
    }

    #[test]
    fn test_san_pawn_short() {
        let board = Board::default();
        let mv = ChessMove::new(Square::E2, Square::E3, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "e3");
    }

    #[test]
    fn test_san_piece() {
        let board = Board::default();
        let mv = ChessMove::new(Square::G1, Square::F3, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Nf3");
    }

    #[test]
    fn test_san_bishop() {
        // Italian Game position where Bc4 is legal (e2 pawn moved to e4)
        let board = Board::from_str("r1bqkbnr/pppppppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3").unwrap();
        let mv = ChessMove::new(Square::F1, Square::C4, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Bc4");
    }

    #[test]
    fn test_san_castling_kingside() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let mv = ChessMove::new(Square::E1, Square::G1, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "O-O");
    }

    #[test]
    fn test_san_castling_queenside() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let mv = ChessMove::new(Square::E1, Square::C1, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "O-O-O");
    }

    #[test]
    fn test_san_castling_with_check() {
        // White O-O places rook on f1, giving check to black king on f8
        let board = Board::from_str("5k2/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mv = ChessMove::new(Square::E1, Square::G1, None);
        assert!(board.legal(mv));
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "O-O+");
        // from_san doesn't support + suffix; roundtrip via O-O
        let mv_back = ChessMove::from_san(&board, "O-O").unwrap();
        assert_eq!(mv_back, mv);
    }

    #[test]
    fn test_san_capture() {
        let board = Board::from_str("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
        let mv = ChessMove::new(Square::D7, Square::D5, None);
        let _san = move_to_standard_san(&board, mv).unwrap();
        // d5 is not a capture from d7
        let board2 = Board::from_str("rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2").unwrap();
        let mv2 = ChessMove::new(Square::D7, Square::D5, None);
        let san2 = move_to_standard_san(&board2, mv2).unwrap();
        assert_eq!(san2, "d5");
    }

    #[test]
    fn test_san_pawn_capture() {
        // Create a position where exd5 is a pawn capture
        let board = Board::from_str("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2").unwrap();
        let mv = ChessMove::new(Square::E4, Square::D5, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "exd5");
    }

    #[test]
    fn test_san_en_passant() {
        // White pawn on e5, black pawn just played d7d5
        let board = Board::from_str("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3").unwrap();
        let mv = ChessMove::new(Square::E5, Square::D6, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "exd6");
    }

    #[test]
    fn test_san_promotion() {
        let board = Board::from_str("8/4P3/8/8/8/8/8/4K2k w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::E7, Square::E8, Some(Piece::Queen));
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "e8=Q");
    }

    #[test]
    fn test_san_promotion_capture() {
        let board = Board::from_str("3r3k/4P3/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::E7, Square::D8, Some(Piece::Queen));
        let san = move_to_standard_san(&board, mv).unwrap();
        // After exd8=Q, the queen on d8 attacks g8 (check)
        assert_eq!(san, "exd8=Q+");
    }

    #[test]
    fn test_san_check() {
        // Position where Bxf7+ gives check (Italian Game)
        let board = Board::from_str("r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4").unwrap();
        let mv = ChessMove::new(Square::C4, Square::F7, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert!(san.ends_with('+'), "Expected check, got: {}", san);
    }

    #[test]
    fn test_san_checkmate() {
        // Scholar's mate position
        let board = Board::from_str("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
        let mv = ChessMove::new(Square::H5, Square::F7, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert!(san.ends_with('#'), "Expected checkmate, got: {}", san);
    }

    #[test]
    fn test_san_disambiguation_file() {
        // Two rooks on a2 and c2 that can both go to b2
        let board = Board::from_str("4k3/8/8/8/8/8/R1R5/4K3 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::A2, Square::B2, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Rab2");
    }

    #[test]
    fn test_san_disambiguation_rank() {
        // Two rooks on a1 and a3 (same file), both can go to a2
        let board = Board::from_str("4k3/8/8/8/8/R7/8/R3K3 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::A1, Square::A2, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "R1a2");
    }

    #[test]
    fn test_san_disambiguation_different_file_and_rank() {
        // Rook A on a1, Rook B on c3, destination c1 — different file AND rank
        let board = Board::from_str("4k3/8/8/8/8/2R5/8/R3K3 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::A1, Square::C1, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Rac1");
        // Roundtrip
        let mv_back = ChessMove::from_san(&board, "Rac1").unwrap();
        assert_eq!(mv_back, mv);
    }

    #[test]
    fn test_san_disambiguation_both_file_and_rank() {
        // Knight on b1, same-file alternative on b3, same-rank alternative on f1, dest d2
        let board = Board::from_str("4k3/8/8/8/8/1N6/8/1N2KN2 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::B1, Square::D2, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Nb1d2");
        // Roundtrip (from_san may not support both-file-and-rank, so just verify via SAN)
        let mv_back = ChessMove::from_san(&board, &san).unwrap();
        assert_eq!(mv_back, mv);
    }

    #[test]
    fn test_san_from_san_roundtrip_pawn() {
        let board = Board::default();
        let san = "e4";
        let mv = ChessMove::from_san(&board, san).unwrap();
        assert_eq!(mv.get_source(), Square::E2);
        assert_eq!(mv.get_dest(), Square::E4);
    }

    #[test]
    fn test_san_from_san_roundtrip_piece() {
        let board = Board::default();
        let san = "Nf3";
        let mv = ChessMove::from_san(&board, san).unwrap();
        assert_eq!(mv.get_source(), Square::G1);
        assert_eq!(mv.get_dest(), Square::F3);
    }

    #[test]
    fn test_san_from_san_roundtrip_castling() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let san = "O-O";
        let mv = ChessMove::from_san(&board, san).unwrap();
        assert_eq!(mv.get_source(), Square::E1);
        assert_eq!(mv.get_dest(), Square::G1);
    }

    #[test]
    fn test_san_from_san_roundtrip_castling_queenside() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let san = "O-O-O";
        let mv = ChessMove::from_san(&board, san).unwrap();
        assert_eq!(mv.get_source(), Square::E1);
        assert_eq!(mv.get_dest(), Square::C1);
    }

    // ── board_to_pgn_fen ──

    #[test]
    fn test_board_to_pgn_fen_basic() {
        let board = Board::default();
        let fen = board_to_pgn_fen(&board).unwrap();
        assert_eq!(fen, "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    }

    #[test]
    fn test_board_to_pgn_fen_after_trigger() {
        let puzzle = fixture_puzzle_00010();
        let original_board = Board::from_str(&puzzle.fen).unwrap();
        let trigger = parse_legal_uci_move(&original_board, "f3g5").unwrap();
        let puzzle_board = original_board.make_move_new(trigger);
        let fen = board_to_pgn_fen(&puzzle_board).unwrap();
        assert!(fen.ends_with(" 0 1"), "FEN must end with ' 0 1': {}", fen);
        assert_ne!(fen, puzzle.fen, "Exported FEN must differ from original");
    }

    #[test]
    fn test_board_to_pgn_fen_en_passant_target_square() {
        // Position where a pawn double-pushed and an opposing pawn can capture en passant.
        // After ...d5, white pawn on e5 can capture en passant on d6.
        let board = Board::from_str("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3").unwrap();
        let fen = board_to_pgn_fen(&board).unwrap();
        let ep_field = fen.split_whitespace().nth(3).unwrap();
        assert_eq!(ep_field, "d6", "En passant target should be d6, got: {}", ep_field);
    }

    // ── build_pgn_game ──

    #[test]
    fn test_build_pgn_game_00010() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();

        // Check headers
        assert!(pgn.contains("[SetUp \"1\"]"));
        assert!(pgn.contains("[FEN \""));
        assert!(pgn.contains("[Event \"Chess Puzzle\"]"));
        assert!(pgn.contains(&format!("[Site \"https://lichess.org/training/{}\"]", puzzle.puzzle_id)));
        assert!(pgn.contains("[Result \"*\"]"));

        // After trigger (f3g5), it's Black's turn
        // So White="Opponent", Black="Player"
        assert!(pgn.contains("[White \"Opponent\"]"));
        assert!(pgn.contains("[Black \"Player\"]"));

        // Check move text starts with 1...
        assert!(pgn.contains("1... "), "Move text should start with '1...' for Black to move");

        // Check trigger is NOT in move text
        let trigger_san = {
            let original_board = Board::from_str(&puzzle.fen).unwrap();
            let trigger = parse_legal_uci_move(&original_board, "f3g5").unwrap();
            move_to_standard_san(&original_board, trigger).unwrap()
        };
        let move_section = pgn.split("\n\n").last().unwrap_or("");
        assert!(!move_section.contains(&trigger_san), "Trigger SAN '{}' must not appear in move text", trigger_san);

        // Check result
        assert!(pgn.ends_with("*\n"));
    }

    #[test]
    fn test_build_pgn_game_trigger_absent_from_solution() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        let move_text = pgn.split("\n\n").last().unwrap_or("");

        // The trigger move is Nf3-g5 = Ng5 (knight from f3 to g5)
        // After applying it, the solution starts with e7e6 = e6
        // Verify "Ng5" does not appear in move text
        assert!(!move_text.contains("Ng5"), "Trigger move must not be in solution");
    }

    #[test]
    fn test_build_pgn_game_fen_after_trigger() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();

        // Extract FEN from PGN
        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();

        // Parse it
        let exported_board = Board::from_str(fen).unwrap();

        // Compute expected: original + trigger
        let original_board = Board::from_str(&puzzle.fen).unwrap();
        let trigger = parse_legal_uci_move(&original_board, "f3g5").unwrap();
        let expected_board = original_board.make_move_new(trigger);

        assert_eq!(exported_board, expected_board, "Exported FEN must match board after trigger");
    }

    #[test]
    fn test_build_pgn_game_move_sequence_parses() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();

        // Extract FEN
        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();
        let mut board = Board::from_str(fen).unwrap();

        // Extract move text (after the blank line)
        let move_text = pgn.split("\n\n").last().unwrap_or("");

        // Parse SAN tokens, ignoring move numbers and result
        let tokens: Vec<&str> = move_text
            .split_whitespace()
            .filter(|t| !t.starts_with(|c: char| c.is_ascii_digit()) && !t.starts_with('*') && *t != "...")
            .collect();

        // Expected UCI solution moves (excluding trigger)
        let all_moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let expected_uci: Vec<&str> = all_moves[1..].to_vec();

        assert_eq!(tokens.len(), expected_uci.len(), "SAN token count must match solution move count");

        for (san, uci) in tokens.iter().zip(expected_uci.iter()) {
            // Parse SAN with from_san
            let mv_from_san = ChessMove::from_san(&board, san)
                .unwrap_or_else(|e| panic!("Failed to parse SAN '{}': {:?}", san, e));

            // Parse UCI
            let mv_from_uci = ChessMove::from_str(uci)
                .unwrap_or_else(|e| panic!("Failed to parse UCI '{}': {:?}", uci, e));

            assert_eq!(mv_from_san, mv_from_uci, "SAN '{}' must equal UCI '{}'", san, uci);

            board = board.make_move_new(mv_from_san);
        }
    }

    #[test]
    fn test_build_pgn_game_moves_navigable_from_fen() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();

        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();
        let mut board = Board::from_str(fen).unwrap();

        let move_text = pgn.split("\n\n").last().unwrap_or("");
        let tokens: Vec<&str> = move_text
            .split_whitespace()
            .filter(|t| !t.starts_with(|c: char| c.is_ascii_digit()) && !t.starts_with('*') && *t != "...")
            .collect();

        // Every SAN must parse and be legal
        for san in &tokens {
            let mv = ChessMove::from_san(&board, san)
                .unwrap_or_else(|e| panic!("SAN '{}' not parseable: {:?}", san, e));
            assert!(board.legal(mv), "SAN '{}' is not legal on current board", san);
            board = board.make_move_new(mv);
        }
    }

    // ── build_pgn_content ──

    #[test]
    fn test_build_pgn_content_multiple_puzzles() {
        let puzzles = read_fixture_puzzles();
        let content = build_pgn_content(&puzzles, "2026.09.03").unwrap();
        // Should contain games separated by blank lines
        let game_count = content.matches("[Event \"Chess Puzzle\"]").count();
        assert_eq!(game_count, puzzles.len());
    }

    // ── Error handling ──

    #[test]
    fn test_build_pgn_game_empty_moves() {
        let puzzle = config::Puzzle {
            puzzle_id: "test".to_string(),
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string(),
            moves: "".to_string(),
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let result = build_pgn_game(&puzzle, "2026.09.03");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_pgn_game_invalid_fen() {
        let puzzle = config::Puzzle {
            puzzle_id: "test".to_string(),
            fen: "not-a-fen".to_string(),
            moves: "e2e4".to_string(),
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let result = build_pgn_game(&puzzle, "2026.09.03");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_pgn_game_illegal_trigger() {
        let puzzle = config::Puzzle {
            puzzle_id: "test".to_string(),
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string(),
            moves: "e2e5 e7e5".to_string(), // e2e5 is illegal
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let result = build_pgn_game(&puzzle, "2026.09.03");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_pgn_game_only_trigger_no_solution() {
        let puzzle = config::Puzzle {
            puzzle_id: "test".to_string(),
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string(),
            moves: "e2e4".to_string(), // only trigger, no solution
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let result = build_pgn_game(&puzzle, "2026.09.03");
        // Single-move puzzles produce a valid PGN with just * as result
        assert!(result.is_ok());
        let pgn = result.unwrap();
        assert!(pgn.contains("[SetUp \"1\"]"));
        assert!(pgn.contains("[FEN \""));
        assert!(pgn.ends_with("*\n"));
    }

    // ── Language independence ──

    #[test]
    fn test_pgn_builder_does_not_use_lang() {
        // The build_pgn_game function does not take a lang parameter,
        // proving it's language-independent.
        // We simply verify it compiles and produces correct SAN.
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        // Standard SAN letters, not localized
        assert!(pgn.contains("Nf7") || pgn.contains("e6"),
            "PGN should contain standard SAN notation");
        // No Spanish/French piece names
        assert!(!pgn.contains("Cf"), "PGN must not contain localized piece names");
        assert!(!pgn.contains("Td"), "PGN must not contain localized piece names");
    }

    // ── Side tags after trigger ──

    #[test]
    fn test_side_tags_black_to_move_after_trigger() {
        let puzzle = fixture_puzzle_00010();
        // After trigger (f3g5), Black to move
        // So White="Opponent", Black="Player"
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        assert!(pgn.contains("[White \"Opponent\"]"));
        assert!(pgn.contains("[Black \"Player\"]"));
    }

    #[test]
    fn test_side_tags_white_to_move_after_trigger() {
        // Black triggers (Ra8-a7), leaving White to move
        let puzzle = config::Puzzle {
            puzzle_id: "test_w".to_string(),
            fen: "r3k3/8/8/8/8/8/8/4K3 b - - 0 1".to_string(),
            moves: "a8a7 e1e2".to_string(),
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        // After Ra7, White to move -> White="Player", Black="Opponent"
        assert!(pgn.contains("[White \"Player\"]"));
        assert!(pgn.contains("[Black \"Opponent\"]"));
        // Move text must start with white move
        let move_text = pgn.split("\n\n").last().unwrap_or("");
        assert!(move_text.starts_with("1. Ke2"), "Move text should start with '1. Ke2', got: {}", move_text);
    }

    // ── FEN correctness ──

    #[test]
    fn test_exported_fen_is_normalized_0_1() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();
        assert!(fen.ends_with(" 0 1"), "FEN must end with ' 0 1': {}", fen);
    }

    #[test]
    fn test_exported_fen_differs_from_original() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();
        assert_ne!(fen, puzzle.fen, "Exported FEN must differ from original puzzle FEN");
    }

    // ── Full fixture validation ──

    #[test]
    fn test_fixture_00010_full_validation() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();

        // Parse FEN
        let fen_line = pgn.lines().find(|l| l.starts_with("[FEN ")).unwrap();
        let fen = fen_line.split('"').nth(1).unwrap();
        let mut board = Board::from_str(fen).unwrap();

        // Verify FEN matches board after trigger
        let original_board = Board::from_str(&puzzle.fen).unwrap();
        let trigger = parse_legal_uci_move(&original_board, "f3g5").unwrap();
        let expected_board = original_board.make_move_new(trigger);
        assert_eq!(board, expected_board);

        // Parse and validate all solution moves
        let move_text = pgn.split("\n\n").last().unwrap_or("");
        let tokens: Vec<&str> = move_text
            .split_whitespace()
            .filter(|t| !t.starts_with(|c: char| c.is_ascii_digit()) && !t.starts_with('*') && *t != "...")
            .collect();

        let all_moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let expected_uci: Vec<&str> = all_moves[1..].to_vec();

        assert_eq!(tokens.len(), expected_uci.len());

        for (san, uci) in tokens.iter().zip(expected_uci.iter()) {
            let mv_san = ChessMove::from_san(&board, san).expect(&format!("Failed to parse SAN: {}", san));
            let mv_uci = ChessMove::from_str(uci).expect(&format!("Failed to parse UCI: {}", uci));
            assert_eq!(mv_san, mv_uci, "SAN/UCI mismatch: {} vs {}", san, uci);
            assert!(board.legal(mv_san));
            board = board.make_move_new(mv_san);
        }

        // Verify move text structure
        assert!(move_text.starts_with("1... "), "Should start with '1...' for Black");
        assert!(move_text.trim_end().ends_with("*"), "Should end with '*'");
    }

    // ── to_pgn file write test ──

    #[test]
    fn test_to_pgn_writes_file_correctly() {
        let puzzles = read_fixture_puzzles();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("cms014_sample_{}.pgn", std::process::id()));

        to_pgn(&puzzles, &lang::Language::English, path.to_str().unwrap().to_string());

        let content = std::fs::read_to_string(&path).expect("PGN file should exist");
        assert!(content.contains("[SetUp \"1\"]"));
        assert!(content.contains("[FEN \""));
        assert!(content.contains("1... ") || content.contains("1. "), "Should have move text");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    // ── Existing tests preserved ──

    #[test]
    fn test_san_check_produces_plus() {
        // Rook on f2 can go to f8 giving check to king on e8
        let board = Board::from_str("4k3/8/8/8/8/8/5R2/4K3 w - - 0 1").unwrap();
        let mv = ChessMove::new(Square::F2, Square::F8, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert!(san.ends_with('+'), "Expected check '+', got: {}", san);
    }

    #[test]
    fn test_san_checkmate_produces_hash() {
        // Back rank mate
        let board = Board::from_str("6k1/5ppp/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mv = ChessMove::new(Square::A1, Square::A8, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert!(san.ends_with('#'), "Expected checkmate '#', got: {}", san);
    }

    #[test]
    fn test_san_pawn_capture_no_promotion() {
        let board = Board::from_str("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2").unwrap();
        let mv = ChessMove::new(Square::E4, Square::D5, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "exd5");
    }

    #[test]
    fn test_san_piece_capture_check() {
        // Bxf7+ in Italian Game
        let board = Board::from_str("r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4").unwrap();
        let mv = ChessMove::new(Square::C4, Square::F7, None);
        let san = move_to_standard_san(&board, mv).unwrap();
        assert_eq!(san, "Bxf7+");
        assert!(san.contains('x'));
        // Roundtrip
        let mv_back = ChessMove::from_san(&board, "Bxf7+").unwrap();
        assert_eq!(mv_back, mv);
    }

    #[test]
    fn generate_sample_pgn_for_review() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_review");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("cms014_chessbase_sample.pgn");

        // Game 1: fixture 00010 (Black to move after trigger)
        let puzzle_00010 = fixture_puzzle_00010();
        let game1 = build_pgn_game(&puzzle_00010, "2026.09.03").unwrap();

        // Game 2: controlled White-to-move puzzle
        let puzzle_white = config::Puzzle {
            puzzle_id: "test_w".to_string(),
            fen: "r3k3/8/8/8/8/8/8/4K3 b - - 0 1".to_string(),
            moves: "a8a7 e1e2".to_string(),
            rating: 1500,
            rating_deviation: 70,
            popularity: 90,
            nb_plays: 5000,
            themes: "king move".to_string(),
            game_url: "https://lichess.org/training/test_w".to_string(),
            opening: String::new(),
        };
        let game2 = build_pgn_game(&puzzle_white, "2026.09.03").unwrap();

        let mut content = game1;
        content.push('\n');
        content.push_str(&game2);
        std::fs::write(&path, &content).expect("Failed to write PGN");

        let read_back = std::fs::read_to_string(&path).expect("PGN file should exist");
        let game_count = read_back.matches("[Event \"Chess Puzzle\"]").count();
        assert_eq!(game_count, 2, "Sample must contain exactly 2 games");
        assert!(read_back.contains("[SetUp \"1\"]"));
        assert!(read_back.contains("[FEN \""));
        assert!(read_back.contains("[Round \"-\"]"));
        // Game 2 must start with 1. Ke2
        assert!(read_back.contains("1. Ke2"));
    }

    // ── CMS-015: PDF diagram tests ──

    #[test]
    fn test_pdf_board_labels_white_at_bottom() {
        let (files, ranks) = pdf_board_labels(true);
        assert_eq!(files, [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(ranks, [7, 6, 5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn test_pdf_board_labels_black_at_bottom() {
        let (files, ranks) = pdf_board_labels(false);
        assert_eq!(files, [7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn test_pdf_side_after_trigger_black_to_move() {
        let puzzle = fixture_puzzle_00010();
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let movement = ChessMove::new(
            Square::from_str(&String::from(&moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(moves[0]),
        );
        board = board.make_move_new(movement);
        assert_eq!(board.side_to_move(), Color::Black);
    }

    #[test]
    fn test_pdf_side_after_trigger_white_to_move() {
        let puzzle = config::Puzzle {
            puzzle_id: "test_w".to_string(),
            fen: "r3k3/8/8/8/8/8/8/4K3 b - - 0 1".to_string(),
            moves: "a8a7 e1e2".to_string(),
            rating: 0,
            rating_deviation: 0,
            popularity: 0,
            nb_plays: 0,
            themes: String::new(),
            game_url: String::new(),
            opening: String::new(),
        };
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let movement = ChessMove::new(
            Square::from_str(&String::from(&moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(moves[0]),
        );
        board = board.make_move_new(movement);
        assert_eq!(board.side_to_move(), Color::White);
    }

    fn extract_tj_texts(ops: &[Operation]) -> Vec<String> {
        ops.iter()
            .filter_map(|op| {
                if op.operator == "Tj" {
                    if let Some(Object::String(s, _)) = op.operands.first() {
                        return String::from_utf8(s.clone()).ok();
                    }
                }
                None
            })
            .collect()
    }

    #[test]
    fn test_pdf_header_no_last_move_text() {
        let puzzle = fixture_puzzle_00010();
        let ops = gen_diagram_operations(1, &puzzle, 750, 75, &lang::Language::English);
        let texts = extract_tj_texts(&ops);
        let all_text = texts.join(" ");
        assert!(!all_text.contains("Ultimo"));
        assert!(!all_text.contains("Last move"));
        assert!(!all_text.contains("last move"));
        assert!(!all_text.contains("Juegan"));
        assert!(!all_text.contains("to move"));
    }

    #[test]
    fn test_pdf_header_contains_number() {
        let puzzle = fixture_puzzle_00010();
        let ops = gen_diagram_operations(42, &puzzle, 750, 75, &lang::Language::English);
        let texts = extract_tj_texts(&ops);
        let all_text = texts.join("");
        assert!(all_text.contains("42"), "Header must contain exercise number '42'");
    }

    #[test]
    fn test_pdf_header_no_trigger_san() {
        let puzzle = fixture_puzzle_00010();
        let ops = gen_diagram_operations(1, &puzzle, 750, 75, &lang::Language::English);
        let texts = extract_tj_texts(&ops);
        let all_text = texts.join(" ");
        let trigger_san = {
            let board = Board::from_str(&puzzle.fen).unwrap();
            let trigger = parse_legal_uci_move(&board, "f3g5").unwrap();
            move_to_standard_san(&board, trigger).unwrap()
        };
        assert!(!all_text.contains(&trigger_san), "Trigger SAN '{}' must not appear in diagram", trigger_san);
    }

    #[test]
    fn test_pdf_side_circle_no_eyes_no_mouth() {
        let puzzle = fixture_puzzle_00010();
        let ops = gen_diagram_operations(1, &puzzle, 750, 75, &lang::Language::English);
        let has_re = ops.iter().any(|op| op.operator == "re");
        assert!(!has_re, "Side circle must not use 're' (rectangle for eyes)");
        let has_stroke = ops.iter().any(|op| op.operator == "S");
        assert!(!has_stroke, "Side circle must not use 'S' (stroke for mouth)");
    }

    #[test]
    fn test_pdf_coordinates_white_at_bottom() {
        let puzzle = config::Puzzle {
            puzzle_id: "coord_w".to_string(),
            fen: "r3k3/8/8/8/8/8/8/4K3 b - - 0 1".to_string(),
            moves: "a8a7 e1e2".to_string(),
            rating: 0, rating_deviation: 0, popularity: 0, nb_plays: 0,
            themes: String::new(), game_url: String::new(), opening: String::new(),
        };
        let ops = gen_diagram_operations(1, &puzzle, 750, 75, &lang::Language::English);
        let texts = extract_tj_texts(&ops);
        let all_text = texts.join("");
        for c in 'a'..='h' {
            assert!(all_text.contains(c), "File label '{}' missing", c);
        }
        for c in '1'..='8' {
            assert!(all_text.contains(c), "Rank label '{}' missing", c);
        }
    }

    #[test]
    fn test_pdf_coordinates_black_at_bottom() {
        let puzzle = fixture_puzzle_00010();
        let ops = gen_diagram_operations(1, &puzzle, 750, 75, &lang::Language::English);
        let texts = extract_tj_texts(&ops);
        let all_text = texts.join("");
        for c in 'a'..='h' {
            assert!(all_text.contains(c), "File label '{}' missing", c);
        }
        for c in '1'..='8' {
            assert!(all_text.contains(c), "Rank label '{}' missing", c);
        }
    }

    // ── CMS-016D: light-square glyph mapping ──

    #[test]
    fn test_chess_alpha_light_square_glyph_mapping() {
        assert_eq!(chess_alpha_light_square_glyph('K'), 'k');
        assert_eq!(chess_alpha_light_square_glyph('Q'), 'q');
        assert_eq!(chess_alpha_light_square_glyph('R'), 'r');
        assert_eq!(chess_alpha_light_square_glyph('B'), 'b');
        assert_eq!(chess_alpha_light_square_glyph('N'), 'h');
        assert_eq!(chess_alpha_light_square_glyph('x'), 'x');
        assert_eq!(chess_alpha_light_square_glyph('e'), 'e');
    }

    // ── CMS-016/016B: figurine span tests ──

    #[test]
    fn test_san_to_spans_rook() {
        let spans = standard_san_to_pdf_spans("Rb1+");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "r".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "b1+".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_queen_capture() {
        let spans = standard_san_to_pdf_spans("Qxe7+");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "q".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "xe7+".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_knight() {
        let spans = standard_san_to_pdf_spans("Nxf7");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "h".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "xf7".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_bishop() {
        let spans = standard_san_to_pdf_spans("Bf5");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "b".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "f5".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_king() {
        let spans = standard_san_to_pdf_spans("Kd2");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "k".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "d2".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_pawn() {
        let spans = standard_san_to_pdf_spans("e4");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e4".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_promotion() {
        let spans = standard_san_to_pdf_spans("e1=Q");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e1=".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "q".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_promotion_knight() {
        let spans = standard_san_to_pdf_spans("e1=N+");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e1=".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "h".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "+".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_castling_kingside() {
        let spans = standard_san_to_pdf_spans("O-O");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "O-O".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_castling_queenside() {
        let spans = standard_san_to_pdf_spans("O-O-O");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "O-O-O".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_disambiguation_file() {
        let spans = standard_san_to_pdf_spans("Nbd2");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "h".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "bd2".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_disambiguation_rank() {
        let spans = standard_san_to_pdf_spans("R1a2");
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "r".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "1a2".into() },
        ]);
    }

    #[test]
    fn test_san_to_spans_pgn_unchanged() {
        let puzzle = fixture_puzzle_00010();
        let pgn = build_pgn_game(&puzzle, "2026.09.03").unwrap();
        assert!(pgn.contains("Nf3") || pgn.contains("e6") || pgn.contains("Ng5"),
            "PGN must contain standard SAN letters, not figurines");
        assert!(!pgn.contains("H"), "PGN must not contain Chess Alpha knight glyph");
    }

    // ── CMS-016B: UCI → span tests ──

    #[test]
    fn test_uci_to_spans_rook() {
        let board = Board::from_str("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "a1b1").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "r".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "b1".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_queen_capture_checkmate() {
        let board = Board::from_str("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "h5f7").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "q".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "xf7#".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_knight() {
        let board = Board::default();
        let spans = uci_move_to_pdf_spans(&board, "g1f3").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "h".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "f3".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_bishop() {
        let board = Board::from_str("r1bqkbnr/pppppppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "f1c4").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "b".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "c4".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_king() {
        let board = Board::from_str("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "e1e2").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "k".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e2".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_pawn() {
        let board = Board::default();
        let spans = uci_move_to_pdf_spans(&board, "e2e4").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e4".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_promotion() {
        let board = Board::from_str("8/4P3/8/8/8/8/8/4K2k w - - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "e7e8q").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "e8=".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "q".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_castling_kingside() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "e1g1").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "O-O".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_castling_queenside() {
        let board = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "e1c1").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "O-O-O".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_disambiguation_file() {
        let board = Board::from_str("4k3/8/8/8/8/8/R1R5/4K3 w - - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "a2b2").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "r".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "ab2".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_disambiguation_rank() {
        let board = Board::from_str("4k3/8/8/8/8/R7/8/R3K3 w - - 0 1").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "a1a2").unwrap();
        assert_eq!(spans, vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "r".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "1a2".into() },
        ]);
    }

    #[test]
    fn test_uci_to_spans_language_independent() {
        let board = Board::default();
        let spans_en = uci_move_to_pdf_spans(&board, "e2e4").unwrap();
        let spans_es = uci_move_to_pdf_spans(&board, "e2e4").unwrap();
        assert_eq!(spans_en, spans_es, "Spans must be identical regardless of language");
    }

    #[test]
    fn test_uci_to_spans_sequence_matches_ucis() {
        let puzzle = fixture_puzzle_00010();
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let trigger = ChessMove::new(
            Square::from_str(&String::from(&moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(moves[0]),
        );
        board = board.make_move_new(trigger);
        for uci in &moves[1..] {
            let spans = uci_move_to_pdf_spans(&board, uci).unwrap();
            let cm = parse_legal_uci_move(&board, uci).unwrap();
            let expected_san = move_to_standard_san(&board, cm).unwrap();
            let expected_spans = standard_san_to_pdf_spans(&expected_san);
            assert_eq!(spans, expected_spans, "Span mismatch for UCI {}", uci);
            board = board.make_move_new(cm);
        }
    }

    #[test]
    fn test_uci_to_spans_real_fixture_00010() {
        let puzzle = fixture_puzzle_00010();
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let moves: Vec<&str> = puzzle.moves.split_whitespace().collect();
        let trigger = ChessMove::new(
            Square::from_str(&String::from(&moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(moves[0]),
        );
        board = board.make_move_new(trigger);
        for uci in &moves[1..] {
            let spans = uci_move_to_pdf_spans(&board, uci).unwrap();
            assert!(!spans.is_empty(), "UCI {} produced empty spans", uci);
            let cm = parse_legal_uci_move(&board, uci).unwrap();
            let san = move_to_standard_san(&board, cm).unwrap();
            // Verify total text reconstructs the SAN (with h for knight light-square glyph)
            let total: String = spans.iter().map(|s| s.text.as_str()).collect();
            let expected_total = san.replace('N', "h");
            assert_eq!(total, expected_total, "Span text must match standard SAN with h for knight");
            // Verify figurine spans exist for piece moves
            if san.starts_with(|c: char| "KQRBN".contains(c)) {
                assert!(spans.iter().any(|s| s.font == PdfSolutionFont::Figurine),
                    "Piece move {} must have figurine span", san);
            }
            board = board.make_move_new(cm);
        }
    }

    #[test]
    fn test_pdf_solution_spans_produce_mixed_fonts() {
        let board = Board::from_str("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
        let spans = uci_move_to_pdf_spans(&board, "h5f7").unwrap();
        let mut ops: Vec<Operation> = vec![];
        ops.push(Operation::new("BT", vec![]));
        ops.push(Operation::new("Td", vec![75.into(), 800.into()]));
        append_pdf_solution_spans(&mut ops, &spans);
        ops.push(Operation::new("ET", vec![]));

        let has_chess_alpha = ops.iter().any(|op| {
            op.operator == "Tf" && op.operands.first() == Some(&"Chess Alpha".into())
        });
        let has_regular = ops.iter().any(|op| {
            op.operator == "Tf" && op.operands.first() == Some(&"Regular".into())
        });
        assert!(has_chess_alpha, "Solution must use Chess Alpha font for figurine");
        assert!(has_regular, "Solution must use Regular font for non-figurine text");
    }

    #[test]
    fn test_append_solution_spans_text_rise() {
        let spans = vec![
            PdfSolutionSpan { font: PdfSolutionFont::Figurine, text: "q".into() },
            PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "xf7#".into() },
        ];
        let mut ops: Vec<Operation> = vec![];
        append_pdf_solution_spans(&mut ops, &spans);

        // Find Ts operations and their values in order
        let ts_values: Vec<i32> = ops.iter()
            .filter(|op| op.operator == "Ts")
            .filter_map(|op| op.operands.first().and_then(|v| match v {
                Object::Integer(n) => Some(*n as i32),
                _ => None,
            }))
            .collect();

        // Figurine gets Ts -2, then Regular gets Ts 0, then trailing Ts 0
        assert!(ts_values.windows(2).any(|w| w[0] == -1 && w[1] == 0),
            "Must have Ts -1 (figurine) followed by Ts 0 (regular), got: {:?}", ts_values);

        // Verify the structure: Tf, Ts, Tj for each span, then final Ts 0
        let tf_count = ops.iter().filter(|op| op.operator == "Tf").count();
        let tj_count = ops.iter().filter(|op| op.operator == "Tj").count();
        let ts_count = ops.iter().filter(|op| op.operator == "Ts").count();
        assert_eq!(tf_count, 2, "Must have 2 Tf ops");
        assert_eq!(tj_count, 2, "Must have 2 Tj ops");
        assert_eq!(ts_count, 3, "Must have 3 Ts ops (one per span + trailing reset)");
    }

    // ── CMS-016C: exercise number prefix ──

    #[test]
    fn test_solution_starts_with_exercise_number() {
        let puzzle = fixture_puzzle_00010();
        let mut board = Board::from_str(&puzzle.fen).unwrap();
        let mut moves: VecDeque<&str> = puzzle.moves.split_whitespace().collect();
        let trigger = ChessMove::new(
            Square::from_str(&String::from(&moves[0][..2])).unwrap(),
            Square::from_str(&String::from(&moves[0][2..4])).unwrap(),
            PuzzleTab::check_promotion(moves[0]),
        );
        board = board.make_move_new(trigger);
        moves.pop_front();

        let puzzle_number: usize = 0;
        let mut move_spans: Vec<PdfSolutionSpan> = Vec::new();
        move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: format!("{})", puzzle_number + 1) });
        if board.side_to_move() == Color::Black {
            move_spans.push(PdfSolutionSpan { font: PdfSolutionFont::Regular, text: " 1. ... ".to_string() });
        }
        for chess_move in &moves {
            let spans = uci_move_to_pdf_spans(&board, chess_move).unwrap();
            move_spans.extend(spans);
            let movement = ChessMove::new(
                Square::from_str(&String::from(&chess_move[..2])).unwrap(),
                Square::from_str(&String::from(&chess_move[2..4])).unwrap(),
                PuzzleTab::check_promotion(chess_move));
            board = board.make_move_new(movement);
        }

        assert_eq!(move_spans[0], PdfSolutionSpan { font: PdfSolutionFont::Regular, text: "1)".into() });
        assert!(move_spans.len() > 1, "Must have moves after exercise number");
        let total: String = move_spans.iter().map(|s| s.text.as_str()).collect();
        assert!(total.starts_with("1) 1. ..."), "Must start with '1) 1. ...', got: {}", total);
    }

    #[test]
    fn test_solution_exercise_number_42() {
        let puzzle_number: usize = 41;
        let prefix = format!("{})", puzzle_number + 1);
        assert_eq!(prefix, "42)");
    }
}
