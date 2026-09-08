CREATE TABLE chapters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    position INTEGER NOT NULL CHECK (position > 0),
    target_puzzle_count INTEGER NULL CHECK (
        target_puzzle_count IS NULL OR target_puzzle_count > 0
    ),
    created_at TEXT NOT NULL,
    UNIQUE (position)
);
