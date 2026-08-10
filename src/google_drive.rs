use crate::decklist::MatchedCard;
use crate::marvelcdb::{
    FetchedCard, FetchedDeck, MarvelCdbMessage, MarvelCdbResult, UnmatchedCard,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3/files";

// --- API Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
struct DriveFileList {
    files: Vec<DriveFile>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

// --- Messages ---

#[derive(Debug)]
pub enum GoogleDriveMessage {
    IndexBuildProgress {
        folders_scanned: usize,
        total_folders: usize,
    },
    IndexBuildComplete {
        file_count: usize,
        updated_at: u64,
    },
    Failed(String),
}

// --- Client ---

pub struct GoogleDriveClient {
    client: reqwest::Client,
    api_key: String,
}

impl GoogleDriveClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    /// List all files in a Google Drive folder, handling pagination.
    pub async fn list_files_in_folder(&self, folder_id: &str) -> Result<Vec<DriveFile>> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut request = self.client.get(DRIVE_API_BASE).query(&[
                ("q", format!("'{folder_id}' in parents and trashed=false")),
                (
                    "fields",
                    "files(id,name,mimeType),nextPageToken".to_string(),
                ),
                ("pageSize", "1000".to_string()),
                ("key", self.api_key.clone()),
            ]);

            if let Some(token) = &page_token {
                request = request.query(&[("pageToken", token.as_str())]);
            }

            let response = request.send().await?.error_for_status()?;
            let file_list: DriveFileList = response.json().await?;

            all_files.extend(file_list.files);

            match file_list.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }

        Ok(all_files)
    }

    /// Download a file from Google Drive to a local path.
    pub async fn download_file(&self, file_id: &str, destination: &Path) -> Result<()> {
        let url = format!("{DRIVE_API_BASE}/{file_id}");
        let response = self
            .client
            .get(&url)
            .query(&[("alt", "media"), ("key", &self.api_key)])
            .send()
            .await?
            .error_for_status()?;

        let bytes = response.bytes().await?;

        // Ensure parent directory exists
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(destination, &bytes)?;
        Ok(())
    }

    /// Build a map of faction/folder name -> folder ID by traversing the known Drive structure.
    ///
    /// Expected structure under root:
    /// - Aspects/ -> Aggression/, Justice/, Leadership/, Protection/, Basic/
    /// - Heros/ -> {HeroName}/ folders
    pub async fn build_folder_map(&self, root_folder_id: &str) -> Result<FolderMap> {
        let mut map = FolderMap::default();

        let top_level = self.list_files_in_folder(root_folder_id).await?;

        for item in &top_level {
            if !is_folder(&item.mime_type) {
                continue;
            }

            let name_lower = item.name.to_lowercase();

            if name_lower == "aspects" || name_lower == "aspect" {
                // Traverse into Aspects to find sub-folders
                let aspect_folders = self.list_files_in_folder(&item.id).await?;
                for aspect in &aspect_folders {
                    if is_folder(&aspect.mime_type) {
                        let aspect_lower = aspect.name.to_lowercase();
                        map.aspect_folders.insert(aspect_lower, aspect.id.clone());
                    }
                }
            } else if name_lower == "heros" || name_lower == "heroes" || name_lower == "hero" {
                // Traverse into Heros to find hero folders
                let hero_folders = self.list_files_in_folder(&item.id).await?;
                for hero in &hero_folders {
                    if is_folder(&hero.mime_type) {
                        map.hero_folders.push(HeroFolder {
                            folder_id: hero.id.clone(),
                            folder_name: hero.name.clone(),
                            hero_name: extract_hero_name_from_drive(&hero.name),
                        });
                    }
                }
            }
        }

        Ok(map)
    }

    /// Build a complete index of all files in the Drive folder structure and save to disk.
    pub async fn build_index(
        &self,
        root_folder_id: &str,
        sender: &mpsc::Sender<GoogleDriveMessage>,
    ) -> Result<DriveIndex> {
        let folder_map = self.build_folder_map(root_folder_id).await?;

        // Collect folder IDs as owned strings to avoid borrow issues
        let all_ids: Vec<String> = folder_map
            .all_folder_ids()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let total = all_ids.len();
        let mut folder_files = HashMap::new();

        for (i, folder_id) in all_ids.iter().enumerate() {
            let _ = sender.send(GoogleDriveMessage::IndexBuildProgress {
                folders_scanned: i + 1,
                total_folders: total,
            });
            let files = self.list_files_in_folder(folder_id).await?;
            folder_files.insert(folder_id.clone(), files);
        }

        let index = DriveIndex {
            root_folder_id: root_folder_id.to_string(),
            folder_map,
            folder_files,
            updated_at: timestamp_now(),
        };

        index.save()?;

        let _ = sender.send(GoogleDriveMessage::IndexBuildComplete {
            file_count: index.file_count(),
            updated_at: index.updated_at,
        });

        Ok(index)
    }

    /// Match fetched cards against Drive index and download missing files.
    /// Returns a MarvelCdbResult with matched and unmatched cards.
    pub async fn match_and_download_cards(
        &self,
        fetched_deck: &FetchedDeck,
        images_dir: &Path,
        root_folder_id: &str,
        sender: &mpsc::Sender<MarvelCdbMessage>,
    ) -> Result<MarvelCdbResult> {
        let _ = sender.send(MarvelCdbMessage::Progress(
            "Loading Drive index...".to_string(),
        ));

        // Load cached index or auto-build if missing
        let (folder_map, mut folder_files_cache) = match DriveIndex::load() {
            Some(index) if index.root_folder_id == root_folder_id => {
                log::info!(
                    "Using cached Drive index ({} files in {} folders)",
                    index.file_count(),
                    index.folder_files.len()
                );
                (index.folder_map, index.folder_files)
            }
            _ => {
                log::info!("No cached index found, building from API...");
                let _ = sender.send(MarvelCdbMessage::Progress(
                    "Building Drive index...".to_string(),
                ));
                let map = self.build_folder_map(root_folder_id).await?;

                // Enumerate all folders
                let all_ids: Vec<String> = map
                    .all_folder_ids()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                let total = all_ids.len();
                let mut folder_files = HashMap::new();

                for (i, folder_id) in all_ids.iter().enumerate() {
                    let _ = sender.send(MarvelCdbMessage::Progress(format!(
                        "Indexing folders... {}/{}",
                        i + 1,
                        total
                    )));
                    let files = self.list_files_in_folder(folder_id).await?;
                    folder_files.insert(folder_id.clone(), files);
                }

                (map, folder_files)
            }
        };

        let _ = sender.send(MarvelCdbMessage::Progress(
            "Matching cards against Drive...".to_string(),
        ));

        let mut matched_cards = Vec::new();
        let mut unmatched_cards = Vec::new();
        let total = fetched_deck.cards.len();

        for (i, card) in fetched_deck.cards.iter().enumerate() {
            let _ = sender.send(MarvelCdbMessage::Progress(format!(
                "Matching {}/{}: {}",
                i + 1,
                total,
                card.name
            )));

            match self
                .match_single_card(
                    card,
                    &fetched_deck.hero_name,
                    images_dir,
                    &folder_map,
                    &mut folder_files_cache,
                )
                .await
            {
                Some(matched) => matched_cards.push(matched),
                None => unmatched_cards.push(UnmatchedCard {
                    name: card.name.clone(),
                    count: card.count,
                    faction: card.faction_code.clone(),
                    pack_code: card.pack_code.clone(),
                }),
            }
        }

        // Save updated index
        let index = DriveIndex {
            root_folder_id: root_folder_id.to_string(),
            folder_map,
            folder_files: folder_files_cache,
            updated_at: timestamp_now(),
        };
        if let Err(e) = index.save() {
            log::warn!("Failed to save Drive index: {e}");
        }

        Ok(MarvelCdbResult {
            deck_name: fetched_deck.deck_name.clone(),
            hero_name: fetched_deck.hero_name.clone(),
            matched_cards,
            unmatched_cards,
        })
    }

    /// Try to match a single card against the Drive index, downloading if needed.
    async fn match_single_card(
        &self,
        card: &FetchedCard,
        hero_name: &str,
        images_dir: &Path,
        folder_map: &FolderMap,
        folder_files_cache: &mut HashMap<String, Vec<DriveFile>>,
    ) -> Option<MatchedCard> {
        let normalized_card_name = crate::marvelcdb::normalize_name(&card.name);

        // Determine which folders to search based on faction + pack_code
        let folder_ids =
            folder_map.folders_for_faction(&card.faction_code, hero_name, &card.pack_code);

        // Targeted search (0.85 threshold)
        for folder_id in &folder_ids {
            let files = self.get_or_fetch_files(folder_id, folder_files_cache).await;
            let files = match files {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("Failed to list files in folder {folder_id}: {e}");
                    continue;
                }
            };

            let ctx = MatchContext {
                faction: &card.faction_code,
                hero_name,
                images_dir,
                folder_map,
                folder_id,
                min_score: 0.85,
            };
            if let Some((drive_file, dest_path, score)) =
                find_best_drive_match(&normalized_card_name, &files, &ctx)
            {
                return self
                    .ensure_local_and_match(card, &drive_file, &dest_path, score)
                    .await;
            }
        }

        // Exhaustive fallback (0.0 threshold)
        log::info!(
            "Targeted search failed for '{}', trying all folders...",
            card.name
        );
        let all_ids = folder_map.all_folder_ids();
        let mut global_best: Option<(DriveFile, PathBuf, f32, String)> = None;

        for folder_id in &all_ids {
            if folder_ids.contains(folder_id) {
                continue;
            }

            let files = self.get_or_fetch_files(folder_id, folder_files_cache).await;
            let files = match files {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("Failed to list files in folder {folder_id}: {e}");
                    continue;
                }
            };

            let ctx = MatchContext {
                faction: &card.faction_code,
                hero_name,
                images_dir,
                folder_map,
                folder_id,
                min_score: 0.0,
            };
            if let Some((file, path, score)) =
                find_best_drive_match(&normalized_card_name, &files, &ctx)
            {
                if global_best
                    .as_ref()
                    .is_none_or(|(_, _, best_score, _)| score > *best_score)
                {
                    global_best = Some((file, path, score, (*folder_id).to_string()));
                }
            }
        }

        if let Some((drive_file, dest_path, score, _folder_id)) = global_best {
            return self
                .ensure_local_and_match(card, &drive_file, &dest_path, score)
                .await;
        }

        log::info!(
            "No match found on Drive for '{}' (exhaustive search)",
            card.name
        );
        None
    }

    /// Get files from cache or fetch from API.
    async fn get_or_fetch_files(
        &self,
        folder_id: &str,
        cache: &mut HashMap<String, Vec<DriveFile>>,
    ) -> Result<Vec<DriveFile>> {
        if let Some(cached) = cache.get(folder_id) {
            return Ok(cached.clone());
        }
        let fetched = self.list_files_in_folder(folder_id).await?;
        cache.insert(folder_id.to_string(), fetched.clone());
        Ok(fetched)
    }

    /// Check if file exists locally, download if not, return MatchedCard.
    async fn ensure_local_and_match(
        &self,
        card: &FetchedCard,
        drive_file: &DriveFile,
        dest_path: &Path,
        score: f32,
    ) -> Option<MatchedCard> {
        if dest_path.exists() {
            log::info!(
                "File already exists for '{}': {}",
                card.name,
                dest_path.display()
            );
            return Some(MatchedCard {
                card_name: card.name.clone(),
                count: card.count,
                matched_path: dest_path.to_path_buf(),
                confidence: score,
            });
        }

        match self.download_file(&drive_file.id, dest_path).await {
            Ok(()) => {
                log::info!(
                    "Downloaded '{}' (score={:.2}) -> {}",
                    card.name,
                    score,
                    dest_path.display()
                );
                Some(MatchedCard {
                    card_name: card.name.clone(),
                    count: card.count,
                    matched_path: dest_path.to_path_buf(),
                    confidence: score,
                })
            }
            Err(e) => {
                log::warn!("Failed to download '{}': {e}", card.name);
                None
            }
        }
    }
}

