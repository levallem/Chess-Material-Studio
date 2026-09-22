CREATE TABLE chapter_pgn_positions (
    id INTEGER PRIMARY KEY,
    chapter_id INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    source_game_index INTEGER NOT NULL CHECK (source_game_index >= 0),
    ply_index INTEGER NOT NULL CHECK (ply_index >= 0),
    selected_fen TEXT NOT NULL,
    initial_fen TEXT NOT NULL,
    main_line_uci TEXT NOT NULL,
    header_event TEXT NULL,
    header_site TEXT NULL,
    header_date TEXT NULL,
    header_round TEXT NULL,
    header_white TEXT NULL,
    header_black TEXT NULL,
    header_result TEXT NULL,
    header_set_up TEXT NULL,
    header_fen TEXT NULL,
    UNIQUE (chapter_id, initial_fen, main_line_uci, ply_index)
);

CREATE INDEX chapter_pgn_positions_chapter_id_id
    ON chapter_pgn_positions (chapter_id, id);
