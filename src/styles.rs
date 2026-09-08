use iced::overlay::menu;

use iced::theme::Palette;
use iced::widget::{button, checkbox, container, pick_list, radio, slider};
use iced::{Border, Color};
use iced_aw::style::tab_bar;

macro_rules! rgb {
    ($r:expr, $g:expr, $b:expr) => {
        iced::Color::from_rgb($r as f32 / 255.0, $g as f32 / 255.0, $b as f32 / 255.0)
    };
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterfaceTheme {
    #[default]
    Light,
    Dark,
}

impl InterfaceTheme {
    pub const ALL: [InterfaceTheme; 2] = [InterfaceTheme::Light, InterfaceTheme::Dark];

    pub fn palette(self) -> Palette {
        match self {
            // Keep the historical default UI palette for existing installations.
            Self::Light => Palette {
                background: Color::WHITE,
                text: Color::BLACK,
                primary: rgb!(235.0, 249.0, 255),
                success: rgb!(110.0, 174.0, 213.0),
                danger: Color::BLACK,
                warning: Color::BLACK,
            },
            Self::Dark => Palette {
                background: rgb!(32, 33, 36),
                text: rgb!(232, 234, 237),
                primary: rgb!(48, 49, 52),
                success: rgb!(95, 99, 104),
                danger: rgb!(95, 99, 104),
                warning: rgb!(95, 99, 104),
            },
        }
    }
}

impl std::fmt::Display for InterfaceTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum PieceTheme {
    Cburnett,
    Alpha,
    Merida,
    California,
    Cardinal,
    Governor,
    Dubrovny,
    Gioco,
    Icpieces,
    Maestro,
    Staunty,
    Tatiana,
    FontAlpha,
}

impl PieceTheme {
    pub const ALL: [PieceTheme; 3] = [
        PieceTheme::Cburnett,
        PieceTheme::Alpha,
        PieceTheme::FontAlpha,
    ];

    pub fn normalize(self) -> Self {
        match self {
            Self::California
            | Self::Cardinal
            | Self::Governor
            | Self::Dubrovny
            | Self::Gioco
            | Self::Icpieces
            | Self::Maestro
            | Self::Staunty
            | Self::Tatiana
            | Self::Merida => Self::Cburnett,
            retained => retained,
        }
    }
}

#[cfg(test)]
mod piece_theme_tests {
    use super::PieceTheme;

    #[test]
    fn retained_piece_themes_remain_available_and_unchanged() {
        for theme in [
            PieceTheme::Cburnett,
            PieceTheme::Alpha,
            PieceTheme::FontAlpha,
        ] {
            assert_eq!(theme.normalize(), theme);
        }
    }

    #[test]
    fn retired_piece_themes_normalize_to_cburnett() {
        for theme in [
            PieceTheme::California,
            PieceTheme::Cardinal,
            PieceTheme::Governor,
            PieceTheme::Dubrovny,
            PieceTheme::Gioco,
            PieceTheme::Icpieces,
            PieceTheme::Maestro,
            PieceTheme::Staunty,
            PieceTheme::Tatiana,
            PieceTheme::Merida,
        ] {
            assert_eq!(theme.normalize(), PieceTheme::Cburnett);
        }
    }

    #[test]
    fn available_piece_themes_only_lists_retained_themes() {
        assert_eq!(
            PieceTheme::ALL,
            [
                PieceTheme::Cburnett,
                PieceTheme::Alpha,
                PieceTheme::FontAlpha,
            ]
        );
    }
}

impl std::fmt::Display for PieceTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                PieceTheme::Alpha => "alpha",
                PieceTheme::Merida => "merida",
                PieceTheme::California => "california",
                PieceTheme::Cardinal => "cardinal",
                PieceTheme::Governor => "governor",
                PieceTheme::Dubrovny => "dubrovny",
                PieceTheme::Gioco => "gioco",
                PieceTheme::Icpieces => "icpieces",
                PieceTheme::Maestro => "maestro",
                PieceTheme::Staunty => "staunty",
                PieceTheme::Tatiana => "tatiana",
                PieceTheme::FontAlpha => "Paper - chess alpha",
                _ => "cburnett",
            }
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum BoardTheme {
    #[default]
    Blue,
    Green,
    Brown,
    Purple,
    Red,
    Grey,
    MonochromeGrey,
    BlueDark,
    GreenDark,
    BrownDark,
    PurpleDark,
    RedDark,
    GreyDark,
    MonochromeGreyDark,
    Trans,
    Enby,
}

