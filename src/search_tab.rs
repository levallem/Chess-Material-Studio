use iced::widget::svg::Handle;
use iced::widget::{Container, Button, column as col, Text, Radio, row, Row, Svg, PickList, Slider, Scrollable, Space};
use iced::widget::text::LineHeight;
use iced::{alignment, Alignment, Element, Length, Task, Theme};
use std::io::BufReader;
use std::path::Path;

use diesel::Connection;
use iced_aw::TabLabel;
use chess::{Piece, PROMOTION_PIECES};
use crate::config::{load_config, SETTINGS_FILE, PIECES_DIRECTORY};
use crate::styles::{PieceTheme, btn_style_simple};
use crate::{Tab, Message, config, styles, lang, db, openings};

use lang::{DisplayTranslated,PickListWrapper};
use openings::{Openings, Variation};

#[derive(Debug, Clone)]
pub enum SearchMesssage {
    SliderMinRatingChanged(i32),
    SliderMaxRatingChanged(i32),
    SliderMinPopularityChanged(i32),
    SelectTheme(PickListWrapper<TacticalThemes>),
    SelectOpening(PickListWrapper<Openings>),
    SelectVariation(PickListWrapper<Variation>),
    SelectOpeningSide(OpeningSide),
    SelectPiecePromotion(Piece),
    ClickSearch,
    SelectBase(SearchBase),
}

impl PickListWrapper<TacticalThemes> {
    pub fn get_themes(lang: lang::Language) -> Vec<PickListWrapper<TacticalThemes>> {
        let mut themes_wrapper = Vec::new();
        for theme in TacticalThemes::ALL {
            themes_wrapper.push(
                PickListWrapper::<TacticalThemes> {
                    lang,
                    item: theme,
                }
            );
        }
        themes_wrapper
    }