// --- Folder Map ---

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FolderMap {
    /// aspect name (lowercase) -> folder ID (e.g., "aggression" -> "abc123")
    pub aspect_folders: HashMap<String, String>,
    /// Hero folders
    pub hero_folders: Vec<HeroFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroFolder {
    pub folder_id: String,
    pub folder_name: String,
    pub hero_name: String,
}

// --- Drive Index (persistent cache) ---

fn drive_index_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not find config directory"))?
        .join("tcg_layout");
    Ok(config_dir.join("drive_index.json"))
}

fn timestamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveIndex {
    pub root_folder_id: String,
    pub folder_map: FolderMap,
    pub folder_files: HashMap<String, Vec<DriveFile>>,
    pub updated_at: u64,
}

impl DriveIndex {
    pub fn load() -> Option<Self> {
        let path = drive_index_path().ok()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    pub fn save(&self) -> Result<()> {
        let path = drive_index_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self)?;
        std::fs::write(&path, json)?;
        log::info!("Drive index saved to {path:?}");
        Ok(())
    }

    pub fn file_count(&self) -> usize {
        self.folder_files.values().map(|f| f.len()).sum()
    }
}

/// Load the timestamp of the cached Drive index, if it exists.
pub fn load_drive_index_timestamp() -> Option<u64> {
    DriveIndex::load().map(|idx| idx.updated_at)
}

