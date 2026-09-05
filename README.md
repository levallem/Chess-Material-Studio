# Chess Material Studio

Chess Material Studio is a desktop application for searching, solving, analyzing, and exporting chess puzzles for training and coaching. It currently supports the [Lichess Puzzle Database](https://database.lichess.org/#puzzles) as its primary puzzle source.

## What is Chess Material Studio?

Chess Material Studio helps chess players and coaches work with a local puzzle collection. You can filter puzzles, solve them on an interactive board, explore positions in analysis mode, save favorites, and prepare training material as PDF worksheets, PGN files, or JPEG board images.

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

## Puzzle data

### Lichess CSV

Chess Material Studio can download and extract the [Lichess Puzzle Database](https://database.lichess.org/#puzzles) for local use. The GUI searches the downloaded CSV directly.

The default expected location is:

```text
puzzles/lichess_db_puzzle.csv
```

If automatic download is not suitable, download and extract the Lichess puzzle CSV manually, then place `lichess_db_puzzle.csv` at that path. The complete Lichess puzzle database is not included in this repository.

### Advanced SQLite workflow

SQLite-backed puzzle searching is available as an advanced optional workflow. The puzzle source is configured through `puzzle_sqlite_location` in `settings.json`; the current GUI does not provide a standard file picker for selecting that database.

The puzzle database is separate from `ocp.db`. `ocp.db` is the application's favorites database; it is not the puzzle corpus.

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

## Project history and upstream

Chess Material Studio is an independent project derived from [offline-chess-puzzles](https://github.com/brianch/offline-chess-puzzles), originally created by [brianch](https://github.com/brianch). The project retains the original attribution and MIT license while continuing under its current name and scope.

## Credits

- [Lichess](https://lichess.org/) for creating and publishing the [Lichess Puzzle Database](https://database.lichess.org/#puzzles), which Chess Material Studio supports as its primary puzzle source.
- [chess-engine](https://github.com/adam-mcdaniel/chess-engine/) for serving as a starting point for the original GUI work.
- The [Iced](https://github.com/iced-rs/iced) project, which provides the GUI framework used by the application.
- The creators of the bundled chess piece sets and fonts listed below.

## License and third-party assets

The source code is distributed under the [MIT License](LICENSE), preserving the existing upstream copyright and license notice.

Third-party fonts, chess piece sets, and other assets may use separate licenses or usage terms and are not covered by a blanket MIT claim. The repository currently records these credits:

- **cburnett** — created by Colin M. L. Burnett and provided under CC BY-SA 3.0 Unported; see `pieces/cburnett/license.txt`.
- **California** — created by Jerry S.; currently attributed as CC BY-NC-SA 4.0.
- **Cardinal, Dubrovny, Gioco, Icpieces, Maestro, Staunty, Governor, and Tatiana** — created by sadsnake1, currently attributed as CC BY-NC-SA 4.0, and obtained from the Lichess/lila project.
- **Chess Alpha** piece set and font — created by Eric Bentzen; the included documentation describes it as free for personal, non-commercial use. See the documents in `font/`.
- **Merida** — the original font was created by Armando Hernandez Marroquin and described as freeware. The shaded version used here was created by Felix Kling and obtained from the Lichess/lila project.

Consult the notices included with individual assets before redistributing them, and do not assume that every asset is licensed under MIT.