impl BoardTheme {
    pub fn board_palette(&self) -> BoardPalette {
        match self {
            Self::Blue => BoardPalette::BLUE,
            Self::Green => BoardPalette::GREEN,
            Self::Brown => BoardPalette::BROWN,
            Self::Purple => BoardPalette::PURPLE,
            Self::Red => BoardPalette::RED,
            Self::Grey => BoardPalette::GREY,
            Self::MonochromeGrey => BoardPalette::MONOCHROME_GREY,
            Self::BlueDark => BoardPalette::BLUE_DARK,
            Self::GreenDark => BoardPalette::GREEN_DARK,
            Self::BrownDark => BoardPalette::BROWN_DARK,
            Self::PurpleDark => BoardPalette::PURPLE_DARK,
            Self::RedDark => BoardPalette::RED_DARK,
            Self::GreyDark => BoardPalette::GREY_DARK,
            Self::MonochromeGreyDark => BoardPalette::MONOCHROME_GREY_DARK,
            Self::Trans => BoardPalette::TRANS,
            Self::Enby => BoardPalette::ENBY,
        }
    }
    pub const ALL: [BoardTheme; 16] = [
        BoardTheme::Blue,
        BoardTheme::Green,
        BoardTheme::Brown,
        BoardTheme::Purple,
        BoardTheme::Red,
        BoardTheme::Grey,
        BoardTheme::MonochromeGrey,
        BoardTheme::BlueDark,
        BoardTheme::GreenDark,
        BoardTheme::BrownDark,
        BoardTheme::PurpleDark,
        BoardTheme::RedDark,
        BoardTheme::GreyDark,
        BoardTheme::MonochromeGreyDark,
        BoardTheme::Trans,
        BoardTheme::Enby,
    ];
}

impl std::fmt::Display for BoardTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BoardTheme::Blue => "Blue",
                BoardTheme::Green => "Green",
                BoardTheme::Brown => "Brown",
                BoardTheme::Purple => "Purple",
                BoardTheme::Red => "Red",
                BoardTheme::Grey => "Grey",
                BoardTheme::MonochromeGrey => "Monochrome Grey",
                BoardTheme::BlueDark => "Blue - Dark Mode",
                BoardTheme::GreenDark => "Green - Dark Mode",
                BoardTheme::BrownDark => "Brown - Dark Mode",
                BoardTheme::PurpleDark => "Purple - Dark Mode",
                BoardTheme::RedDark => "Red - Dark Mode",
                BoardTheme::GreyDark => "Grey - Dark Mode",
                BoardTheme::MonochromeGreyDark => "Monochrome Grey - Dark",
                BoardTheme::Trans => "Trans colors",
                BoardTheme::Enby => "NB colors",
            }
        )
    }
}

#[cfg(test)]
mod interface_theme_tests {
    use super::{BoardTheme, InterfaceTheme};

    #[test]
    fn light_and_dark_interface_palettes_are_distinct() {
        let light = InterfaceTheme::Light.palette();
        let dark = InterfaceTheme::Dark.palette();

        assert_ne!(light.background, dark.background);
        assert_ne!(light.text, dark.text);
        assert_ne!(light.primary, dark.primary);
    }

    #[test]
    fn interface_theme_does_not_change_board_square_palette() {
        let before = BoardTheme::Blue.board_palette();
        let _light = InterfaceTheme::Light.palette();
        let _dark = InterfaceTheme::Dark.palette();
        let after = BoardTheme::Blue.board_palette();

        assert_eq!(before.light_square, after.light_square);
        assert_eq!(before.dark_square, after.dark_square);
        assert_eq!(before.selected_light_square, after.selected_light_square);
        assert_eq!(before.selected_dark_square, after.selected_dark_square);
    }
}

pub type ChessBtn = fn(&iced::Theme, iced::widget::button::Status) -> button::Style;
pub fn btn_style_simple(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    match status {
        button::Status::Disabled => button::Style {
            background: Some(iced::Background::Color(palette.background.stronger.color)),
            text_color: palette.background.stronger.text,
            border: Border {
                color: palette.primary.base.color,
                width: 1.,
                radius: 0.3.into(),
            },
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(iced::Background::Color(palette.success.strong.color)),
            text_color: palette.success.strong.text,
            border: Border {
                color: palette.primary.weak.color,
                width: 1.,
                radius: 0.3.into(),
            },
            ..Default::default()
        },
        _ => button::Style {
            background: Some(iced::Background::Color(palette.primary.base.color)),
            text_color: palette.primary.base.text,
            border: Border {
                color: palette.success.strong.color,
                width: 1.,
                radius: 0.3.into(),
            },
            ..Default::default()
        },
    }
}

