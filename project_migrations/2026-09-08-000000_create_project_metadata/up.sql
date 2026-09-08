CREATE TABLE project_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    application_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1),
    project_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);
