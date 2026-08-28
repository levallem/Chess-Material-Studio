CREATE TABLE puzzles (
    puzzle_id TEXT NOT NULL PRIMARY KEY,
    fen TEXT NOT NULL,
    moves TEXT NOT NULL,
    rating INTEGER NOT NULL,
    rating_deviation INTEGER NOT NULL,
    popularity INTEGER NOT NULL,
    nb_plays INTEGER NOT NULL,
    themes TEXT NOT NULL,
    game_url TEXT NOT NULL,
    opening_tags TEXT NOT NULL
);

CREATE INDEX idx_puzzles_rating ON puzzles(rating);
CREATE INDEX idx_puzzles_popularity ON puzzles(popularity);
CREATE INDEX idx_puzzles_nb_plays ON puzzles(nb_plays);
CREATE INDEX idx_puzzles_rating_deviation ON puzzles(rating_deviation);