pub fn btn_style_light_square(theme: &iced::Theme, _status: iced::widget::button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: Some(iced::Background::Color(palette.primary)),
        text_color: rgb!(45., 45., 45.),
        ..Default::default()
    }
}

pub fn btn_style_dark_square(theme: &iced::Theme, _status: iced::widget::button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: Some(iced::Background::Color(palette.success)),
        text_color: rgb!(45., 45., 45.),
        ..Default::default()
    }
}

pub fn btn_style_paper(_theme: &iced::Theme, _status: iced::widget::button::Status) -> button::Style {
    //let palette = theme.palette();
    button::Style {
        background: Some(iced::Background::Color(rgb!(245., 245., 245.))),
        text_color: rgb!(45., 45., 45.),
        border: Border {
            color: iced::Color::BLACK,
            width: 0.,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardSquareStyle {
    Light,
    Dark,
    SelectedLight,
    SelectedDark,
    Paper,
}

pub fn board_button_style(
    board_theme: BoardTheme,
    square_style: BoardSquareStyle,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, _status| {
        let palette = board_theme.board_palette();
        let background = match square_style {
            BoardSquareStyle::Light => palette.light_square,
            BoardSquareStyle::Dark => palette.dark_square,
            BoardSquareStyle::SelectedLight => palette.selected_light_square,
            BoardSquareStyle::SelectedDark => palette.selected_dark_square,
            BoardSquareStyle::Paper => rgb!(245., 245., 245.),
        };

        button::Style {
            background: Some(iced::Background::Color(background)),
            text_color: rgb!(45., 45., 45.),
            ..Default::default()
        }
    }
}

// I just copied over the default styles from Iced and made a few tweaks
pub fn checkbox_style(theme: &iced::Theme, status: checkbox::Status) -> checkbox::Style {
    let palette = theme.extended_palette();
    match status {
        checkbox::Status::Active { is_checked } => styled(
            palette.success.strong.color,
            palette.primary.base,
            palette.success.strong.color,
            palette.primary.base,
            is_checked,
        ),
        checkbox::Status::Hovered { is_checked } => styled(
            palette.primary.base.color,
            palette.success.strong,
            palette.primary.base.color,
            palette.success.strong,
            is_checked,
        ),
        checkbox::Status::Disabled { is_checked } => styled(
            palette.background.weak.color,
            palette.background.weak,
            palette.danger.base.text,
            palette.primary.strong,
            is_checked,
        ),
    }
}

fn styled(
    border_color: Color,
    base: iced::theme::palette::Pair,
    icon_color: Color,
    // I'm not using those, but leaving it ready in case I need
    _accent: iced::theme::palette::Pair,
    _is_checked: bool,
) -> checkbox::Style {
    checkbox::Style {
        background: iced::Background::Color(base.color),
        icon_color,
        border: Border {
            radius: 2.0.into(),
            width: 1.0,
            color: border_color,
        },
        text_color: None,
    }
}

pub fn radio_style(theme: &iced::Theme, status: radio::Status) -> radio::Style {
    let palette = theme.extended_palette();

    let active = radio::Style {
        background: iced::Background::Color(palette.primary.base.color),
        dot_color: palette.success.strong.color,
        border_width: 1.0,
        border_color: palette.success.strong.color,
        text_color: None,
    };

    match status {
        radio::Status::Active { .. } => active,
        radio::Status::Hovered { .. } => radio::Style {
            background: palette.primary.strong.color.into(),
            ..active
        },
    }
}

pub fn slider_style(theme: &iced::Theme, status: slider::Status) -> slider::Style {
    let palette = theme.extended_palette();

    let color = match status {
        slider::Status::Active => palette.primary.base.color,
        slider::Status::Hovered => palette.primary.strong.color,
        slider::Status::Dragged => palette.primary.weak.color,
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (color.into(), palette.background.strong.color.into()),
            width: 5.0,
            border: Border {
                radius: 2.0.into(),
                width: 1.0,
                color: palette.success.strong.color,
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 8.0 },
            background: palette.success.strong.color.into(),
            border_color: color,
            border_width: 2.0,
        },
    }
}

pub fn _container_style_paper(_theme: &iced::Theme) -> container::Style {
    //let palette = theme.palette();
    container::Style {
        background: Some(iced::Background::Color(rgb!(245., 245., 245.))),
        text_color: Some(rgb!(45., 45., 45.)),
        border: Border {
            color: iced::Color::BLACK,
            width: 0.,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn board_container_style(
    board_theme: BoardTheme,
    square_style: BoardSquareStyle,
) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let palette = board_theme.board_palette();
        let background = match square_style {
            BoardSquareStyle::Light => palette.light_square,
            BoardSquareStyle::Dark => palette.dark_square,
            BoardSquareStyle::SelectedLight => palette.selected_light_square,
            BoardSquareStyle::SelectedDark => palette.selected_dark_square,
            BoardSquareStyle::Paper => rgb!(245., 245., 245.),
        };

        container::Style {
            background: Some(iced::Background::Color(background)),
            text_color: Some(rgb!(45., 45., 45.)),
            ..Default::default()
        }
    }
}

pub fn tab_style(theme: &iced::Theme, status: iced_aw::style::Status) -> tab_bar::Style {
    let palette = theme.extended_palette();
    let dark_interface = theme.palette().background == InterfaceTheme::Dark.palette().background;
    let text_color = |background: iced::theme::palette::Pair, light_color: Color| {
        if dark_interface {
            background.text
        } else {
            light_color
        }
    };

    match status {
        iced_aw::style::Status::Active => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.success.base.color),
            background: Some(iced::Background::Color(palette.success.base.color)),
            text_color: text_color(palette.success.base, Color::WHITE),
            ..Default::default()
        },
        iced_aw::style::Status::Selected => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.primary.base.color),
            background: Some(iced::Background::Color(palette.primary.base.color)),
            text_color: text_color(palette.primary.base, Color::WHITE),
            ..Default::default()
        },
        iced_aw::style::Status::Focused => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.primary.base.color),
            background: Some(iced::Background::Color(palette.primary.base.color)),
            text_color: text_color(palette.primary.base, Color::WHITE),
            ..Default::default()
        },
        iced_aw::style::Status::Hovered => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.success.strong.color),
            background: Some(iced::Background::Color(palette.success.strong.color)),
            text_color: text_color(palette.success.strong, Color::WHITE),
            ..Default::default()
        },
        iced_aw::style::Status::Pressed => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.success.weak.color),
            background: Some(iced::Background::Color(palette.success.weak.color)),
            text_color: text_color(palette.success.weak, Color::WHITE),
            ..Default::default()
        },
        iced_aw::style::Status::Disabled => tab_bar::Style {
            tab_label_background: iced::Background::Color(palette.primary.base.color),
            background: Some(iced::Background::Color(palette.primary.base.color)),
            text_color: text_color(palette.primary.base, rgb!(45., 45., 45.)),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tab_style_tests {
    use super::*;

    fn interface_theme(theme: InterfaceTheme) -> iced::Theme {
        iced::Theme::custom("test", theme.palette())
    }

    #[test]
    fn dark_tab_text_uses_readable_palette_text_for_all_states() {
        let theme = interface_theme(InterfaceTheme::Dark);
        let palette = theme.extended_palette();
        let cases = [
            (iced_aw::style::Status::Active, palette.success.base.text),
            (iced_aw::style::Status::Selected, palette.primary.base.text),
            (iced_aw::style::Status::Focused, palette.primary.base.text),
            (iced_aw::style::Status::Hovered, palette.success.strong.text),
            (iced_aw::style::Status::Pressed, palette.success.weak.text),
            (iced_aw::style::Status::Disabled, palette.primary.base.text),
        ];

        for (status, expected_text) in cases {
            assert_eq!(tab_style(&theme, status).text_color, expected_text);
        }
    }

    #[test]
    fn light_tab_text_colors_remain_unchanged() {
        let theme = interface_theme(InterfaceTheme::Light);

        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Active).text_color,
            Color::WHITE
        );
        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Selected).text_color,
            Color::WHITE
        );
        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Focused).text_color,
            Color::WHITE
        );
        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Hovered).text_color,
            Color::WHITE
        );
        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Pressed).text_color,
            Color::WHITE
        );
        assert_eq!(
            tab_style(&theme, iced_aw::style::Status::Disabled).text_color,
            rgb!(45., 45., 45.)
        );
    }
}

