pub mod card_list_panel;
pub mod decklist_panel;
pub mod parameters_panel;
pub mod preview_panel;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PageSizeOption {
    A4,
    USLetter,
    A3,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CardSizeOption {
    Poker,
    Bridge,
    Tarot,
    Custom,
}

impl PageSizeOption {
    pub fn get_size(&self) -> Option<(f32, f32)> {
        match self {
            PageSizeOption::A4 => Some((210.0, 297.0)),
            PageSizeOption::USLetter => Some((215.9, 279.4)),
            PageSizeOption::A3 => Some((297.0, 420.0)),
            PageSizeOption::Custom => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            PageSizeOption::A4 => "A4 (210 × 297 mm)",
            PageSizeOption::USLetter => "US Letter (215.9 × 279.4 mm)",
            PageSizeOption::A3 => "A3 (297 × 420 mm)",
            PageSizeOption::Custom => "Custom",
        }
    }
}

impl CardSizeOption {
    pub fn get_size(&self) -> Option<(f32, f32)> {
        match self {
            CardSizeOption::Poker => Some((63.0, 88.0)),
            CardSizeOption::Bridge => Some((56.0, 87.0)),
            CardSizeOption::Tarot => Some((70.0, 120.0)),
            CardSizeOption::Custom => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CardSizeOption::Poker => "Poker Card (63 × 88 mm)",
            CardSizeOption::Bridge => "Bridge Card (56 × 87 mm)",
            CardSizeOption::Tarot => "Tarot Card (70 × 120 mm)",
            CardSizeOption::Custom => "Custom",
        }
    }
}
