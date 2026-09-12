use iced::Subscription;
use iced::futures::Stream;
use iced::futures::sink::SinkExt;
use iced::stream;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::PUZZLES_DIRECTORY;
use crate::{LICHESS_DB_URL, Message, config};

pub fn download_lichess_db() -> Subscription<Message> {
    Subscription::run(download_stream)
}

fn download_stream() -> impl Stream<Item = Message> {
    let url = String::from(LICHESS_DB_URL);
    let destination = PathBuf::from(config::SETTINGS.puzzle_db_location.clone());
    let archive = PathBuf::from(PUZZLES_DIRECTORY).join("lichess_db_puzzle.csv.zst");

    stream::channel(100, async move |mut output| {
        let response = match reqwest::get(&url).await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => response,
                Err(error) => {
                    let _ = output
                        .send(Message::DBDownloadFailed(format!(
                            "HTTP request failed: {error}"
                        )))
                        .await;
                    return;
                }
            },
            Err(error) => {
                let _ = output
                    .send(Message::DBDownloadFailed(format!(
                        "Network request failed: {error}"
                    )))
                    .await;
                return;
            }
        };

        let total = response.content_length();
        let mut response = response;
        let mut archive_file = match prepare_compressed_file(&archive) {
            Ok(file) => file,
            Err(error) => {
                let _ = output.send(Message::DBDownloadFailed(error)).await;
                return;
            }
        };
        let mut downloaded = 0_u64;

        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if let Err(error) = archive_file.write_all(&chunk) {
                        let _ = output
                            .send(Message::DBDownloadFailed(format!(
                                "Failed to write compressed puzzle database {}: {error}",
                                archive.display()
                            )))
                            .await;
                        return;
                    }

                    downloaded += chunk.len() as u64;
                    let progress = match total {
                        Some(total) if total > 0 => {
                            format!(" {:.2}%", (downloaded as f32 / total as f32) * 100.0)
                        }
                        _ => format!(" {downloaded} bytes"),
                    };
                    if output
                        .send(Message::DownloadProgress(progress))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {
                    if let Err(error) = archive_file.flush() {
                        let _ = output
                            .send(Message::DBDownloadFailed(format!(
                                "Failed to flush compressed puzzle database {}: {error}",
                                archive.display()
                            )))
                            .await;
                        return;
                    }
                    drop(archive_file);

                    match decompress_and_publish(&archive, &destination) {
                        Ok(()) => {
                            let _ = fs::remove_file(&archive);
                            let _ = output.send(Message::DBDownloadFinished).await;
                        }
                        Err(error) => {
                            let _ = output.send(Message::DBDownloadFailed(error)).await;
                        }
                    }
                    return;
                }
                Err(error) => {
                    let _ = output
                        .send(Message::DBDownloadFailed(format!(
                            "Failed while downloading puzzle database: {error}"
                        )))
                        .await;
                    return;
                }
            }
        }
    })
}

pub(crate) fn prepare_compressed_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| {
            format!(
                "Failed to create compressed puzzle database {}: {error}",
                path.display()
            )
        })
}

