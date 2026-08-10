# Google Drive Integration for MarvelCDB Unmatched Card Downloads

## Context
When importing a MarvelCDB deck, some cards can't be matched to local image files. The user has a complete card image collection in Google Drive with the same folder structure as their local directory. This feature adds a "Download from Google Drive" button that appears after a MarvelCDB fetch, allowing the user to automatically find and download missing card images.

## Approach
- **Auth**: Google API Key + folder shared as "anyone with the link can view". No OAuth2, no browser popups, no token refresh. API key stored in system keyring.
- **Drive API**: Direct REST calls via existing `reqwest` — only need list files + download endpoints
- **Trigger**: Manual button after MarvelCDB fetch shows unmatched cards
- **Search strategy**: Build a faction→folder-ID map by traversing known Drive folder structure (Aspects/, heros/, Basic/), then fuzzy-match filenames against unmatched card names using existing matching logic
- **No new crate dependencies** — everything uses existing `reqwest`, `serde`, `keyring`

## Prerequisites (one-time user setup)
1. In Google Cloud Console: create a project, enable Drive API, create an API key (restrict to Drive API)
2. In Google Drive: right-click the Marvel Champions folder → Share → "Anyone with the link" → Viewer
3. In the app: enter the API key and paste the Drive folder URL

## Files to Create/Modify

### New: `src/google_drive.rs`
- `GoogleDriveClient` struct wrapping `reqwest::Client` + API key
- `list_files_in_folder(folder_id)` → `Vec<DriveFile>` — calls `GET /drive/v3/files?q='{id}'+in+parents&key=...`
- `download_file(file_id, destination)` — calls `GET /drive/v3/files/{id}?alt=media&key=...`
- `build_folder_map(root_folder_id)` — traverses Aspects/, heros/, Basic/ to build faction→folder-ID HashMap
- `search_and_download_unmatched(unmatched_cards, hero_name, images_dir, sender)` — orchestrator
- `GoogleDriveMessage` enum for async progress (same pattern as `MarvelCdbMessage`)
- `parse_drive_folder_url(url) -> Result<String>` — extract folder ID from URL
- Unit tests for URL parsing, folder mapping logic

### Modified: `Cargo.toml`
No new dependencies needed. Existing `reqwest`, `serde`, `keyring`, `tokio` cover everything.

### Modified: `src/settings.rs`
- Add `google_drive_folder_id: Option<String>` to `AppSettings`
- Add keyring methods: `load_google_drive_api_key`, `save_google_drive_api_key`, `delete_google_drive_api_key`

### Modified: `src/marvelcdb.rs`
- Add `pub fn rematch_unmatched_cards(unmatched, images_dir, hero_name) -> (Vec<MatchedCard>, Vec<UnmatchedCard>)`
- Make `parse_card_name_from_filename` and `normalize_name` pub(crate)

### Modified: `src/ui/decklist_panel.rs`
- Add to `DecklistState`: `google_drive_api_key`, `google_drive_folder_url`, `is_downloading_from_drive`, `drive_download_progress`, `drive_download_error`
- Add "Google Drive" collapsing section: API key field, folder URL field
- After unmatched cards list: "Download from Google Drive" button with progress spinner
- Add new callback: `google_drive_download_callback`

### Modified: `src/main.rs`
- Add fields: `google_drive_receiver`, `google_drive_task`
- Add `start_google_drive_download()` — spawns async task
- Add `process_google_drive_messages()` — polls receiver, updates UI state
- On completion: merge newly-matched cards, remove from unmatched
- Wire callbacks from decklist panel

## Implementation Phases

### Phase 1: google_drive.rs — API client + folder mapping
- [ ] Create `src/google_drive.rs` with `GoogleDriveClient`, Drive REST calls, folder URL parsing
- [ ] Implement `build_folder_map` to traverse known folder structure
- [ ] Implement `search_and_download_unmatched` orchestrator
- [ ] Unit tests for URL parsing and folder mapping

### Phase 2: Settings + UI
- [ ] Add Google Drive API key + folder ID to settings.rs
- [ ] Add Google Drive section to decklist_panel.rs (API key, folder URL, download button)
- [ ] Add DecklistState fields for drive state

### Phase 3: Wire it up in main.rs
- [ ] Add rematch_unmatched_cards to marvelcdb.rs
- [ ] Add async task management + message processing in main.rs
- [ ] Connect download button to async flow
- [ ] On completion: update matched/unmatched lists

### Phase 4: Polish
- [ ] Error handling (invalid API key, folder not shared, file not found)
- [ ] cargo clippy + cargo fmt
- [ ] cargo test

## Drive API Details
All calls use `?key={API_KEY}` query parameter. No auth headers needed.

- **List files**: `GET https://www.googleapis.com/drive/v3/files?q='{folder_id}'+in+parents&fields=files(id,name,mimeType)&pageSize=1000&key={key}`
- **Download**: `GET https://www.googleapis.com/drive/v3/files/{file_id}?alt=media&key={key}`
- **Pagination**: handle `nextPageToken` for folders with 1000+ files

## Download Flow
1. MarvelCDB fetch completes with unmatched cards
2. User clicks "Download from Google Drive"
3. Async task:
   a. Build folder map (Aspects/Aggression→id, heros/SpiderMan→id, etc.)
   b. For each unmatched card, determine target folder by faction
   c. List files in that folder (cache results per folder)
   d. Fuzzy-match filenames using existing `parse_card_name_from_filename` + `normalize_name`
   e. Download matches to correct local path
   f. Re-run `rematch_unmatched_cards` on the originals
   g. Send completion message with updated results
4. UI updates: newly-matched cards move from unmatched to matched list

## Verification
1. `cargo test` — existing + new tests pass
2. `cargo clippy` — no warnings
3. Manual: configure API key + folder URL → fetch deck → click download → files appear locally → cards match