CREATE TABLE chapter_puzzle_reviews (
    chapter_id INTEGER NOT NULL,
    puzzle_id TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('selected', 'discarded')),
    fen TEXT NOT NULL,
    moves TEXT NOT NULL,
    rating INTEGER NOT NULL,
    rating_deviation INTEGER NOT NULL,
    popularity INTEGER NOT NULL,
    nb_plays INTEGER NOT NULL,
    themes TEXT NOT NULL,
    game_url TEXT NOT NULL,
    opening_tags TEXT NOT NULL,
    reviewed_at TEXT NOT NULL,
    PRIMARY KEY (chapter_id, puzzle_id)
);

CREATE UNIQUE INDEX chapter_puzzle_reviews_selected_puzzle_id_unique
    ON chapter_puzzle_reviews (puzzle_id)
    WHERE decision = 'selected';