impl FolderMap {
    /// Get folder IDs to search for a given faction, hero name, and pack code.
    /// Searches: aspect folder, basic folder, current hero folder, and pack hero folder.
    fn folders_for_faction(&self, faction: &str, hero_name: &str, pack_code: &str) -> Vec<&str> {
        let mut folders = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mapped = map_faction_to_aspect(faction);

        // Add matching aspect folder
        if let Some(folder_id) = self.aspect_folders.get(mapped) {
            if seen.insert(folder_id.as_str()) {
                folders.push(folder_id.as_str());
            }
        }

        // For non-basic cards, also check the "basic" aspect folder
        if mapped != "basic" {
            if let Some(basic_id) = self.aspect_folders.get("basic") {
                if seen.insert(basic_id.as_str()) {
                    folders.push(basic_id.as_str());
                }
            }
        }

        let normalized_hero = crate::marvelcdb::normalize_name(hero_name);
        let normalized_pack = crate::marvelcdb::normalize_name(pack_code);

        for hero_folder in &self.hero_folders {
            let normalized_folder_hero = crate::marvelcdb::normalize_name(&hero_folder.hero_name);

            // Add current hero's folder and pack's hero folder
            // (e.g., pack_code="nova" matches "Nova" folder)
            if (normalized_folder_hero == normalized_hero
                || normalized_folder_hero == normalized_pack)
                && seen.insert(hero_folder.folder_id.as_str())
            {
                folders.push(hero_folder.folder_id.as_str());
            }
        }

        folders
    }