pub fn pick_list_style(theme: &iced::Theme, status: iced::widget::pick_list::Status) -> pick_list::Style {
    let palette = theme.extended_palette();

    let (bg, text) = match status {
        pick_list::Status::Hovered => (palette.success.strong.color, palette.success.strong.text),
        _ => (palette.primary.base.color, palette.primary.base.text),
    };
    pick_list::Style {
        text_color: text, //palette.danger.base.color,
        placeholder_color: palette.success.weak.color,
        handle_color: palette.success.strong.color,
        background: iced::Background::Color(bg),
        border: Border {
            color: palette.success.strong.color,
            width: 1.,
            radius: 0.3.into(),
        },
    }
}

pub fn menu_style(theme: &iced::Theme) -> menu::Style {
    let palette = theme.extended_palette();

    menu::Style {
        background: iced::Background::Color(palette.primary.base.color),
        border: Border {
            color: palette.success.strong.color,
            width: 1.,
            radius: 0.3.into(),
        },
        selected_background: iced::Background::Color(palette.success.base.color),
        selected_text_color: palette.success.base.text,
        text_color: palette.primary.base.text,
        shadow: iced::Shadow::default(),
    }
}

/// Board-square colors, intentionally independent from the application UI palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardPalette {
    pub light_square: Color,
    pub dark_square: Color,
    pub selected_light_square: Color,
    pub selected_dark_square: Color,
}