pub(crate) fn decompress_and_publish(archive: &Path, destination: &Path) -> Result<(), String> {
    let temporary = temporary_csv_path(destination)?;
    let result = (|| {
        let source = File::open(archive).map_err(|error| {
            format!(
                "Failed to open compressed puzzle database {}: {error}",
                archive.display()
            )
        })?;
        let mut temporary_file = File::create(&temporary).map_err(|error| {
            format!(
                "Failed to create temporary puzzle database {}: {error}",
                temporary.display()
            )
        })?;

        zstd::stream::copy_decode(source, &mut temporary_file).map_err(|error| {
            format!(
                "Failed to decompress puzzle database into {}: {error}",
                temporary.display()
            )
        })?;
        temporary_file.flush().map_err(|error| {
            format!(
                "Failed to flush temporary puzzle database {}: {error}",
                temporary.display()
            )
        })?;
        drop(temporary_file);

        publish_temporary_csv(&temporary, destination)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn publish_temporary_csv(temporary: &Path, destination: &Path) -> Result<(), String> {
    publish_temporary_csv_with_renamer(temporary, destination, |from, to| fs::rename(from, to))
}

fn publish_temporary_csv_with_renamer<F>(
    temporary: &Path,
    destination: &Path,
    rename: F,
) -> Result<(), String>
where
    F: Fn(&Path, &Path) -> std::io::Result<()>,
{
    if !destination.exists() {
        return rename(temporary, destination).map_err(|error| {
            format!(
                "Failed to publish puzzle database {}: {error}",
                destination.display()
            )
        });
    }

    let backup = backup_csv_path(destination)?;
    if backup.exists() {
        return Err(format!(
            "Cannot replace puzzle database {} because backup {} already exists",
            destination.display(),
            backup.display()
        ));
    }
    rename(destination, &backup).map_err(|error| {
        format!(
            "Failed to preserve existing puzzle database {}: {error}",
            destination.display()
        )
    })?;

    match rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => match rename(&backup, destination) {
            Ok(()) => Err(format!(
                "Failed to publish puzzle database {}: {error}; previous database restored",
                destination.display()
            )),
            Err(restore_error) => Err(format!(
                "Failed to publish puzzle database {}: {error}; previous database remains at {} because restoration failed: {restore_error}",
                destination.display(),
                backup.display()
            )),
        },
    }
}

fn temporary_csv_path(destination: &Path) -> Result<PathBuf, String> {
    let file_name = destination.file_name().ok_or_else(|| {
        format!(
            "Puzzle database destination has no file name: {}",
            destination.display()
        )
    })?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    Ok(destination.with_file_name(temporary_name))
}

fn backup_csv_path(destination: &Path) -> Result<PathBuf, String> {
    let file_name = destination.file_name().ok_or_else(|| {
        format!(
            "Puzzle database destination has no file name: {}",
            destination.display()
        )
    })?;
    let mut backup_name = file_name.to_os_string();
    backup_name.push(".backup");
    Ok(destination.with_file_name(backup_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("download_db_tests")
                .join(format!("{label}-{}-{sequence}", std::process::id()));
            fs::create_dir_all(&directory).expect("test directory should be created");
            Self(directory)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn preparation_truncates_previous_compressed_download() {
        let directory = TestDirectory::new("truncate");
        let archive = directory.0.join("puzzles.csv.zst");
        fs::write(&archive, b"partial previous attempt").expect("old archive should be written");

        let mut file = prepare_compressed_file(&archive).expect("archive should be recreated");
        file.write_all(b"new")
            .expect("new archive contents should be written");
        drop(file);

        assert_eq!(
            fs::read(&archive).expect("archive should be readable"),
            b"new"
        );
    }

    #[test]
    fn invalid_zstd_does_not_publish_a_partial_csv() {
        let directory = TestDirectory::new("invalid-zstd");
        let archive = directory.0.join("puzzles.csv.zst");
        let destination = directory.0.join("puzzles.csv");
        fs::write(&archive, b"not zstd").expect("invalid archive should be written");
        fs::write(&destination, b"previous csv").expect("previous destination should be written");

        assert!(decompress_and_publish(&archive, &destination).is_err());
        assert_eq!(
            fs::read(&destination).expect("previous destination should remain readable"),
            b"previous csv"
        );
        assert!(
            !temporary_csv_path(&destination)
                .expect("temporary path should be derived")
                .exists()
        );
    }

    #[test]
    fn valid_zstd_is_published_only_after_successful_decompression() {
        let directory = TestDirectory::new("valid-zstd");
        let archive = directory.0.join("puzzles.csv.zst");
        let destination = directory.0.join("puzzles.csv");
        let expected = b"PuzzleId,FEN\npuzzle-1,8/8/8/8/8/8/8/K6k w - - 0 1\n";
        let compressed =
            zstd::stream::encode_all(Cursor::new(expected), 0).expect("test CSV should compress");
        fs::write(&archive, compressed).expect("compressed archive should be written");

        decompress_and_publish(&archive, &destination).expect("CSV should be published");
        let mut actual = Vec::new();
        File::open(&destination)
            .expect("published CSV should open")
            .read_to_end(&mut actual)
            .expect("published CSV should read");
        assert_eq!(actual, expected);
        assert!(
            !temporary_csv_path(&destination)
                .expect("temporary path should be derived")
                .exists()
        );
    }

    #[test]
    fn publish_failure_restores_the_previous_csv() {
        let directory = TestDirectory::new("publish-failure");
        let temporary = directory.0.join("puzzles.csv.tmp");
        let destination = directory.0.join("puzzles.csv");
        fs::write(&temporary, b"new csv").expect("temporary CSV should be written");
        fs::write(&destination, b"previous csv").expect("previous CSV should be written");

        let result = publish_temporary_csv_with_renamer(&temporary, &destination, |from, to| {
            if from == temporary && to == destination {
                return Err(std::io::Error::other("simulated publication failure"));
            }
            fs::rename(from, to)
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read(&destination).expect("previous CSV should be restored"),
            b"previous csv"
        );
        assert!(
            !backup_csv_path(&destination)
                .expect("backup path should be derived")
                .exists()
        );
    }
}
