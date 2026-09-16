# Chess Material Studio

Chess Material Studio is a desktop application for working with chess puzzles offline. Use it as a standalone puzzle solver to search, solve, analyze, and export a local collection, or use its optional Projects workflow to prepare persistent editorial material for books, classes, and similar work. It currently supports the [Lichess Puzzle Database](https://database.lichess.org/#puzzles) as its primary puzzle source.

## What is Chess Material Studio?

Chess Material Studio helps chess players and coaches work with a local puzzle collection. Without opening a project, you can filter puzzles, solve them on an interactive board, explore positions in analysis mode, save favorites, and create PDF worksheets, PGN files, or JPEG board images. Projects and Chapters add an optional persistent workflow for reviewing and organizing puzzle material.

## Features

- Search puzzles locally using rating range, minimum popularity, tactical theme, opening, supported variation, opening side, and a configurable result limit.
- Search the Lichess puzzle CSV directly, with optional SQLite-backed searching for advanced setups.
- Solve puzzles interactively with move validation, promotion handling, hints, previous/next navigation, and optional automatic loading of the next puzzle.
- Save favorite puzzles and search within your favorites.
- Control board orientation, flip the board, and configure whether coordinates are shown in the interactive UI.
- Use analysis mode and optionally connect an external UCI-compatible chess engine. Chess Material Studio does not bundle an engine.
- Keep application preferences between sessions, including board theme, piece theme, interface language, coordinates, sound, and training behavior.
- Export puzzle worksheets as PDF with exercise diagrams, side-to-move indicators, board orientation based on the side to move, coordinates, and an optional solution section with figurine notation.
- Export puzzle sets as PGN using standard SAN notation.
- Save the current board as a JPEG image.
- Create or open editorial projects, organize them into Chapters, and optionally set a target puzzle count for each Chapter.
- Review puzzles in an active Chapter as `Selected` or `Discarded`, clear a review decision, and resume the same editorial work after reopening the project.
- Load and export selected editorial puzzles by active Chapter, marked Chapters, or the complete project.

## Editorial workflow

Projects are optional: the normal offline puzzle search and solver work without opening one. To prepare editorial material, create or open a project, create Chapters, choose an active Chapter, and optionally set its target puzzle count. Search the puzzle corpus, then review each puzzle as `Selected` or `Discarded`; a decision can also be cleared.

Each project is stored in its own `.cms.sqlite` file. Reviews persist with a complete snapshot of the relevant puzzle data, rather than only its `PuzzleId`, so `Selected` and `Discarded` work survives closing and reopening a project. The selected puzzles of the active Chapter can be loaded back into the solver in their editorial order.

During normal searches of the puzzle corpus, reviewed puzzles in the active Chapter are excluded from new results. This exclusion does not apply to Favorites searches. A puzzle marked `Selected` cannot be selected simultaneously in another Chapter of the same project; this restriction does not apply to `Discarded` reviews.

### Editorial exports

The Projects tab exports selected material through these routes:

- Active Chapter to PGN.
- Active Chapter to PDF.
- Chapters marked for export to PGN.
- Chapters marked for export to PDF.
- Complete project to PGN.
- Complete project to PDF.

The multiple-Chapter export selection is temporary interface state, not project data stored in the `.cms.sqlite` file. It is cleared when a project is created, opened, or closed.

## Puzzle data

### Lichess CSV

Chess Material Studio can download and extract the [Lichess Puzzle Database](https://database.lichess.org/#puzzles) for local use. The GUI searches the downloaded CSV directly.

The default expected location is:

```text
puzzles/lichess_db_puzzle.csv
```

If automatic download is not suitable, download and extract the Lichess puzzle CSV manually, then place `lichess_db_puzzle.csv` at that path. The complete Lichess puzzle database is not included in this repository.

### Optional SQLite puzzle corpus

SQLite-backed puzzle searching is an optional alternative to the local CSV. In **Settings**, choose a valid puzzle SQLite database with the file picker, or choose the CSV source again. The SQLite corpus is imported from Lichess data and is used as a search source; it is not an editorial project database.

A limited CSV-to-SQLite import can be created with:

```bash
cargo run --locked --release --bin import_puzzles -- \
  --csv puzzles/lichess_db_puzzle.csv \
  --db target/cms_import/puzzles.sqlite \
  --max-rows 100000
```

The importer also provides guarded full-import support and a `--resume` option. Run the following command to inspect the current CLI options before using those advanced operations:

```bash
cargo run --locked --release --bin import_puzzles -- --help
```

### Three separate SQLite uses

Chess Material Studio uses three distinct kinds of local data:

1. **Puzzle corpus SQLite** — an optional SQLite database imported from Lichess and used instead of the CSV for puzzle searches.
2. **Project `.cms.sqlite`** — an independent editorial project file containing its Chapters, reviews, and puzzle snapshots so work can continue later.
3. **`ocp.db`** — the database used exclusively by the existing Favorites system.

Neither a project file nor `ocp.db` replaces the Lichess puzzle corpus, and the optional puzzle corpus does not contain editorial project work.

## Local configuration

`settings.json` is optional local configuration. It is ignored by Git, and if it is absent at startup Chess Material Studio uses built-in default settings. Saving preferences can create or update this local file; it is not required to run the application and should not be versioned. The current release-packaging workflow does not include `settings.json` in its packages.

## Building and running

Install the [Rust toolchain](https://www.rust-lang.org/tools/install), clone this repository, and run the main application explicitly:

```bash
git clone https://github.com/levallem/Chess-Material-Studio.git
cd Chess-Material-Studio
cargo run --locked --release --bin chess-material-studio
```

To build the release executable without running it:

```bash
cargo build --locked --release --bin chess-material-studio
```

To check all configured targets locally:

```bash
cargo check --locked --all-targets
```

The project keeps `Cargo.lock` under version control, so `--locked` is recommended for reproducible dependency resolution.

### Linux

The Ubuntu CI environment installs these development packages before compiling:

```bash
sudo apt-get install libasound2-dev libgtk-3-dev libsqlite3-dev
```

Package names may differ on other Linux distributions. The project is also configured to build on macOS and on 64-bit and 32-bit Windows; no additional platform-specific prerequisites are currently documented for those systems.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, quality gates, and pull request expectations.

## Project history and upstream

Chess Material Studio is an independent project derived from [offline-chess-puzzles](https://github.com/brianch/offline-chess-puzzles), originally created by [brianch](https://github.com/brianch). The project retains the original attribution and MIT license while continuing under its current name and scope.

## Credits

- [Lichess](https://lichess.org/) for creating and publishing the [Lichess Puzzle Database](https://database.lichess.org/#puzzles), which Chess Material Studio supports as its primary puzzle source.
- [chess-engine](https://github.com/adam-mcdaniel/chess-engine/) for serving as a starting point for the original GUI work.
- The [Iced](https://github.com/iced-rs/iced) project, which provides the GUI framework used by the application.
- Colin M. L. Burnett and the Noto Project Authors for the bundled chess piece set and fonts.

## License and third-party assets

The source code is distributed under the [MIT License](LICENSE), preserving the existing upstream copyright and license notice.

Third-party assets are not covered by the source-code MIT license. The bundled assets are the **Cburnett** SVG chess pieces, **Noto Sans**, and **Noto Sans Symbols 2**. Move sounds are synthesized at runtime, so no third-party audio files are shipped. The full Lichess puzzle database is supported as an optional download but is not included.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for authorship, versions, hashes, and license details.