    pub fn new_theme(lang: lang::Language, theme: TacticalThemes) -> Self {
        Self { lang, item: theme}
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum TacticalThemes {
    #[default]
    All,
    Opening, Middlegame, Endgame, RookEndgame, BishopEndgame, PawnEndgame, KnightEndgame, QueenEndgame, QueenRookEndgame,
    AdvancedPawn, AtackingF2F7, CapturingDefender, DiscoveredAttack, DoubleCheck, ExposedKing, Fork, HangingPiece, KingsideAttack, Pin, QueensideAttack, Sacrifice, Skewer, TrappedPiece,
    Attraction, Clearance, CollinearMove, DefensiveMove, Deflection, Interference, Intermezzo, QuietMove, XRayAttack, Zugzwang,
    Mate, MateIn1, MateIn2, MateIn3, MateIn4, MateIn5, AnastasiaMate, ArabianMate, BackRankMate, BalestraMate, BlindSwineMate, BodenMate, CornerMate, DoubleBishopMate, DovetailMate, EpauletteMate, HookMate, KillBoxMate, PillsburyMate, MorphysMate, OperaMate, SwallowstailMate, TriangleMate, VukovicMate, SmotheredMate,
    Castling, EnPassant, Promotion, UnderPromotion,
    Equality, Advantage, Crushing,
    OneMove, Short, Long, VeryLong,
    Master, MasterVsMaster, SuperGM
}

impl TacticalThemes {

    const ALL: [TacticalThemes; 73] = [
        TacticalThemes::All,
        TacticalThemes::Opening, TacticalThemes::Middlegame, TacticalThemes::Endgame, TacticalThemes::RookEndgame,
        TacticalThemes::BishopEndgame, TacticalThemes::PawnEndgame, TacticalThemes::KnightEndgame,
        TacticalThemes::QueenEndgame, TacticalThemes::QueenRookEndgame,

        TacticalThemes::AdvancedPawn, TacticalThemes::AtackingF2F7, TacticalThemes::CapturingDefender,
        TacticalThemes::DiscoveredAttack, TacticalThemes::DoubleCheck, TacticalThemes::ExposedKing,
        TacticalThemes::Fork, TacticalThemes::HangingPiece, TacticalThemes::KingsideAttack, TacticalThemes::Pin,
        TacticalThemes::QueensideAttack, TacticalThemes::Sacrifice, TacticalThemes::Skewer,
        TacticalThemes::TrappedPiece,

        TacticalThemes::Attraction, TacticalThemes::Clearance, TacticalThemes::CollinearMove, TacticalThemes::DefensiveMove,
        TacticalThemes::Deflection, TacticalThemes::Interference, TacticalThemes::Intermezzo,
        TacticalThemes::QuietMove, TacticalThemes::XRayAttack, TacticalThemes::Zugzwang,

        TacticalThemes::Mate, TacticalThemes::MateIn1, TacticalThemes::MateIn2, TacticalThemes::MateIn3,
        TacticalThemes::MateIn4, TacticalThemes::MateIn5, TacticalThemes::AnastasiaMate, TacticalThemes::ArabianMate,
        TacticalThemes::BackRankMate, TacticalThemes::BalestraMate, TacticalThemes::BlindSwineMate, TacticalThemes::BodenMate,
        TacticalThemes::CornerMate, TacticalThemes::DoubleBishopMate, TacticalThemes::DovetailMate, TacticalThemes::EpauletteMate, TacticalThemes::HookMate,
        TacticalThemes::KillBoxMate, TacticalThemes::PillsburyMate, TacticalThemes::MorphysMate, TacticalThemes::OperaMate, TacticalThemes::SwallowstailMate, TacticalThemes::TriangleMate,
        TacticalThemes::VukovicMate, TacticalThemes::SmotheredMate,

        TacticalThemes::Castling, TacticalThemes::EnPassant, TacticalThemes::Promotion,
        TacticalThemes::UnderPromotion,

        TacticalThemes::Equality, TacticalThemes::Advantage, TacticalThemes::Crushing,

        TacticalThemes::OneMove, TacticalThemes::Short, TacticalThemes::Long, TacticalThemes::VeryLong,

        TacticalThemes::Master, TacticalThemes::MasterVsMaster, TacticalThemes::SuperGM
    ];

    pub fn get_tr_key(&self) -> &str {
        match self {
            TacticalThemes::All => "themes_all",
            _ => self.get_tag_name(),
        }
    }

    pub fn get_tag_name(&self) -> &str {
        match self {
            TacticalThemes::All => "",
            TacticalThemes::Opening => "opening",
            TacticalThemes::Middlegame => "middlegame",
            TacticalThemes::Endgame => "endgame",
            TacticalThemes::RookEndgame => "rookEndgame",
            TacticalThemes::BishopEndgame => "bishopEndgame",
            TacticalThemes::PawnEndgame => "pawnEndgame",
            TacticalThemes::KnightEndgame => "knightEndgame",
            TacticalThemes::QueenEndgame => "queenEndgame",
            TacticalThemes::QueenRookEndgame => "queenRookEndgame",

            TacticalThemes::AdvancedPawn => "advancedPawn",
            TacticalThemes::AtackingF2F7 => "attackingF2F7",
            TacticalThemes::CapturingDefender => "capturingDefender",
            TacticalThemes::DiscoveredAttack => "discoveredAttack",
            TacticalThemes::DoubleCheck => "doubleCheck",
            TacticalThemes::ExposedKing => "exposedKing",
            TacticalThemes::Fork => "fork",
            TacticalThemes::HangingPiece => "hangingPiece",
            TacticalThemes::KingsideAttack => "kingsideAttack",
            TacticalThemes::Pin => "pin",
            TacticalThemes::QueensideAttack => "queensideAttack",
            TacticalThemes::Sacrifice => "sacrifice",
            TacticalThemes::Skewer => "skewer",
            TacticalThemes::TrappedPiece => "trappedPiece",

            TacticalThemes::Attraction => "attraction",
            TacticalThemes::Clearance => "clearance",
            TacticalThemes::CollinearMove => "collinearMove",
            TacticalThemes::DefensiveMove => "defensiveMove",
            TacticalThemes::Deflection => "deflection",
            TacticalThemes::Interference => "interference",
            TacticalThemes::Intermezzo => "intermezzo",
            TacticalThemes::QuietMove => "quietMove",
            TacticalThemes::XRayAttack =>"xRayAttack",
            TacticalThemes::Zugzwang => "zugzwang",

            TacticalThemes::Mate => "mate",
            TacticalThemes::MateIn1 => "mateIn1",
            TacticalThemes::MateIn2 => "mateIn2",
            TacticalThemes::MateIn3 => "mateIn3",
            TacticalThemes::MateIn4 => "mateIn4",
            TacticalThemes::MateIn5 => "mateIn5",
            TacticalThemes::AnastasiaMate => "anastasiaMate",
            TacticalThemes::ArabianMate => "arabianMate",
            TacticalThemes::BackRankMate => "backRankMate",
            TacticalThemes::BalestraMate => "balestraMate",
            TacticalThemes::BlindSwineMate => "blindSwineMate",
            TacticalThemes::BodenMate => "bodenMate",
            TacticalThemes::CornerMate => "cornerMate",
            TacticalThemes::DoubleBishopMate => "doubleBishopMate",
            TacticalThemes::DovetailMate => "dovetailMate",
            TacticalThemes::EpauletteMate => "epauletteMate",
            TacticalThemes::HookMate => "hookMate",
            TacticalThemes::KillBoxMate => "killBoxMate",
            TacticalThemes::PillsburyMate => "pillsburysMate",
            TacticalThemes::MorphysMate => "morphysMate",
            TacticalThemes::OperaMate => "operaMate",
            TacticalThemes::SwallowstailMate => "swallowstailMate",
            TacticalThemes::TriangleMate => "triangleMate",
            TacticalThemes::VukovicMate => "vukovicMate",
            TacticalThemes::SmotheredMate => "smotheredMate",

            TacticalThemes::Castling => "castling",
            TacticalThemes::EnPassant => "enPassant",
            TacticalThemes::Promotion => "promotion",
            TacticalThemes::UnderPromotion => "underPromotion",
            TacticalThemes::Equality => "equality",
            TacticalThemes::Advantage => "advantage",
            TacticalThemes::Crushing => "crushing",

            TacticalThemes::OneMove => "oneMove",
            TacticalThemes::Short => "short",
            TacticalThemes::Long => "long",
            TacticalThemes::VeryLong => "veryLong",

            TacticalThemes::Master => "master",
            TacticalThemes::MasterVsMaster => "masterVsMaster",
            TacticalThemes::SuperGM => "superGM",
        }
    }

}

impl DisplayTranslated for TacticalThemes {
    fn to_str_tr(&self) -> &str {
        self.get_tr_key()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum OpeningSide {
    Any, White, Black
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SearchBase {
    Lichess, Favorites
}

pub fn gen_piece_vec(theme: &PieceTheme) -> Vec<Handle> {
    let mut handles = Vec::<Handle>::with_capacity(5);
    let theme_str = &theme.to_string();
    // this first entry won't be used, it's there just to fill the vec, so we can index by the Piece
    handles.insert(0, Handle::from_path(String::from(PIECES_DIRECTORY) + "cburnett/wP.svg"));
    handles.insert(Piece::Knight.to_index(), Handle::from_path(String::from(PIECES_DIRECTORY) + theme_str + "/wN.svg"));
    handles.insert(Piece::Bishop.to_index(), Handle::from_path(String::from(PIECES_DIRECTORY) + theme_str + "/wB.svg"));
    handles.insert(Piece::Rook.to_index(), Handle::from_path(String::from(PIECES_DIRECTORY) + theme_str + "/wR.svg"));
    handles.insert(Piece::Queen.to_index(), Handle::from_path(String::from(PIECES_DIRECTORY) + theme_str + "/wQ.svg"));
    handles
}

#[derive(Debug)]
pub struct SearchTab {
    pub theme: PickListWrapper<TacticalThemes>,
    pub opening: PickListWrapper<Openings>,
    pub variation: PickListWrapper<Variation>,
    pub opening_side: Option<OpeningSide>,
    slider_min_rating_value: i32,
    slider_max_rating_value: i32,
    slider_min_popularity: i32,
    pub piece_theme_promotion: styles::PieceTheme,
    pub piece_to_promote_to: Piece,

    pub show_searching_msg: bool,
    pub lang: lang::Language,
    base: Option<SearchBase>,
    pub promotion_piece_img: Vec<Handle>,
}

fn adapt_sqlite_puzzle(
    puzzle: offline_chess_puzzles::models::Puzzle,
) -> config::Puzzle {
    config::Puzzle {
        puzzle_id: puzzle.puzzle_id,
        fen: puzzle.fen,
        moves: puzzle.moves,
        rating: puzzle.rating,
        rating_deviation: puzzle.rating_deviation,
        popularity: puzzle.popularity,
        nb_plays: puzzle.nb_plays,
        themes: puzzle.themes,
        game_url: puzzle.game_url,
        opening: puzzle.opening,
    }
}

fn search_csv_from_path(
    csv_path: &Path,
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: TacticalThemes,
    opening: Openings,
    variation: Variation,
    op_side: Option<OpeningSide>,
    result_limit: usize,
) -> Option<Vec<config::Puzzle>> {
    let mut puzzles: Vec<config::Puzzle> = Vec::new();

    let reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(csv_path);

    if let Ok(mut reader) = reader {
        puzzles.clear();
        if opening != Openings::Any {
            let opening_tag: &str = if variation.name != Variation::ANY_STR {
                &variation.name
            } else {
                opening.get_field_name()
            };
            let side = match op_side {
                None => OpeningSide::Any,
                Some(x) => x
            };
            match side {
                OpeningSide::Any => {
                    for result in reader.deserialize::<config::Puzzle>() {
                        if let Ok(record) = result &&
                                record.opening.contains(opening_tag) &&
                                record.rating >= min_rating && record.rating <= max_rating &&
                                record.popularity >= min_popularity &&
                                record.themes.contains(theme.get_tag_name()) {
                            puzzles.push(record);
                        }
                        if puzzles.len() == result_limit {
                            break;
                        }
                    }
                } OpeningSide::Black => {
                    for result in reader.deserialize::<config::Puzzle>() {
                        if let Ok(record) = result &&
                                record.opening.contains(opening_tag) &&
                                !record.game_url.contains("black") &&
                                record.rating >= min_rating && record.rating <= max_rating &&
                                record.popularity >= min_popularity &&
                                record.themes.contains(theme.get_tag_name()) {
                            puzzles.push(record);
                        }
                        if puzzles.len() == result_limit {
                            break;
                        }
                    }
                } OpeningSide::White => {
                    for result in reader.deserialize::<config::Puzzle>() {
                        if let Ok(record) = result &&
                                record.opening.contains(opening_tag) &&
                                record.game_url.contains("black") &&
                                record.rating >= min_rating && record.rating <= max_rating &&
                                record.popularity >= min_popularity &&
                                record.themes.contains(theme.get_tag_name()) {
                            puzzles.push(record);
                        }
                        if puzzles.len() == result_limit {
                            break;
                        }
                    }
                }
            }
        } else {
            for result in reader.deserialize::<config::Puzzle>() {
                if let Ok(record) = result && record.rating >= min_rating && record.rating <= max_rating &&
                        record.popularity >= min_popularity &&
                        record.themes.contains(theme.get_tag_name()) {
                    puzzles.push(record);
                }
                if puzzles.len() == result_limit {
                    break;
                }
            }
        }
    }
    Some(puzzles)
}

fn search_sqlite_from_path(
    db_path: &Path,
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: TacticalThemes,
    opening: Openings,
    variation: Variation,
    op_side: Option<OpeningSide>,
    result_limit: usize,
) -> Result<Vec<config::Puzzle>, String> {
    if !db_path.is_file() {
        return Err(format!("database file not found: {}", db_path.display()));
    }
    let path_str = db_path.to_str().ok_or("invalid db path (non-UTF-8)")?;
    let mut conn = diesel::sqlite::SqliteConnection::establish(path_str)
        .map_err(|e| format!("cannot open DB: {}", e))?;

    let theme_tag = match theme {
        TacticalThemes::All => None,
        other => Some(other.get_tag_name().to_string()),
    };

    let opening_tag = if opening == Openings::Any {
        None
    } else if variation.name != Variation::ANY_STR {
        Some(variation.name.to_string())
    } else {
        Some(opening.get_field_name().to_string())
    };

    let side = if opening_tag.is_none() {
        offline_chess_puzzles::puzzle_search::SearchSide::Any
    } else {
        match op_side {
            None | Some(OpeningSide::Any) => offline_chess_puzzles::puzzle_search::SearchSide::Any,
            Some(OpeningSide::White) => offline_chess_puzzles::puzzle_search::SearchSide::White,
            Some(OpeningSide::Black) => offline_chess_puzzles::puzzle_search::SearchSide::Black,
        }
    };

    let filters = offline_chess_puzzles::puzzle_search::PuzzleSearchFilters {
        min_rating,
        max_rating,
        min_popularity,
        theme_tag,
        opening_tag,
        side,
        limit: result_limit,
    };

    let results = offline_chess_puzzles::puzzle_search::search_puzzles(&mut conn, &filters)?;
    Ok(results.into_iter().map(adapt_sqlite_puzzle).collect())
}

pub fn search_with_config(
    config: &config::OfflinePuzzlesConfig,
    min_rating: i32,
    max_rating: i32,
    min_popularity: i32,
    theme: TacticalThemes,
    opening: Openings,
    variation: Variation,
    op_side: Option<OpeningSide>,
    result_limit: usize,
) -> Option<Vec<config::Puzzle>> {
    match &config.puzzle_sqlite_location {
        Some(sqlite_path) => {
            let path = std::path::Path::new(sqlite_path);
            match search_sqlite_from_path(
                path, min_rating, max_rating, min_popularity,
                theme, opening, variation, op_side, result_limit,
            ) {
                Ok(results) => Some(results),
                Err(e) => {
                    eprintln!("CMS-013: SQLite search failed: {}", e);
                    None
                }
            }
        }
        None => {
            let csv_path = std::path::Path::new(&config.puzzle_db_location);
            search_csv_from_path(
                csv_path, min_rating, max_rating, min_popularity,
                theme, opening, variation, op_side, result_limit,
            )
        }
    }
}

impl SearchTab {
    pub fn new() -> Self {
        SearchTab {
            theme : PickListWrapper::new_theme(config::SETTINGS.lang, config::SETTINGS.last_theme),
            opening: PickListWrapper::new_opening(config::SETTINGS.lang, config::SETTINGS.last_opening),
            variation: PickListWrapper::new_variation(config::SETTINGS.lang, config::SETTINGS.last_variation.clone()),
            opening_side: config::SETTINGS.last_opening_side,
            slider_min_rating_value: config::SETTINGS.last_min_rating,
            slider_max_rating_value: config::SETTINGS.last_max_rating,
            slider_min_popularity: config::SETTINGS.last_min_popularity,
            piece_theme_promotion: config::SETTINGS.piece_theme,
            piece_to_promote_to: Piece::Queen,
            show_searching_msg: false,
            lang: config::SETTINGS.lang,
            base: Some(SearchBase::Lichess),
            promotion_piece_img: gen_piece_vec(&config::SETTINGS.piece_theme),
        }
    }

    pub fn update(&mut self, message: SearchMesssage) -> Task<Message> {//config::AppEvents {
        match message {
            SearchMesssage::SliderMinRatingChanged(new_value) => {
                self.slider_min_rating_value = new_value;
                Task::none()
            } SearchMesssage::SliderMaxRatingChanged(new_value) => {
                self.slider_max_rating_value = new_value;
                Task::none()
            } SearchMesssage::SliderMinPopularityChanged(new_value) => {
                self.slider_min_popularity = new_value;
                Task::none()
            } SearchMesssage::SelectTheme(new_theme) => {
                self.theme = new_theme;
                Task::none()
            } SearchMesssage::SelectOpening(new_opening) => {
                self.opening = new_opening;
                self.variation.item = Variation::ANY;
                Task::none()
            } SearchMesssage::SelectVariation(new_variation) => {
                self.variation = new_variation;
                Task::none()
            } SearchMesssage::SelectOpeningSide(new_opening_side) => {
                self.opening_side = Some(new_opening_side);
                Task::none()
            } SearchMesssage::SelectPiecePromotion(piece) => {
                self.piece_to_promote_to = piece;
                Task::none()
            } SearchMesssage::ClickSearch => {
                self.show_searching_msg = true;
                SearchTab::save_search_settings(self.slider_min_rating_value,
                    self.slider_max_rating_value, self.slider_min_popularity, self.theme.item,
                    self.opening.item, self.variation.item.clone(), self.opening_side);

                let config = load_config();
                if self.base == Some(SearchBase::Favorites) {
                    Task::perform(
                        SearchTab::search_favs(self.slider_min_rating_value,
                            self.slider_max_rating_value, self.slider_min_popularity,
                            self.theme.item, self.opening.item, self.variation.item.clone(),
                            self.opening_side, config.search_results_limit), Message::LoadPuzzle)
                } else {
                    Task::perform(
                        SearchTab::search(self.slider_min_rating_value,
                            self.slider_max_rating_value, self.slider_min_popularity,
                            self.theme.item, self.opening.item, self.variation.item.clone(),
                            self.opening_side, config.search_results_limit), Message::LoadPuzzle)
                }
            } SearchMesssage::SelectBase(base) => {
                self.base = Some(base);
                Task::none()
            }
        }
    }

    pub fn save_search_settings(min_rating: i32, max_rating: i32, min_popularity: i32, theme: TacticalThemes, opening: Openings, variation: Variation, op_side: Option<OpeningSide>) {
        let file = std::fs::File::open(SETTINGS_FILE);
        if let Ok(file) = file {
            let buf_reader = BufReader::new(file);
            if let Ok(mut config) = serde_json::from_reader::<std::io::BufReader<std::fs::File>, config::OfflinePuzzlesConfig>(buf_reader) {
                config.last_min_rating = min_rating;
                config.last_max_rating = max_rating;
                config.last_min_popularity = min_popularity;
                config.last_theme = theme;
                config.last_opening = opening;
                config.last_variation = variation;
                config.last_opening_side = op_side;

                let file = std::fs::File::create(SETTINGS_FILE);
                if let Ok(file) = file && serde_json::to_writer_pretty(file, &config).is_err() {
                    println!("Error saving search options.");
                }
            }
        }
    }

    pub async fn search_favs(min_rating: i32, max_rating: i32, min_popularity: i32, theme: TacticalThemes, opening: Openings, variation:Variation, op_side: Option<OpeningSide>, result_limit: usize) -> Option<Vec<config::Puzzle>> {
        db::get_favorites(min_rating, max_rating, min_popularity, theme, opening, variation, op_side, result_limit)
    }

    pub async fn search(min_rating: i32, max_rating: i32, min_popularity: i32, theme: TacticalThemes, opening: Openings, variation: Variation, op_side: Option<OpeningSide>, result_limit: usize) -> Option<Vec<config::Puzzle>> {
        let config = load_config();
        search_with_config(
            &config,
            min_rating, max_rating, min_popularity,
            theme, opening, variation, op_side, result_limit,
        )
    }
}


impl Tab for SearchTab {
    type Message = Message;

    fn title(&self) -> String {
        lang::tr(&self.lang, "search")
    }

    fn tab_label(&self) -> TabLabel {
        TabLabel::Text(self.title())
    }

    fn content(&self) -> Element<'_, Message> {
        let mut search_col = col![
            Container::new(
                row![
                    Radio::new(lang::tr(&self.lang, "lichess_db"), SearchBase::Lichess, self.base, SearchMesssage::SelectBase).style(styles::radio_style),
                    Radio::new(lang::tr(&self.lang, "my_favories"), SearchBase::Favorites, self.base, SearchMesssage::SelectBase).style(styles::radio_style),
                ].spacing(10)
            ).align_x(alignment::Horizontal::Center).width(Length::Fill),
            row![
                Text::new(lang::tr(&self.lang, "min_rating")),
                Slider::new(
                    0..=config::MAX_RATING,
                    self.slider_min_rating_value,
                    SearchMesssage::SliderMinRatingChanged,
                ).style(styles::slider_style),
                Text::new(self.slider_min_rating_value.to_string())
            ].width(Length::Fill),
            row![
                Text::new(lang::tr(&self.lang, "max_rating")),
                Slider::new(
                    0..=config::MAX_RATING,
                    self.slider_max_rating_value,
                    SearchMesssage::SliderMaxRatingChanged,
                ).style(styles::slider_style),
                Text::new(self.slider_max_rating_value.to_string())
                ].width(Length::Fill),
            row![
                Text::new(lang::tr(&self.lang, "min_popularity")),
                Slider::new(
                    -100..=100,
                    self.slider_min_popularity,
                    SearchMesssage::SliderMinPopularityChanged,
                ).style(styles::slider_style),
                Text::new(self.slider_min_popularity.to_string())
                ].width(Length::Fill),
            Text::new(lang::tr(&self.lang, "theme_label")),
            PickList::new(
                PickListWrapper::get_themes(self.lang),
                Some(self.theme.clone()),
                SearchMesssage::SelectTheme
            ).style(styles::pick_list_style).menu_style(styles::menu_style),
            Text::new(lang::tr(&self.lang, "in_opening")),
            PickList::new(
                PickListWrapper::get_openings(self.lang),
                Some(self.opening.clone()),
                SearchMesssage::SelectOpening
            ).style(styles::pick_list_style).menu_style(styles::menu_style),
            Text::new(lang::tr(&self.lang, "in_the_variation")),
            PickList::new(
                PickListWrapper::get_variations(self.lang, Some(&self.opening.item)),
                Some(self.variation.clone()),
                SearchMesssage::SelectVariation
            ).style(styles::pick_list_style).menu_style(styles::menu_style),
        ].padding([0, 30]).spacing(10).align_x(Alignment::Center);

        if self.opening.item != Openings::Any {
            let row_color = row![
                Radio::new(lang::tr(&self.lang, "any"), OpeningSide::Any, self.opening_side, SearchMesssage::SelectOpeningSide).style(styles::radio_style),
                Radio::new(lang::tr(&self.lang, "white"), OpeningSide::White, self.opening_side, SearchMesssage::SelectOpeningSide).style(styles::radio_style),
                Radio::new(lang::tr(&self.lang, "black"), OpeningSide::Black, self.opening_side, SearchMesssage::SelectOpeningSide).style(styles::radio_style)
            ].spacing(5).align_y(Alignment::Center);
            search_col = search_col.push(Text::new(lang::tr(&self.lang, "side"))).push(row_color);
        }

        let mut row_promotion = Row::new().spacing(5).align_y(Alignment::Center);
        if self.piece_theme_promotion == PieceTheme::FontAlpha {
            // Promotion piece selector
            for i in 0..4 {
                let piece;
                let mut text;
                match i {
                    0 => {
                        piece = Piece::Rook;
                        text = String::from("r");
                    }
                    1 => {
                        piece = Piece::Knight;
                        text = String::from("h");
                    }
                    2 => {
                        piece = Piece::Bishop;
                        text = String::from("b");
                    }
                    _ => {
                        piece = Piece::Queen;
                        text = String::from("q");
                    }
                };
                if self.piece_to_promote_to == piece {
                    text = text.to_uppercase();
                };
                row_promotion = row_promotion.push(Row::new().spacing(0).align_y(Alignment::Center)
                    .push(Button::new(
                        Text::new(text).font(config::CHESS_ALPHA).size(60).align_y(Alignment::Center).line_height(LineHeight::Absolute(60.into()))
                    )
                    .padding(0)
                    .width(60)
                    .height(60)
                    .style(styles::btn_style_paper)
                    .on_press(SearchMesssage::SelectPiecePromotion(piece))
                ));
            }
        } else {
            for piece in PROMOTION_PIECES {
                let square_style: styles::ChessBtn =
                    if self.piece_to_promote_to == piece {
                        styles::btn_style_dark_square
                    } else {
                        styles::btn_style_light_square
                    };
                row_promotion = row_promotion.push(
                    Row::new().width(60).height(60).align_y(Alignment::Start)
                    .push(Button::new(
                        Svg::new(self.promotion_piece_img[piece.to_index()].clone())
                    )
                    .on_press(SearchMesssage::SelectPiecePromotion(piece))
                    .style(square_style)
                ));
            }

        }

        search_col = search_col.push(Space::new().height(10));
        if self.show_searching_msg {
            search_col = search_col.push(Text::new(lang::tr(&self.lang, "searching")));
        }
        search_col = search_col
            .push(Button::new(Text::new(lang::tr(&self.lang, "btn_search"))).padding(5).on_press(SearchMesssage::ClickSearch).style(btn_style_simple))
            .push(Text::new(lang::tr(&self.lang, "promotion_piece")))
            .push(row_promotion);

        let scroll = Scrollable::new(search_col);
        let content: Element<SearchMesssage, Theme, iced::Renderer> = Container::new(scroll)
            .align_x(alignment::Horizontal::Center).height(Length::Fill)
            .into();

        content.map(Message::Search)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use offline_chess_puzzles::puzzle_import::MIGRATIONS;
    use diesel_migrations::MigrationHarness;
    use std::sync::atomic::{AtomicU64, Ordering};

    const FIXTURE: &str = include_str!("../tests/fixtures/lichess_puzzles_sample.csv");

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("cms_test_tmp");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}_{}_{}", name, std::process::id(), id))
    }

    fn setup_test_sqlite() -> std::path::PathBuf {
        let db_path = tmp_path("search_tab_test");
        let mut conn = diesel::sqlite::SqliteConnection::establish(db_path.to_str().unwrap())
            .expect("Failed to open test database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        offline_chess_puzzles::puzzle_import::import_puzzles_from_reader(&mut conn, FIXTURE.as_bytes())
            .expect("fixture import should succeed");
        drop(conn);
        db_path
    }

    fn setup_test_csv() -> std::path::PathBuf {
        let csv_path = tmp_path("search_tab_test.csv");
        std::fs::write(&csv_path, FIXTURE.as_bytes()).expect("write fixture csv");
        csv_path
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    // ── Adapter test ────────────────────────────────────────────────

    #[test]
    fn test_adapt_sqlite_puzzle_all_fields() {
        let lib_puzzle = offline_chess_puzzles::models::Puzzle {
            puzzle_id: "test_001".to_string(),
            fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1".to_string(),
            moves: "e7e5".to_string(),
            rating: 1500,
            rating_deviation: 70,
            popularity: 95,
            nb_plays: 10000,
            themes: "fork opening".to_string(),
            game_url: "https://lichess.org/training/abc123".to_string(),
            opening: "Italian_Game".to_string(),
        };
        let adapted = adapt_sqlite_puzzle(lib_puzzle);
        assert_eq!(adapted.puzzle_id, "test_001");
        assert_eq!(adapted.fen, "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
        assert_eq!(adapted.moves, "e7e5");
        assert_eq!(adapted.rating, 1500);
        assert_eq!(adapted.rating_deviation, 70);
        assert_eq!(adapted.popularity, 95);
        assert_eq!(adapted.nb_plays, 10000);
        assert_eq!(adapted.themes, "fork opening");
        assert_eq!(adapted.game_url, "https://lichess.org/training/abc123");
        assert_eq!(adapted.opening, "Italian_Game");
    }

    // ── Dispatcher: None → CSV ──────────────────────────────────────

    #[test]
    fn test_none_sqlite_uses_csv() {
        let csv_path = setup_test_csv();
        let mut cfg = config::OfflinePuzzlesConfig::default();
        cfg.puzzle_sqlite_location = None;
        cfg.puzzle_db_location = csv_path.to_str().unwrap().to_string();

        let results = search_with_config(
            &cfg,
            0, 4000, -100,
            TacticalThemes::All,
            Openings::Any,
            Variation::ANY.clone(),
            Some(OpeningSide::Any),
            10,
        );
        assert!(results.is_some());
        let puzzles = results.unwrap();
        let mut ids: Vec<&str> = puzzles.iter().map(|p| p.puzzle_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["00008", "00009", "00010", "00011"]);
        cleanup(&csv_path);
    }

    // ── Dispatcher: Some → SQLite ───────────────────────────────────

    #[test]
    fn test_some_sqlite_uses_sqlite() {
        let db_path = setup_test_sqlite();
        let mut cfg = config::OfflinePuzzlesConfig::default();
        cfg.puzzle_sqlite_location = Some(db_path.to_str().unwrap().to_string());
        cfg.puzzle_db_location = "/nonexistent/path.csv".to_string();

        let results = search_with_config(
            &cfg,
            0, 4000, -100,
            TacticalThemes::All,
            Openings::Any,
            Variation::ANY.clone(),
            Some(OpeningSide::Any),
            10,
        );
        assert!(results.is_some());
        let puzzles = results.unwrap();
        let mut ids: Vec<&str> = puzzles.iter().map(|p| p.puzzle_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["00008", "00009", "00010", "00011"]);
        cleanup(&db_path);
    }

    // ── Dispatcher: error → no fallback ─────────────────────────────

    #[test]
    fn test_sqlite_error_no_fallback() {
        let csv_path = setup_test_csv();
        let mut cfg = config::OfflinePuzzlesConfig::default();
        cfg.puzzle_sqlite_location = Some("/nonexistent/db.sqlite".to_string());
        cfg.puzzle_db_location = csv_path.to_str().unwrap().to_string();

        let results = search_with_config(
            &cfg,
            0, 4000, -100,
            TacticalThemes::All,
            Openings::Any,
            Variation::ANY.clone(),
            Some(OpeningSide::Any),
            10,
        );
        assert!(results.is_none(), "SQLite error should NOT fall back to CSV");
        cleanup(&csv_path);
    }

    // ── Parity: ALL ─────────────────────────────────────────────────

    #[test]
    fn test_parity_all() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 0, 4000, -100,
            TacticalThemes::All, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 0, 4000, -100,
            TacticalThemes::All, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: rating + popularity ─────────────────────────────────

    #[test]
    fn test_parity_rating_popularity() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 1500, 1700, 90,
            TacticalThemes::All, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 1500, 1700, 90,
            TacticalThemes::All, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: theme ───────────────────────────────────────────────

    #[test]
    fn test_parity_theme_fork() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 0, 4000, -100,
            TacticalThemes::Fork, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 0, 4000, -100,
            TacticalThemes::Fork, Openings::Any, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: opening ─────────────────────────────────────────────

    #[test]
    fn test_parity_opening() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: side Black ──────────────────────────────────────────

    #[test]
    fn test_parity_side_black() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Black), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Black), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: side White ──────────────────────────────────────────

    #[test]
    fn test_parity_side_white() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::White), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 0, 4000, -100,
            TacticalThemes::All, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::White), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }

    // ── Parity: combined ────────────────────────────────────────────

    #[test]
    fn test_parity_combined() {
        let db_path = setup_test_sqlite();
        let csv_path = setup_test_csv();

        let csv_results = search_csv_from_path(
            &csv_path, 1600, 1800, 0,
            TacticalThemes::Fork, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();
        let sqlite_results = search_sqlite_from_path(
            &db_path, 1600, 1800, 0,
            TacticalThemes::Fork, Openings::ItalianGame, Variation::ANY.clone(),
            Some(OpeningSide::Any), 10,
        ).unwrap();

        let csv_ids: std::collections::HashSet<&str> = csv_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        let sqlite_ids: std::collections::HashSet<&str> = sqlite_results.iter().map(|p| p.puzzle_id.as_str()).collect();
        assert_eq!(csv_ids, sqlite_ids);
        cleanup(&db_path);
        cleanup(&csv_path);
    }
}
