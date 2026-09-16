# Contributing to Chess Material Studio

Thanks for helping improve Chess Material Studio. Contributions should prioritize reliability, data integrity, small reviewable changes, appropriate tests, and documentation that stays aligned with the actual behavior.

## Set up the project

This repository fixes its development toolchain to Rust 1.98.1 in [`rust-toolchain.toml`](rust-toolchain.toml). That selects the toolchain used by this project; it is not a declaration of a minimum supported Rust version.

Clone the repository and enter it:

```bash
git clone https://github.com/levallem/Chess-Material-Studio.git
cd Chess-Material-Studio
```

On Linux, install the development packages used by CI:

```bash
sudo apt-get install libasound2-dev libgtk-3-dev libsqlite3-dev
```

CI also validates builds on macOS and 64-bit and 32-bit Windows. No additional platform-specific setup is documented here.

## Run the application

```bash
cargo run --locked --release --bin chess-material-studio
```

## Run the quality gates

Run the relevant checks before opening a pull request. Pull requests should keep these checks green.

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo check --locked --all-targets
cargo build --locked --release --bin chess-material-studio
git diff --check
```

- `cargo fmt --check` verifies formatting.
- `cargo clippy` treats warnings as errors across targets.
- `cargo test` runs the test suite.
- `cargo check` validates configured targets without a release build.
- `cargo build` verifies the release application binary.
- `git diff --check` detects whitespace errors in the proposed change.

## Make focused changes

- Keep the scope small and avoid unrelated changes.
- Separate broad refactors from features or fixes when practical.
- Add or update tests when behavior changes.
- Keep documentation synchronized with behavior.
- Explain schema or migration changes when they are necessary.
- Do not change persistence or data formats accidentally.

## Local data and SQLite files

Do not accidentally version local or complete runtime data, including `settings.json`, complete local puzzle databases, personal project `.cms.sqlite` files, local `ocp.db`, or other user runtime data.

This does not prohibit small, purpose-built test fixtures. Chess Material Studio distinguishes the optional puzzle-corpus SQLite database, editorial project `.cms.sqlite` files, and the Favorites `ocp.db`; do not conflate their roles.

## Assets and licenses

Any new asset must have a verifiable origin, a license compatible with its use and distribution, and any required attribution or notice. See [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for the current bundled-asset notices. Do not assume every asset is covered by the source-code MIT license.

## Database changes and migrations

Changes under [`migrations/`](migrations/) or [`project_migrations/`](project_migrations/) need a clear reason and appropriate tests. Document the impact without inventing a migration process that the project does not have.

## Pull requests

Use a clear summary and explain the motivation. Include the tests or validations you ran, screenshots when a visual change makes them useful, and any schema or migration changes. Keep the proposed change easy to review.