    /// Get ALL folder IDs (aspects + heroes) for a last-resort exhaustive search.
    fn all_folder_ids(&self) -> Vec<&str> {
        let mut folders: Vec<&str> = self.aspect_folders.values().map(|id| id.as_str()).collect();
        for hero in &self.hero_folders {
            folders.push(hero.folder_id.as_str());
        }
        folders
    }
}

// --- Helpers ---

fn is_folder(mime_type: &str) -> bool {
    mime_type == "application/vnd.google-apps.folder"
}

fn map_faction_to_aspect(faction: &str) -> &str {
    match faction {
        "aggression" => "aggression",
        "justice" => "justice",
        "leadership" => "leadership",
        "protection" => "protection",
        "basic" | "neutral" | "pool" => "basic",
        "hero" => "hero",
        _ => faction,
    }
}

/// Extract hero name from a Drive folder name.
/// Handles formats like "Ororo Monroe_Storm (u)" -> "Storm"
fn extract_hero_name_from_drive(dir_name: &str) -> String {
    // Strip parenthetical suffixes like " (u)"
    let name = if let Some(paren_idx) = dir_name.rfind('(') {
        dir_name[..paren_idx].trim()
    } else {
        dir_name.trim()
    };

    // Same logic as marvelcdb: split on underscore
    if let Some((_civilian, hero)) = name.split_once('_') {
        hero.to_string()
    } else {
        name.to_string()
    }
}

