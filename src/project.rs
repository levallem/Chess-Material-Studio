use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::path::Path;

pub const PROJECT_APPLICATION_ID: &str = "chess-material-studio-project";
pub const PROJECT_SCHEMA_VERSION: i32 = 1;
pub const PROJECT_MIGRATIONS: EmbeddedMigrations = embed_migrations!("project_migrations");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMetadata {
    pub project_name: String,
    pub schema_version: i32,
    pub created_at: String,
}

#[derive(QueryableByName)]
struct ProjectMetadataRow {
    #[diesel(sql_type = Text)]
    application_id: String,
    #[diesel(sql_type = Integer)]
    schema_version: i32,
    #[diesel(sql_type = Text)]
    project_name: String,
    #[diesel(sql_type = Text)]
    created_at: String,
}

pub fn create_project(path: &Path, project_name: &str) -> Result<ProjectMetadata, String> {
    if project_name.trim().is_empty() {
        return Err("project name cannot be empty".into());
    }

    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err("project file already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect project path: {error}")),
    }

    let path_string = project_path_string(path)?;
    let result = (|| {
        let mut connection = SqliteConnection::establish(&path_string)
            .map_err(|error| format!("cannot create SQLite project: {error}"))?;
        connection
            .run_pending_migrations(PROJECT_MIGRATIONS)
            .map_err(|error| format!("cannot run project migrations: {error}"))?;

        let metadata = ProjectMetadata {
            project_name: project_name.to_owned(),
            schema_version: PROJECT_SCHEMA_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        diesel::sql_query(
            "INSERT INTO project_metadata \
             (id, application_id, schema_version, project_name, created_at) \
             VALUES (1, ?, ?, ?, ?)",
        )
        .bind::<Text, _>(PROJECT_APPLICATION_ID)
        .bind::<Integer, _>(metadata.schema_version)
        .bind::<Text, _>(&metadata.project_name)
        .bind::<Text, _>(&metadata.created_at)
        .execute(&mut connection)
        .map_err(|error| format!("cannot write project metadata: {error}"))?;

        Ok(metadata)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }

    result
}

pub fn open_project(path: &Path) -> Result<ProjectMetadata, String> {
    let file_metadata =
        std::fs::metadata(path).map_err(|error| format!("project file cannot be read: {error}"))?;
    if !file_metadata.is_file() {
        return Err("project path is not a regular file".into());
    }

    let path_string = project_path_string(path)?;
    let mut connection = SqliteConnection::establish(&path_string)
        .map_err(|error| format!("cannot open SQLite project: {error}"))?;
    let rows = diesel::sql_query(
        "SELECT application_id, schema_version, project_name, created_at \
         FROM project_metadata WHERE id = 1",
    )
    .load::<ProjectMetadataRow>(&mut connection)
    .map_err(|error| format!("cannot read project metadata: {error}"))?;

    let [row] = rows.as_slice() else {
        return Err("project metadata is missing or invalid".into());
    };
    if row.application_id != PROJECT_APPLICATION_ID {
        return Err("SQLite file is not a Chess Material Studio project".into());
    }
    if row.schema_version != PROJECT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported project schema version: {}",
            row.schema_version
        ));
    }
    if row.project_name.trim().is_empty() || row.created_at.trim().is_empty() {
        return Err("project metadata is missing required values".into());
    }

    Ok(ProjectMetadata {
        project_name: row.project_name.clone(),
        schema_version: row.schema_version,
        created_at: row.created_at.clone(),
    })
}

