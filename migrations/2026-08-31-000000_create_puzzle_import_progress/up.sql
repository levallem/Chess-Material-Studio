CREATE TABLE puzzle_import_progress (
    source_key TEXT PRIMARY KEY NOT NULL,
    completed_rows BIGINT NOT NULL
);