impl BoardPalette {
    pub const BLUE: Self = Self {
        light_square: rgb!(235.0, 249.0, 255),
        dark_square: rgb!(110.0, 174.0, 213.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };

    pub const BLUE_DARK: Self = Self {
        light_square: rgb!(235.0, 249.0, 255),
        dark_square: rgb!(110.0, 174.0, 213.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const RED: Self = Self {
        light_square: rgb!(249.0, 234.0, 246),
        dark_square: rgb!(230.0, 133.0, 141.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };

    pub const RED_DARK: Self = Self {
        light_square: rgb!(249.0, 234.0, 246),
        dark_square: rgb!(230.0, 133.0, 141.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const GREEN_DARK: Self = Self {
        light_square: rgb!(238.0, 240.0, 203.0),
        dark_square: rgb!(136.0, 161.0, 111.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const BROWN_DARK: Self = Self {
        light_square: rgb!(241., 221., 186.),
        dark_square: rgb!(186., 142., 107.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const PURPLE_DARK: Self = Self {
        light_square: rgb!(233., 223., 242.),
        dark_square: rgb!(162., 136., 188.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const MONOCHROME_GREY_DARK: Self = Self {
        light_square: rgb!(235., 235., 235.),
        dark_square: rgb!(155., 155., 155.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const GREY_DARK: Self = Self {
        light_square: rgb!(222., 227., 230.),
        dark_square: rgb!(140., 162., 173.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const GREEN: Self = Self {
        light_square: rgb!(238.0, 240.0, 203.0),
        dark_square: rgb!(136.0, 161.0, 111.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const BROWN: Self = Self {
        light_square: rgb!(241., 221., 186.),
        dark_square: rgb!(186., 142., 107.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const PURPLE: Self = Self {
        light_square: rgb!(233., 223., 242.),
        dark_square: rgb!(162., 136., 188.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const MONOCHROME_GREY: Self = Self {
        light_square: rgb!(235., 235., 235.),
        dark_square: rgb!(155., 155., 155.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const GREY: Self = Self {
        light_square: rgb!(222., 227., 230.),
        dark_square: rgb!(140., 162., 173.),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const TRANS: Self = Self {
        light_square: rgb!(252.0, 252.0, 252.0),
        dark_square: rgb!(245.0, 183.0, 195.0),
        selected_light_square: rgb!(205, 210, 106),
        selected_dark_square: rgb!(170, 162, 58),
    };
    pub const ENBY: Self = Self {
        light_square: rgb!(246.0, 246.0, 246.0),
        dark_square: rgb!(211.0, 192.0, 80.0),
        selected_light_square: rgb!(172.0, 131.0, 191.0),
        selected_dark_square: rgb!(132.0, 91.0, 151.0),
    };
}
