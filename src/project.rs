use crate::types::{Card, LayoutParams};
use crate::ui::{CardSizeOption, PageSizeOption};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Extension used for saved project files, without the leading dot.
pub const PROJECT_FILE_EXTENSION: &str = "tcgproj";

fn default_copy_count() -> u32 {
    1
}

/// A card as saved in a project file: just enough to rebuild a `Card` on
/// load. Thumbnails, DPI, etc. are re-derived from disk, not persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCard {
    pub path: PathBuf,
    #[serde(default)]
    pub back_path: Option<PathBuf>,
    #[serde(default = "default_copy_count")]
    pub copy_count: u32,
}

impl From<&Card> for ProjectCard {
    fn from(card: &Card) -> Self {
        Self {
            path: card.path.clone(),
            back_path: card.back_path.clone(),
            copy_count: card.copy_count,
        }
    }
}

impl ProjectCard {
    /// Rebuilds a `Card`, reading DPI from disk. If the file has moved or
    /// been deleted this still succeeds; the thumbnail request made
    /// afterwards is what surfaces the failure (`ThumbnailState::Failed`).
    pub fn to_card(&self) -> Card {
        let mut card = Card::new(self.path.clone());
        card.back_path = self.back_path.clone();
        card.set_copy_count(self.copy_count);
        card
    }
}

/// The full saved state of a project: layout configuration plus the card
/// list. Deliberately holds no image data, only file paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub layout_params: LayoutParams,
    pub page_size_option: PageSizeOption,
    pub card_size_option: CardSizeOption,
    #[serde(default)]
    pub cards: Vec<ProjectCard>,
}

impl Project {
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let project = serde_json::from_str(&contents)?;
        Ok(project)
    }
}

/// The project file's name without extension, used as a window-title /
/// display label. Falls back to the full path if it has no file stem.
pub fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_project() -> Project {
        Project {
            layout_params: LayoutParams::default(),
            page_size_option: PageSizeOption::A4,
            card_size_option: CardSizeOption::Poker,
            cards: vec![
                ProjectCard {
                    path: PathBuf::from("front1.png"),
                    back_path: Some(PathBuf::from("back1.png")),
                    copy_count: 3,
                },
                ProjectCard {
                    path: PathBuf::from("front2.png"),
                    back_path: None,
                    copy_count: 1,
                },
            ],
        }
    }

    #[test]
    fn test_project_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.tcgproj");

        let project = sample_project();
        project.save_to_file(&path).unwrap();

        let loaded = Project::load_from_file(&path).unwrap();
        assert_eq!(loaded.layout_params, project.layout_params);
        assert_eq!(loaded.page_size_option, project.page_size_option);
        assert_eq!(loaded.card_size_option, project.card_size_option);
        assert_eq!(loaded.cards, project.cards);
    }

    #[test]
    fn test_project_card_from_card_and_back() {
        let mut card = Card::new(PathBuf::from("front.png"));
        card.back_path = Some(PathBuf::from("back.png"));
        card.set_copy_count(4);

        let project_card = ProjectCard::from(&card);
        assert_eq!(project_card.path, card.path);
        assert_eq!(project_card.back_path, card.back_path);
        assert_eq!(project_card.copy_count, 4);

        let rebuilt = project_card.to_card();
        assert_eq!(rebuilt.path, card.path);
        assert_eq!(rebuilt.back_path, card.back_path);
        assert_eq!(rebuilt.copy_count, card.copy_count);
    }

    #[test]
    fn test_project_cards_default_when_absent() {
        // Backwards compat: a hand-written project file without a `cards`
        // array should still load, as an empty project.
        let mut value = serde_json::to_value(sample_project()).unwrap();
        value.as_object_mut().unwrap().remove("cards");

        let loaded: Project = serde_json::from_value(value).unwrap();
        assert!(loaded.cards.is_empty());
    }

    #[test]
    fn test_load_missing_file_errors() {
        let result = Project::load_from_file(Path::new("/nonexistent/path/x.tcgproj"));
        assert!(result.is_err());
    }

    #[test]
    fn test_display_name() {
        assert_eq!(
            display_name(Path::new("/foo/bar/My Deck.tcgproj")),
            "My Deck"
        );
        assert_eq!(display_name(Path::new("no_extension")), "no_extension");
    }

    #[test]
    fn test_project_card_default_copy_count_when_absent() {
        let mut value = serde_json::to_value(ProjectCard {
            path: PathBuf::from("a.png"),
            back_path: None,
            copy_count: 5,
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("copy_count");
        value.as_object_mut().unwrap().remove("back_path");

        let loaded: ProjectCard = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.copy_count, 1);
        assert_eq!(loaded.back_path, None);
    }
}
