use anyhow::{Context, Result, bail};
use log::warn;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

pub const BASE_URL: &str = "https://apis.roblox.com/legacy-localization-tables";

const LEGACY_MAX_ENTRIES_PER_UPDATE: usize = 50;
const PATCH_MAX_RETRIES: u32 = 12;

const TRANSIENT_STATUS_CODES: &[StatusCode] = &[
    StatusCode::TOO_MANY_REQUESTS,
    StatusCode::BAD_GATEWAY,
    StatusCode::SERVICE_UNAVAILABLE,
    StatusCode::GATEWAY_TIMEOUT,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryIdentifier {
    pub key: String,
    pub source: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub identifier: EntryIdentifier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translations: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchEntry {
    pub identifier: EntryIdentifier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translations: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<bool>,
}

#[derive(Deserialize)]
struct AutoLocalizationTableResponse {
    #[serde(rename = "autoLocalizationTableId")]
    auto_localization_table_id: String,
}

#[derive(Deserialize)]
struct EntriesPageResponse {
    data: Option<Vec<RemoteEntry>>,
    #[serde(rename = "nextPageCursor")]
    next_page_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct EntryOperationLimits {
    pub max_context_length: usize,
    pub max_key_length: usize,
    pub max_source_length: usize,
}

impl Default for EntryOperationLimits {
    fn default() -> Self {
        Self {
            max_context_length: 500,
            max_key_length: 300,
            max_source_length: 300,
        }
    }
}

#[derive(Deserialize)]
struct TableLimitsResponse {
    #[serde(rename = "entryOperationLimits")]
    entry_operation_limits: Option<EntryOperationLimitsResponse>,
    #[serde(rename = "tableOperationLimits")]
    table_operation_limits: Option<TableOperationLimits>,
}

#[derive(Deserialize)]
struct EntryOperationLimitsResponse {
    #[serde(rename = "maxContextLength")]
    max_context_length: Option<usize>,
    #[serde(rename = "maxKeyLength")]
    max_key_length: Option<usize>,
    #[serde(rename = "maxSourceLength")]
    max_source_length: Option<usize>,
}

#[derive(Deserialize)]
struct TableOperationLimits {
    #[serde(rename = "maxEntriesPerUpdate")]
    max_entries_per_update: Option<usize>,
}

#[derive(Deserialize)]
pub struct PatchResponse {
    #[serde(rename = "modifiedEntriesAndTranslations", default)]
    pub modified_entries_and_translations: Vec<Value>,
    #[serde(rename = "failedEntriesAndTranslations", default)]
    pub failed_entries_and_translations: Vec<FailedEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    #[serde(rename = "errorCode")]
    pub error_code: Option<i32>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
}

#[derive(Deserialize)]
pub struct FailedEntry {
    pub identifier: Option<EntryIdentifier>,
    pub error: Option<ApiError>,
}

pub struct LocalizationClient {
    client: Client,
    api_key: String,
    game_id: u64,
    table_id: String,
}

impl LocalizationClient {
    pub fn new(api_key: String, game_id: u64) -> Result<Self> {
        let client = Client::new();
        let table_id = Self::create_table(&client, &api_key, game_id)?;

        Ok(Self {
            client,
            api_key,
            game_id,
            table_id,
        })
    }

    fn create_table(client: &Client, api_key: &str, game_id: u64) -> Result<String> {
        let url = format!("{BASE_URL}/v1/autolocalization/games/{game_id}/autolocalizationtable");
        let response = client
            .post(&url)
            .header("x-api-key", api_key)
            .send()
            .context("failed to create autolocalization table")?;

        let status = response.status();
        let body = response
            .text()
            .context("failed to read autolocalization response")?;
        if !status.is_success() {
            bail!(format_api_error(status, &body));
        }

        let parsed: AutoLocalizationTableResponse =
            serde_json::from_str(&body).context("failed to parse autolocalization response")?;
        Ok(parsed.auto_localization_table_id)
    }

    pub fn fetch_remote_entries(&self) -> Result<(Vec<RemoteEntry>, usize)> {
        let mut remote_entries = Vec::new();
        let mut fetch_cursor: Option<String> = None;

        loop {
            let mut query = vec![("gameId", self.game_id.to_string())];
            if let Some(cursor) = &fetch_cursor {
                if !cursor.is_empty() {
                    query.push(("cursor", cursor.clone()));
                }
            }

            let url = format!(
                "{BASE_URL}/v1/localization-table/tables/{}/entries",
                self.table_id
            );
            let response = self
                .client
                .get(&url)
                .header("x-api-key", &self.api_key)
                .query(&query)
                .send()
                .context("failed to fetch localization entries")?;

            let status = response.status();
            let body = response.text().context("failed to read entries response")?;
            if !status.is_success() {
                bail!(format_api_error(status, &body));
            }

            let page: EntriesPageResponse =
                serde_json::from_str(&body).context("failed to parse entries response")?;
            fetch_cursor = page.next_page_cursor;

            if let Some(data) = page.data {
                remote_entries.extend(data);
            }

            match fetch_cursor.as_deref() {
                None | Some("") => break,
                Some(_) => {}
            }
        }

        let (deduped_entries, duplicate_rows) = dedupe_remote_entries_by_key(&remote_entries);
        let unique_count = deduped_entries.len();

        if duplicate_rows > 0 {
            return Ok((remote_entries, unique_count));
        }

        Ok((deduped_entries, unique_count))
    }

    pub fn fetch_limits(&self) -> (EntryOperationLimits, usize) {
        let url = format!("{BASE_URL}/v1/localization-table/limits");
        let response = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&[("gameId", self.game_id.to_string())])
            .send();

        let mut entry_limits = EntryOperationLimits::default();
        let mut max_entries_per_update = LEGACY_MAX_ENTRIES_PER_UPDATE;

        if let Ok(response) = response {
            if response.status().is_success() {
                if let Ok(body) = response.text() {
                    if let Ok(limits) = serde_json::from_str::<TableLimitsResponse>(&body) {
                        if let Some(entry) = limits.entry_operation_limits {
                            if let Some(max) = entry.max_context_length.filter(|&v| v > 0) {
                                entry_limits.max_context_length = max;
                            }
                            if let Some(max) = entry.max_key_length.filter(|&v| v > 0) {
                                entry_limits.max_key_length = max;
                            }
                            if let Some(max) = entry.max_source_length.filter(|&v| v > 0) {
                                entry_limits.max_source_length = max;
                            }
                        }
                        if let Some(max) = limits
                            .table_operation_limits
                            .and_then(|l| l.max_entries_per_update)
                        {
                            if max > 0 {
                                max_entries_per_update = max.min(LEGACY_MAX_ENTRIES_PER_UPDATE);
                            }
                        }
                    }
                }
            }
        }

        (entry_limits, max_entries_per_update)
    }

    pub fn max_entries_per_update(&self) -> usize {
        self.fetch_limits().1
    }

    pub fn max_create_entries_per_update(&self) -> usize {
        self.max_entries_per_update()
            .min(LEGACY_MAX_ENTRIES_PER_UPDATE / 2)
    }

    pub fn patch_entries(&self, entries: &[PatchEntry]) -> Result<PatchResponse> {
        let request_body = serde_json::json!({
            "name": "Unused Translation Table Name Placeholder",
            "entries": entries,
        });

        let url = format!("{BASE_URL}/v1/localization-table/tables/{}", self.table_id);

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let response = self
                .client
                .patch(&url)
                .header("x-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .query(&[("gameId", self.game_id.to_string())])
                .json(&request_body)
                .send()
                .context("failed to PATCH localization table")?;

            let status = response.status();
            if status.is_success() {
                let body = response.text().context("failed to read PATCH response")?;
                return serde_json::from_str(&body).context("failed to parse PATCH response");
            }

            let body = response.text().unwrap_or_default();
            let should_retry = TRANSIENT_STATUS_CODES.contains(&status);
            if should_retry && attempt < PATCH_MAX_RETRIES {
                let wait_seconds = 2_u64.pow(attempt.saturating_sub(1)).min(60);
                warn!(
                    "PATCH retry {attempt}/{PATCH_MAX_RETRIES} after {status}, waiting {wait_seconds}s"
                );
                thread::sleep(Duration::from_secs(wait_seconds));
                continue;
            }

            bail!(format_api_error(status, &body));
        }
    }
}

pub fn dedupe_remote_entries_by_key(entries: &[RemoteEntry]) -> (Vec<RemoteEntry>, usize) {
    let mut by_key: HashMap<String, RemoteEntry> = HashMap::new();
    let mut duplicate_rows = 0usize;

    for entry in entries {
        let key = entry.identifier.key.clone();
        if by_key.contains_key(&key) {
            duplicate_rows += 1;
        }
        by_key.insert(key, entry.clone());
    }

    if duplicate_rows > 0 {
        warn!("remote has {duplicate_rows} duplicate key rows");
    }

    (by_key.into_values().collect(), duplicate_rows)
}

pub fn format_api_error(status: StatusCode, body: &str) -> String {
    serde_json::json!({ "status": status.as_u16(), "body": body }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_keeps_last_entry_per_key() {
        let entries = vec![
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "a".into(),
                    source: "one".into(),
                    context: "a".into(),
                },
                translations: None,
                metadata: None,
            },
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "a".into(),
                    source: "two".into(),
                    context: "a".into(),
                },
                translations: None,
                metadata: None,
            },
        ];

        let (deduped, dupes) = dedupe_remote_entries_by_key(&entries);

        assert_eq!(dupes, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].identifier.source, "two");
    }
}
