use crate::types::Card;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DecklistEntry {
    pub card_name: String,
    pub count: u32,
}

#[derive(Debug, Clone)]
pub struct MatchedCard {
    pub card_name: String,
    pub count: u32,
    pub matched_path: PathBuf,
    pub confidence: f32,
}

#[derive(Serialize, Deserialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Serialize, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Serialize, Deserialize)]
struct CardMatching {
    matches: Vec<CardMatch>,
}

#[derive(Serialize, Deserialize)]
struct CardMatch {
    card_name: String,
    filename: String,
    confidence: f32,
}

pub struct DecklistManager {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl DecklistManager {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
        }
    }

    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = Some(api_key);
    }

    /// Parse decklist text into entries
    pub fn parse_decklist(&self, decklist_text: &str) -> Result<Vec<DecklistEntry>> {
        let mut entries = Vec::new();

        for line in decklist_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue; // Skip empty lines and comments
            }

            // Try to parse different decklist formats:
            // Format 1: "4 Lightning Bolt"
            // Format 2: "4x Lightning Bolt"
            // Format 3: "Lightning Bolt x4"
            // Format 4: "Lightning Bolt (4)"

            let entry = if let Some((count_str, name)) = line.split_once(' ') {
                // Format 1: "4 Lightning Bolt"
                if let Ok(count) = count_str.parse::<u32>() {
                    Some(DecklistEntry {
                        card_name: name.trim().to_string(),
                        count,
                    })
                } else if count_str.ends_with('x') {
                    // Format 2: "4x Lightning Bolt"
                    let count_str = count_str.trim_end_matches('x');
                    if let Ok(count) = count_str.parse::<u32>() {
                        Some(DecklistEntry {
                            card_name: name.trim().to_string(),
                            count,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(entry) = entry {
                entries.push(entry);
                continue;
            }

            // Format 3: "Lightning Bolt x4"
            if let Some((name, count_part)) = line.rsplit_once(" x") {
                if let Ok(count) = count_part.parse::<u32>() {
                    entries.push(DecklistEntry {
                        card_name: name.trim().to_string(),
                        count,
                    });
                    continue;
                }
            }

            // Format 4: "Lightning Bolt (4)"
            if line.ends_with(')') {
                if let Some(open_paren) = line.rfind('(') {
                    let name = line[..open_paren].trim();
                    let count_str = &line[open_paren + 1..line.len() - 1];
                    if let Ok(count) = count_str.parse::<u32>() {
                        entries.push(DecklistEntry {
                            card_name: name.to_string(),
                            count,
                        });
                        continue;
                    }
                }
            }

            // If we can't parse count, assume 1 copy
            entries.push(DecklistEntry {
                card_name: line.to_string(),
                count: 1,
            });
        }

        Ok(entries)
    }

    /// Match card names to image files using AI
    pub async fn match_cards_to_files(
        &self,
        decklist_entries: &[DecklistEntry],
        available_cards: &[Card],
    ) -> Result<Vec<MatchedCard>> {
        let api_key = self.api_key.as_ref()
            .ok_or_else(|| anyhow!("OpenAI API key not set. Set OPENAI_API_KEY environment variable or use set_api_key()"))?;

        // Extract filenames from available cards
        let filenames: Vec<String> = available_cards
            .iter()
            .filter_map(|card| {
                card.path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        if filenames.is_empty() {
            return Ok(vec![]);
        }

        let card_names: Vec<String> = decklist_entries
            .iter()
            .map(|entry| entry.card_name.clone())
            .collect();

        let prompt = format!(
            r#"You are helping match Trading Card Game card names from a decklist to image filenames. 

Card names from decklist: {}

Available image filenames: {}

Please match each card name to the most likely filename. The filenames might be abbreviated, have different capitalization, use underscores/hyphens instead of spaces, or have set codes/numbers appended.

Return your response as valid JSON in this exact format:
{{
  "matches": [
    {{
      "card_name": "Lightning Bolt",
      "filename": "lightning_bolt_001", 
      "confidence": 0.95
    }}
  ]
}}

Only include matches where you're reasonably confident (confidence > 0.5). Use confidence values between 0.5 and 1.0 where 1.0 is a perfect match."#,
            serde_json::to_string(&card_names)?,
            serde_json::to_string(&filenames)?
        );

        let request = OpenAIRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            temperature: 0.1,
        };

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        let openai_response: OpenAIResponse = response.json().await?;
        let content = &openai_response.choices[0].message.content;

        // Parse the JSON response
        let card_matching: CardMatching = serde_json::from_str(content).map_err(|e| {
            anyhow!(
                "Failed to parse AI response as JSON: {}. Response was: {}",
                e,
                content
            )
        })?;

        // Convert to MatchedCard format
        let mut matched_cards = Vec::new();
        let entry_map: HashMap<String, u32> = decklist_entries
            .iter()
            .map(|entry| (entry.card_name.clone(), entry.count))
            .collect();

        for card_match in card_matching.matches {
            if let Some(&count) = entry_map.get(&card_match.card_name) {
                // Find the actual path for this filename
                if let Some(card) = available_cards.iter().find(|c| {
                    c.path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(|s| s == card_match.filename)
                        .unwrap_or(false)
                }) {
                    matched_cards.push(MatchedCard {
                        card_name: card_match.card_name,
                        count,
                        matched_path: card.path.clone(),
                        confidence: card_match.confidence,
                    });
                }
            }
        }

        Ok(matched_cards)
    }

    /// Apply decklist to cards, reordering and setting copy counts
    pub fn apply_decklist_to_cards(
        &self,
        matched_cards: &[MatchedCard],
        available_cards: &mut Vec<Card>,
    ) -> Result<()> {
        // Create a map of paths to matched cards for quick lookup
        let matched_map: HashMap<PathBuf, &MatchedCard> = matched_cards
            .iter()
            .map(|mc| (mc.matched_path.clone(), mc))
            .collect();

        // Separate cards into matched and unmatched
        let mut matched_card_objects = Vec::new();
        let mut unmatched_cards = Vec::new();

        for card in available_cards.drain(..) {
            if let Some(matched) = matched_map.get(&card.path) {
                let mut updated_card = card;
                updated_card.set_copy_count(matched.count);
                matched_card_objects.push((matched, updated_card));
            } else {
                unmatched_cards.push(card);
            }
        }

        // Sort matched cards by the order they appear in the decklist
        matched_card_objects.sort_by_key(|(matched, _)| {
            matched_cards
                .iter()
                .position(|mc| mc.card_name == matched.card_name)
                .unwrap_or(usize::MAX)
        });

        // Rebuild the cards vector: matched cards first, then unmatched
        for (_, card) in matched_card_objects {
            available_cards.push(card);
        }
        available_cards.extend(unmatched_cards);

        Ok(())
    }
}

impl Default for DecklistManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_decklist_format_1() {
        let manager = DecklistManager::new();
        let decklist = "4 Lightning Bolt\n2 Shock\n1 Fireball";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].card_name, "Lightning Bolt");
        assert_eq!(entries[0].count, 4);
        assert_eq!(entries[1].card_name, "Shock");
        assert_eq!(entries[1].count, 2);
    }

    #[test]
    fn test_parse_decklist_format_2() {
        let manager = DecklistManager::new();
        let decklist = "4x Lightning Bolt\n2x Shock";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].card_name, "Lightning Bolt");
        assert_eq!(entries[0].count, 4);
    }

    #[test]
    fn test_parse_decklist_format_3() {
        let manager = DecklistManager::new();
        let decklist = "Lightning Bolt x4\nShock x2";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].card_name, "Lightning Bolt");
        assert_eq!(entries[0].count, 4);
    }

    #[test]
    fn test_parse_decklist_format_4() {
        let manager = DecklistManager::new();
        let decklist = "Lightning Bolt (4)\nShock (2)";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].card_name, "Lightning Bolt");
        assert_eq!(entries[0].count, 4);
    }

    #[test]
    fn test_parse_decklist_mixed_formats() {
        let manager = DecklistManager::new();
        let decklist = "4 Lightning Bolt\n2x Shock\nFireball x1\nCounterspell (3)\nPlain Card Name";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[4].card_name, "Plain Card Name");
        assert_eq!(entries[4].count, 1); // Default count
    }

    #[test]
    fn test_parse_decklist_with_comments() {
        let manager = DecklistManager::new();
        let decklist =
            "# This is a comment\n4 Lightning Bolt\n// Another comment\n2 Shock\n\n3 Fireball";

        let entries = manager.parse_decklist(decklist).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].card_name, "Lightning Bolt");
    }

    #[test]
    fn test_apply_decklist_to_cards() {
        let manager = DecklistManager::new();
        let mut cards = vec![
            Card::new(PathBuf::from("shock.jpg")),
            Card::new(PathBuf::from("lightning_bolt.jpg")),
        ];

        let matched_cards = vec![
            MatchedCard {
                card_name: "Lightning Bolt".to_string(),
                count: 4,
                matched_path: PathBuf::from("lightning_bolt.jpg"),
                confidence: 0.9,
            },
            MatchedCard {
                card_name: "Shock".to_string(),
                count: 2,
                matched_path: PathBuf::from("shock.jpg"),
                confidence: 0.8,
            },
        ];

        manager
            .apply_decklist_to_cards(&matched_cards, &mut cards)
            .unwrap();

        // Cards should be reordered to match decklist order
        assert_eq!(cards[0].path, PathBuf::from("lightning_bolt.jpg"));
        assert_eq!(cards[0].copy_count, 4);
        assert_eq!(cards[1].path, PathBuf::from("shock.jpg"));
        assert_eq!(cards[1].copy_count, 2);
    }
}
