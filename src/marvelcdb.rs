use crate::decklist::MatchedCard;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

const MARVELCDB_API_BASE: &str = "https://marvelcdb.com/api/public";

// --- API Types ---

#[derive(Debug, Deserialize)]
pub struct MarvelCdbDecklist {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub hero_code: Option<String>,
    #[serde(default)]
    pub hero_name: Option<String>,
    pub slots: HashMap<String, u32>,
    #[serde(default)]
    pub meta: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MarvelCdbCard {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub type_code: String,
    #[serde(default)]
    pub faction_code: String,
    #[serde(default)]
    pub pack_code: String,
    #[serde(default)]
    pub linked_card: Option<Box<MarvelCdbCard>>,
}

// --- Result Types ---

#[derive(Debug, Clone)]
pub struct MarvelCdbResult {
    pub deck_name: String,
    pub hero_name: String,
    pub matched_cards: Vec<MatchedCard>,
    pub unmatched_cards: Vec<UnmatchedCard>,
}

#[derive(Debug, Clone)]
pub struct UnmatchedCard {
    pub name: String,
    pub count: u32,
    pub faction: String,
    pub pack_code: String,
}

#[derive(Debug, Clone)]
pub struct FetchedCard {
    pub code: String,
    pub name: String,
    pub count: u32,
    pub faction_code: String,
    pub pack_code: String,
}

#[derive(Debug, Clone)]
pub struct FetchedDeck {
    pub deck_name: String,
    pub hero_name: String,
    pub cards: Vec<FetchedCard>,
}

// --- Messages ---

#[derive(Debug)]
pub enum MarvelCdbMessage {
    Started,
    Progress(String),
    DeckFetched {
        deck_name: String,
        hero_name: String,
    },
    Completed(MarvelCdbResult),
    Failed(String),
}

// --- URL Parsing ---

pub fn parse_marvelcdb_input(input: &str) -> Result<u64> {
    let input = input.trim();

    // Try direct numeric parse
    if let Ok(id) = input.parse::<u64>() {
        return Ok(id);
    }

    // Try to extract from URL
    // Handles: https://marvelcdb.com/decklist/view/12345/slug-name
    let segments: Vec<&str> = input.split('/').collect();
    for (i, segment) in segments.iter().enumerate() {
        if (*segment == "decklist" || *segment == "deck") && i + 2 < segments.len() {
            // The ID is typically 2 segments after "decklist" (view/12345)
            if let Ok(id) = segments[i + 2].parse::<u64>() {
                return Ok(id);
            }
        }
    }

    // Also try: the last numeric segment in the URL
    for segment in segments.iter().rev() {
        // Strip any non-numeric suffix (e.g., "12345/slug" -> try "12345")
        if let Ok(id) = segment.parse::<u64>() {
            return Ok(id);
        }
    }

    Err(anyhow!(
        "Could not parse MarvelCDB deck ID from input: {input}"
    ))
}

// --- API Fetching ---

async fn fetch_decklist(client: &reqwest::Client, deck_id: u64) -> Result<MarvelCdbDecklist> {
    let url = format!("{MARVELCDB_API_BASE}/decklist/{deck_id}");
    let response = client.get(&url).send().await?.error_for_status()?;
    let decklist: MarvelCdbDecklist = response.json().await?;
    Ok(decklist)
}

async fn fetch_card(client: &reqwest::Client, code: &str) -> Result<MarvelCdbCard> {
    let url = format!("{MARVELCDB_API_BASE}/card/{code}");
    let response = client.get(&url).send().await?.error_for_status()?;
    let card: MarvelCdbCard = response.json().await?;
    Ok(card)
}

async fn fetch_hero_signature_cards(
    client: &reqwest::Client,
    hero_code: &str,
) -> Result<Vec<MarvelCdbCard>> {
    // Hero code is like "01001a" - extract pack code prefix (numeric part)
    // Hero signature cards share the same pack and have faction_code == "hero"
    let hero_card = fetch_card(client, hero_code).await?;
    let pack_code = &hero_card.pack_code;

    // Fetch all cards in the pack
    let url = format!("{MARVELCDB_API_BASE}/cards/{pack_code}");
    let response = client.get(&url).send().await?.error_for_status()?;
    let all_cards: Vec<MarvelCdbCard> = response.json().await?;

    // Filter to hero faction cards (signature cards)
    let signature_cards: Vec<MarvelCdbCard> = all_cards
        .into_iter()
        .filter(|c| c.faction_code == "hero" && c.code != hero_code)
        .collect();

    Ok(signature_cards)
}

// --- Card Name Parsing ---

// Known card type suffixes to strip from filenames
const CARD_TYPE_SUFFIXES: &[&str] = &[
    "Event",
    "Upgrade",
    "Ally",
    "Support",
    "Resource",
    "Attachment",
    "Obligation",
    "Alter-Ego",
    "Hero",
    "Minion",
    "Treachery",
    "Player Side Scheme",
    "Side Scheme",
    "Environment",
    "Permanent",
];

pub(crate) fn parse_card_name_from_filename(
    file_path: &Path,
    context_prefix: Option<&str>,
) -> String {
    let stem = file_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let mut name = stem;

    // Strip context prefix if present (aspect name or hero name from directory)
    if let Some(prefix) = context_prefix {
        // Try stripping with various separators
        for sep in ["_", " ", "-"] {
            let full_prefix = format!("{prefix}{sep}");
            if let Some(stripped) = name.strip_prefix(&full_prefix) {
                name = stripped.to_string();
                break;
            }
        }
        // Also try the hero name extracted from directory
        if let Some((_civilian, hero)) = prefix.split_once('_') {
            for sep in ["_", " ", "-"] {
                let hero_prefix = format!("{hero}{sep}");
                if let Some(stripped) = name.strip_prefix(&hero_prefix) {
                    name = stripped.to_string();
                    break;
                }
            }
        }
    }

    // Strip trailing numbers first (e.g., "_01", " 2") before suffix stripping
    name = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches('_')
        .trim_end_matches('-')
        .trim_end_matches(' ')
        .to_string();

    // Strip known card type suffixes
    for suffix in CARD_TYPE_SUFFIXES {
        for sep in ["_", " ", "-"] {
            let full_suffix = format!("{sep}{suffix}");
            if name.ends_with(&full_suffix) {
                name = name[..name.len() - full_suffix.len()].to_string();
                break;
            }
        }
    }

    // Strip trailing numbers again (in case suffix was between name and number)
    let name = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches('_')
        .trim_end_matches('-')
        .trim_end_matches(' ');

    // Decode _s -> 's
    let name = name.replace("_s ", "'s ").replace("_s_", "'s ");

    // Replace underscores/hyphens with spaces
    let name = name.replace(['_', '-'], " ");

    // Collapse whitespace
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");

    name
}

pub(crate) fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// --- Orchestrator ---

pub async fn fetch_deck(deck_input: &str) -> Result<FetchedDeck> {
    let deck_id = parse_marvelcdb_input(deck_input)?;
    let client = reqwest::Client::new();

    // Fetch the decklist
    let decklist = fetch_decklist(&client, deck_id).await?;
    let deck_name = decklist.name.clone();

    // Fetch card details for all slots
    let mut fetched_cards = Vec::new();
    for (code, count) in &decklist.slots {
        match fetch_card(&client, code).await {
            Ok(card) => {
                fetched_cards.push(FetchedCard {
                    code: card.code,
                    name: card.name,
                    count: *count,
                    faction_code: card.faction_code,
                    pack_code: card.pack_code,
                });
            }
            Err(e) => {
                log::warn!("Failed to fetch card {code}: {e}");
            }
        }
    }

    // Fetch hero signature cards if we have a hero code
    let hero_name = if let Some(hero_code) = &decklist.hero_code {
        match fetch_hero_signature_cards(&client, hero_code).await {
            Ok(sig_cards) => {
                let hero_card = fetch_card(&client, hero_code).await.ok();
                let name = hero_card
                    .as_ref()
                    .map(|c| c.name.clone())
                    .or(decklist.hero_name.clone())
                    .unwrap_or_default();

                // Add hero identity card itself
                if let Some(hc) = &hero_card {
                    fetched_cards.push(FetchedCard {
                        code: hc.code.clone(),
                        name: hc.name.clone(),
                        count: 1,
                        faction_code: "hero".to_string(),
                        pack_code: hc.pack_code.clone(),
                    });
                    // Add the alter-ego side if linked
                    if let Some(linked) = &hc.linked_card {
                        fetched_cards.push(FetchedCard {
                            code: linked.code.clone(),
                            name: linked.name.clone(),
                            count: 1,
                            faction_code: "hero".to_string(),
                            pack_code: linked.pack_code.clone(),
                        });
                    }
                }

                // Add signature cards
                for sig_card in sig_cards {
                    // Only add if not already in slots
                    if !decklist.slots.contains_key(&sig_card.code) {
                        fetched_cards.push(FetchedCard {
                            code: sig_card.code.clone(),
                            name: sig_card.name,
                            count: 1,
                            faction_code: "hero".to_string(),
                            pack_code: sig_card.pack_code,
                        });
                    }
                }

                name
            }
            Err(e) => {
                log::warn!("Failed to fetch hero signature cards: {e}");
                decklist.hero_name.clone().unwrap_or_default()
            }
        }
    } else {
        decklist.hero_name.clone().unwrap_or_default()
    };

    // Sort by card code so matched results come out in card number order
    fetched_cards.sort_by(|a, b| a.code.cmp(&b.code));

    Ok(FetchedDeck {
        deck_name,
        hero_name,
        cards: fetched_cards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_marvelcdb_input_numeric() {
        assert_eq!(parse_marvelcdb_input("12345").unwrap(), 12345);
    }

    #[test]
    fn test_parse_marvelcdb_input_url() {
        assert_eq!(
            parse_marvelcdb_input("https://marvelcdb.com/decklist/view/12345/my-cool-deck")
                .unwrap(),
            12345
        );
    }

    #[test]
    fn test_parse_marvelcdb_input_url_no_slug() {
        assert_eq!(
            parse_marvelcdb_input("https://marvelcdb.com/decklist/view/99999").unwrap(),
            99999
        );
    }

    #[test]
    fn test_parse_marvelcdb_input_invalid() {
        assert!(parse_marvelcdb_input("not-a-number").is_err());
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("Spider-Man"), "spiderman");
        assert_eq!(normalize_name("Captain America"), "captain america");
        assert_eq!(normalize_name("Nick Fury's Plan"), "nick furys plan");
    }

    #[test]
    fn test_parse_card_name_from_filename() {
        let path = Path::new("/images/Aspects/Aggression/Aggression_Uppercut_Event_01.tiff");
        let name = parse_card_name_from_filename(path, Some("Aggression"));
        assert_eq!(name, "Uppercut");
    }

    #[test]
    fn test_parse_card_name_possessive() {
        let path = Path::new("/images/heros/Tony_IronMan/IronMan_Pepper_s_Rescue_Ally.tiff");
        let name = parse_card_name_from_filename(path, Some("Tony_IronMan"));
        assert_eq!(name, "Pepper's Rescue");
    }
}