/// Parse a Google Drive folder URL or ID to extract the folder ID.
///
/// Accepts:
/// - `https://drive.google.com/drive/folders/FOLDER_ID`
/// - `https://drive.google.com/drive/folders/FOLDER_ID?usp=sharing`
/// - `https://drive.google.com/drive/u/0/folders/FOLDER_ID`
/// - Raw folder ID string
pub fn parse_drive_folder_url(input: &str) -> Result<String> {
    let input = input.trim();

    if input.is_empty() {
        return Err(anyhow!("Empty input"));
    }

    // If it looks like a URL, extract the folder ID
    if input.starts_with("http://") || input.starts_with("https://") {
        let segments: Vec<&str> = input.split('/').collect();
        for (i, segment) in segments.iter().enumerate() {
            if *segment == "folders" && i + 1 < segments.len() {
                // The folder ID is the next segment, possibly with query params
                let id = segments[i + 1].split('?').next().unwrap_or("");
                if !id.is_empty() {
                    return Ok(id.to_string());
                }
            }
        }
        return Err(anyhow!("Could not find folder ID in URL: {input}"));
    }

    // Assume it's a raw folder ID
    Ok(input.to_string())
}

/// Known aspect prefixes to try as fallback when context prefix doesn't match.
const ASPECT_PREFIXES: &[&str] = &["Aggression", "Justice", "Leadership", "Protection", "Basic"];

/// Parse a card name from a Drive filename, trying the context prefix first,
/// then falling back to known aspect prefixes.
fn parse_drive_filename(filename: &str, context_prefix: Option<&str>) -> String {
    let path = Path::new(filename);

    // Try context prefix first
    let parsed = crate::marvelcdb::parse_card_name_from_filename(path, context_prefix);
    let normalized = crate::marvelcdb::normalize_name(&parsed);

    // If the context prefix didn't seem to strip anything useful
    // (i.e., the result still contains the aspect prefix), try aspect prefixes
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    for prefix in ASPECT_PREFIXES {
        if stem.starts_with(prefix) {
            let with_aspect = crate::marvelcdb::parse_card_name_from_filename(path, Some(prefix));
            let norm_aspect = crate::marvelcdb::normalize_name(&with_aspect);
            // Use the aspect-stripped version if it's shorter (more was stripped)
            if norm_aspect.len() < normalized.len() {
                return norm_aspect;
            }
        }
    }

    normalized
}

struct MatchContext<'a> {
    faction: &'a str,
    hero_name: &'a str,
    images_dir: &'a Path,
    folder_map: &'a FolderMap,
    folder_id: &'a str,
    min_score: f32,
}