fn project_path_string(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "project path is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::Connection;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempProjectDb {
        path: std::path::PathBuf,
        directory: std::path::PathBuf,
    }

    impl TempProjectDb {
        fn new(label: &str) -> Self {
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("cms_023a_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("test directory should be created");
            Self {
                path: directory.join("project.cms.sqlite"),
                directory,
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempProjectDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(&self.directory);
        }
    }

    #[derive(QueryableByName)]
    struct TableName {
        #[diesel(sql_type = Text)]
        name: String,
    }

    #[derive(QueryableByName)]
    struct MigrationVersion {
        #[diesel(sql_type = Text)]
        version: String,
    }

    #[test]
    fn creating_a_project_creates_a_valid_sqlite_project() {
        let project = TempProjectDb::new("create");
        let metadata =
            create_project(project.path(), "Libro táctico").expect("project should be created");

        assert!(project.path().is_file());
        assert_eq!(metadata.project_name, "Libro táctico");
        assert_eq!(metadata.schema_version, PROJECT_SCHEMA_VERSION);
        assert_eq!(open_project(project.path()).unwrap(), metadata);
    }

    #[test]
    fn reopening_a_project_returns_the_same_name() {
        let project = TempProjectDb::new("reopen");
        create_project(project.path(), "Patrones de ataque").unwrap();

        assert_eq!(
            open_project(project.path()).unwrap().project_name,
            "Patrones de ataque"
        );
    }

    #[test]
    fn project_schema_version_starts_at_one() {
        let project = TempProjectDb::new("schema-version");

        assert_eq!(
            create_project(project.path(), "Versión inicial")
                .unwrap()
                .schema_version,
            1
        );
    }

    #[test]
    fn opening_rejects_an_invalid_application_identifier() {
        let project = TempProjectDb::new("invalid-identifier");
        create_project(project.path(), "Identificador").unwrap();
        {
            let mut connection =
                SqliteConnection::establish(project.path().to_str().unwrap()).unwrap();
            diesel::sql_query("UPDATE project_metadata SET application_id = 'other-app'")
                .execute(&mut connection)
                .unwrap();
        }

        assert!(open_project(project.path()).is_err());
    }

    #[test]
    fn creating_rejects_empty_or_whitespace_only_project_names() {
        for name in ["", "   ", "\t\n"] {
            let project = TempProjectDb::new("empty-name");
            assert!(create_project(project.path(), name).is_err());
            assert!(!project.path().exists());
        }
    }

    #[test]
    fn creating_rejects_an_existing_file_without_overwriting_it() {
        let project = TempProjectDb::new("existing-file");
        std::fs::write(project.path(), b"existing content").unwrap();

        assert!(create_project(project.path(), "No reemplazar").is_err());
        assert_eq!(std::fs::read(project.path()).unwrap(), b"existing content");
    }

    #[test]
    fn opening_rejects_a_nonexistent_file() {
        let project = TempProjectDb::new("missing-file");

        assert!(open_project(project.path()).is_err());
    }

    #[test]
    fn opening_rejects_an_unrelated_sqlite_database() {
        let project = TempProjectDb::new("unrelated-sqlite");
        {
            let mut connection =
                SqliteConnection::establish(project.path().to_str().unwrap()).unwrap();
            diesel::sql_query("CREATE TABLE unrelated_data (value TEXT NOT NULL)")
                .execute(&mut connection)
                .unwrap();
        }

        assert!(open_project(project.path()).is_err());
    }

    #[test]
    fn creating_a_project_does_not_depend_on_ocp_db() {
        let project = TempProjectDb::new("no-ocp-db");
        let ocp_db = project.directory.join("ocp.db");
        assert!(!ocp_db.exists());

        create_project(project.path(), "Independiente").unwrap();

        assert!(!ocp_db.exists());
        assert!(open_project(project.path()).is_ok());
    }

    #[test]
    fn project_migrations_are_isolated_from_normal_migrations() {
        let project = TempProjectDb::new("migration-isolation");
        create_project(project.path(), "Migraciones aisladas").unwrap();
        let mut connection = SqliteConnection::establish(project.path().to_str().unwrap()).unwrap();

        let tables: HashSet<String> =
            diesel::sql_query("SELECT name FROM sqlite_master WHERE type = 'table'")
                .load::<TableName>(&mut connection)
                .unwrap()
                .into_iter()
                .map(|table| table.name)
                .collect();
        assert!(tables.contains("project_metadata"));
        assert!(!tables.contains("favs"));
        assert!(!tables.contains("puzzles"));
        assert!(!tables.contains("puzzle_import_progress"));

        if tables.contains("__diesel_schema_migrations") {
            let migration_versions: HashSet<String> =
                diesel::sql_query("SELECT version FROM __diesel_schema_migrations")
                    .load::<MigrationVersion>(&mut connection)
                    .unwrap()
                    .into_iter()
                    .map(|migration| migration.version)
                    .collect();

            assert!(
                migration_versions.contains("20260908000000"),
                "versions: {migration_versions:?}"
            );
            for normal_migration in ["20230511035750", "20260827000000", "20260831000000"] {
                assert!(!migration_versions.contains(normal_migration));
            }
        }
    }
}
