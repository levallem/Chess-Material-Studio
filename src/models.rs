use crate::schema::favs;
use crate::schema::puzzles;
use diesel::prelude::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, Queryable)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Puzzle {
    #[serde(rename = "PuzzleId")]
    pub puzzle_id: String,
    #[serde(rename = "FEN")]
    pub fen: String,
    #[serde(rename = "Moves")]
    pub moves: String,
    #[serde(rename = "Rating")]
    pub rating: i32,
    #[serde(rename = "RatingDeviation")]
    pub rating_deviation: i32,
    #[serde(rename = "Popularity")]
    pub popularity: i32,
    #[serde(rename = "NbPlays")]
    pub nb_plays: i32,
    #[serde(rename = "Themes")]
    pub themes: String,
    #[serde(rename = "GameUrl")]
    pub game_url: String,
    #[serde(rename = "OpeningTags")]
    #[serde(default)]
    pub opening: String,
}

impl Puzzle {
    pub fn solver_move_count(&self) -> usize {
        self.moves.split_whitespace().count() / 2
    }
}

/*
#[derive(Queryable)]
pub struct Favorite {
    pub puzzle_id: String,
    pub fen: String,
    pub moves: String,
    pub rating: i32,
    pub rd: i32,
    pub popularity: i32,
    pub nb_plays: i32,
    pub themes: String,
    pub game_url: String,
    pub opening_tags: String,
}
*/
#[derive(Insertable)]
#[diesel(table_name = favs)]
pub struct NewFavorite<'a> {
    pub puzzle_id: &'a str,
    pub fen: &'a str,
    pub moves: &'a str,
    pub rating: i32,
    pub rd: i32,
    pub popularity: i32,
    pub nb_plays: i32,
    pub themes: &'a str,
    pub game_url: &'a str,
    pub opening_tags: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = puzzles)]
pub struct NewPuzzle<'a> {
    pub puzzle_id: &'a str,
    pub fen: &'a str,
    pub moves: &'a str,
    pub rating: i32,
    pub rating_deviation: i32,
    pub popularity: i32,
    pub nb_plays: i32,
    pub themes: &'a str,
    pub game_url: &'a str,
    pub opening_tags: &'a str,
}