/// Find the best matching file in a Drive folder for an unmatched card.
/// `min_score` is the minimum jaro_winkler score to accept (0.85 for targeted, 0.0 for fallback).
fn find_best_drive_match(
    normalized_card_name: &str,
    files: &[DriveFile],
    ctx: &MatchContext<'_>,
) -> Option<(DriveFile, PathBuf, f32)> {
    let mut best: Option<(DriveFile, PathBuf, f32)> = None;
    let context_prefix = determine_context_prefix(ctx.folder_id, ctx.folder_map);

    for file in files {
        if is_folder(&file.mime_type) {
            continue;
        }

        // Only consider image files
        let ext = Path::new(&file.name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !matches!(ext.as_str(), "tiff" | "tif" | "png" | "jpg" | "jpeg") {
            continue;
        }

        // Parse the card name, trying context prefix then aspect prefix fallback
        let normalized_parsed = parse_drive_filename(&file.name, context_prefix.as_deref());

        let score = strsim::jaro_winkler(normalized_card_name, &normalized_parsed) as f32;

        if score >= ctx.min_score && best.as_ref().is_none_or(|(_, _, s)| score > *s) {
            let dest_path = determine_local_path(
                &file.name,
                ctx.faction,
                ctx.hero_name,
                ctx.images_dir,
                ctx.folder_id,
                ctx.folder_map,
            );
            best = Some((file.clone(), dest_path, score));
        }
    }

    best
}

/// Determine the context prefix for filename parsing based on which folder we're in.
fn determine_context_prefix(folder_id: &str, folder_map: &FolderMap) -> Option<String> {
    // Check if it's an aspect folder
    for (aspect_name, id) in &folder_map.aspect_folders {
        if id == folder_id {
            // Capitalize first letter for prefix matching
            let mut chars = aspect_name.chars();
            let capitalized = match chars.next() {
                None => return None,
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            };
            return Some(capitalized);
        }
    }

    // Check if it's a hero folder
    for hero in &folder_map.hero_folders {
        if hero.folder_id == folder_id {
            return Some(hero.hero_name.clone());
        }
    }

    None
}

/// Determine the local file path where a downloaded file should be saved.
fn determine_local_path(
    filename: &str,
    faction: &str,
    hero_name: &str,
    images_dir: &Path,
    folder_id: &str,
    folder_map: &FolderMap,
) -> PathBuf {
    // Check if the folder_id corresponds to a hero folder
    for hero in &folder_map.hero_folders {
        if hero.folder_id == folder_id {
            return images_dir
                .join("heros")
                .join(&hero.folder_name)
                .join(filename);
        }
    }

    // Check if the folder_id corresponds to an aspect folder
    for (aspect_name, id) in &folder_map.aspect_folders {
        if id == folder_id {
            let mut chars = aspect_name.chars();
            let capitalized = match chars.next() {
                None => return images_dir.join(filename),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            };
            return images_dir.join("Aspects").join(capitalized).join(filename);
        }
    }

    // Fallback: place in the appropriate faction directory
    let mapped = map_faction_to_aspect(faction);
    match mapped {
        "hero" => images_dir.join("heros").join(hero_name).join(filename),
        "basic" => images_dir.join("Aspects").join("Basic").join(filename),
        aspect => {
            let mut chars = aspect.chars();
            let capitalized = match chars.next() {
                None => return images_dir.join(filename),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            };
            images_dir.join("Aspects").join(capitalized).join(filename)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_folder_url_raw_id() {
        let result = parse_drive_folder_url("1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2").unwrap();
        assert_eq!(result, "1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2");
    }

    #[test]
    fn test_parse_drive_folder_url_standard() {
        let result = parse_drive_folder_url(
            "https://drive.google.com/drive/folders/1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2",
        )
        .unwrap();
        assert_eq!(result, "1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2");
    }

    #[test]
    fn test_parse_drive_folder_url_with_query_params() {
        let result = parse_drive_folder_url(
            "https://drive.google.com/drive/folders/1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2?usp=sharing",
        )
        .unwrap();
        assert_eq!(result, "1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2");
    }

    #[test]
    fn test_parse_drive_folder_url_with_user_path() {
        let result = parse_drive_folder_url(
            "https://drive.google.com/drive/u/0/folders/1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2",
        )
        .unwrap();
        assert_eq!(result, "1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2");
    }

    #[test]
    fn test_parse_drive_folder_url_empty() {
        assert!(parse_drive_folder_url("").is_err());
    }

    #[test]
    fn test_parse_drive_folder_url_invalid_url() {
        assert!(parse_drive_folder_url("https://drive.google.com/drive/whatever").is_err());
    }

    #[test]
    fn test_parse_drive_folder_url_with_whitespace() {
        let result = parse_drive_folder_url("  1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2  ").unwrap();
        assert_eq!(result, "1FO7FRfJbqGsmAkfePhkzpmEqmW1-VwF2");
    }

    #[test]
    fn test_extract_hero_name_from_drive_simple() {
        assert_eq!(extract_hero_name_from_drive("Ororo Monroe_Storm"), "Storm");
    }

    #[test]
    fn test_extract_hero_name_from_drive_with_suffix() {
        assert_eq!(
            extract_hero_name_from_drive("Ororo Monroe_Storm (u)"),
            "Storm"
        );
    }

    #[test]
    fn test_extract_hero_name_from_drive_no_underscore() {
        assert_eq!(extract_hero_name_from_drive("Wolverine"), "Wolverine");
    }

    #[test]
    fn test_extract_hero_name_from_drive_with_parenthetical() {
        assert_eq!(
            extract_hero_name_from_drive("Anna Marie_Rogue (u)"),
            "Rogue"
        );
    }

    #[test]
    fn test_folder_map_folders_for_faction() {
        let mut map = FolderMap::default();
        map.aspect_folders
            .insert("aggression".to_string(), "agg_id".to_string());
        map.aspect_folders
            .insert("basic".to_string(), "basic_id".to_string());
        map.hero_folders.push(HeroFolder {
            folder_id: "storm_id".to_string(),
            folder_name: "Ororo Monroe_Storm".to_string(),
            hero_name: "Storm".to_string(),
        });

        // Aggression faction -> aggression folder + basic folder + hero folder
        let folders = map.folders_for_faction("aggression", "Storm", "");
        assert!(folders.contains(&"agg_id"));
        assert!(folders.contains(&"basic_id"));
        assert!(folders.contains(&"storm_id"));

        // Hero faction -> basic folder + hero folder
        let folders = map.folders_for_faction("hero", "Storm", "");
        assert!(folders.contains(&"basic_id"));
        assert!(folders.contains(&"storm_id"));

        // Pack code matches a different hero folder
        map.hero_folders.push(HeroFolder {
            folder_id: "nova_id".to_string(),
            folder_name: "Sam Alexander_Nova".to_string(),
            hero_name: "Nova".to_string(),
        });
        let folders = map.folders_for_faction("aggression", "Storm", "nova");
        assert!(folders.contains(&"agg_id"));
        assert!(folders.contains(&"storm_id"));
        assert!(folders.contains(&"nova_id"));
    }

    #[test]
    fn test_determine_local_path_aspect_folder() {
        let mut map = FolderMap::default();
        map.aspect_folders
            .insert("aggression".to_string(), "agg_id".to_string());

        let images_dir = Path::new("/images/Marvel Champions");
        let result = determine_local_path(
            "Aggression_Uppercut_Event.tiff",
            "aggression",
            "Storm",
            images_dir,
            "agg_id",
            &map,
        );
        assert_eq!(
            result,
            PathBuf::from(
                "/images/Marvel Champions/Aspects/Aggression/Aggression_Uppercut_Event.tiff"
            )
        );
    }

    #[test]
    fn test_determine_local_path_hero_folder() {
        let mut map = FolderMap::default();
        map.hero_folders.push(HeroFolder {
            folder_id: "storm_id".to_string(),
            folder_name: "Ororo Monroe_Storm (u)".to_string(),
            hero_name: "Storm".to_string(),
        });

        let images_dir = Path::new("/images/Marvel Champions");
        let result = determine_local_path(
            "Storm_Lightning_Bolt_Event.tiff",
            "hero",
            "Storm",
            images_dir,
            "storm_id",
            &map,
        );
        assert_eq!(
            result,
            PathBuf::from("/images/Marvel Champions/heros/Ororo Monroe_Storm (u)/Storm_Lightning_Bolt_Event.tiff")
        );
    }
}
