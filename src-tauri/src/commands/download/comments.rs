//! YouTube Data API comment sidecar support for completed downloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};


const COMMENT_THREADS_URL: &str = "https://www.googleapis.com/youtube/v3/commentThreads";
const TOP_COMMENT_COUNT: usize = 10;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeApiCredentialsInfo {
    credential_type: String,
    project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentThreadsResponse {
    #[serde(default)]
    items: Vec<CommentThread>,
}

#[derive(Debug, Deserialize)]
struct CommentThread {
    id: String,
    snippet: CommentThreadSnippet,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentThreadSnippet {
    top_level_comment: TopLevelComment,
    #[serde(default)]
    total_reply_count: u64,
}

#[derive(Debug, Deserialize)]
struct TopLevelComment {
    snippet: CommentSnippet,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentSnippet {
    #[serde(default)]
    author_display_name: String,
    #[serde(default)]
    author_channel_id: Option<AuthorChannelId>,
    #[serde(default)]
    text_original: String,
    #[serde(default)]
    like_count: u64,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct AuthorChannelId {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedComment {
    rank: usize,
    id: String,
    author: String,
    author_channel_id: String,
    text: String,
    like_count: u64,
    published_at: String,
    updated_at: String,
    reply_count: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommentsExport {
    video_id: String,
    source: &'static str,
    order: &'static str,
    count: usize,
    downloaded_at: String,
    comments: Vec<ExportedComment>,
}

fn find_api_key(value: &Value) -> Option<String> {
    const API_KEY_FIELDS: &[&str] = &[
        "youtube_api_key",
        "youtubeApiKey",
        "api_key",
        "apiKey",
        "developer_key",
        "developerKey",
        "key",
    ];

    match value {
        Value::Object(map) => {
            for field in API_KEY_FIELDS {
                if let Some(key) = map.get(*field).and_then(Value::as_str) {
                    let trimmed = key.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            map.values().find_map(find_api_key)
        }
        Value::Array(values) => values.iter().find_map(find_api_key),
        _ => None,
    }
}

fn read_credentials(path: &str) -> Result<(Value, String), String> {
    if path.trim().is_empty() {
        return Err("err_youtube_api_credentials_missing".to_string());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("err_youtube_api_credentials_read:{}", e))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|e| format!("err_youtube_api_credentials_json:{}", e))?;
    let api_key = find_api_key(&value).ok_or_else(|| {
        if value.get("type").and_then(Value::as_str) == Some("service_account") {
            "err_youtube_service_account_unsupported".to_string()
        } else {
            "err_youtube_api_key_missing".to_string()
        }
    })?;
    Ok((value, api_key))
}

pub(super) fn validate_credentials_path(path: &str) -> Result<YoutubeApiCredentialsInfo, String> {
    let (value, _) = read_credentials(path)?;
    let raw_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("api_key_json");
    let credential_type = if raw_type == "service_account" {
        // The service-account identity itself is not used by YouTube Data API. If this
        // JSON also carries an API key, the key is what authenticates public reads.
        "service_account+api_key".to_string()
    } else {
        raw_type.to_string()
    };
    let project_id = value
        .get("project_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(YoutubeApiCredentialsInfo {
        credential_type,
        project_id,
    })
}

#[tauri::command]
pub fn validate_youtube_api_credentials(path: String) -> Result<YoutubeApiCredentialsInfo, String> {
    validate_credentials_path(&path)
}

fn youtube_video_id(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("err_youtube_video_id_missing".to_string());
    }

    if let Some(query) = trimmed.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if parts.next() == Some("v") {
                if let Some(id) = parts.next() {
                    let id = id.split('#').next().unwrap_or(id).trim();
                    if !id.is_empty() {
                        return Ok(id.to_string());
                    }
                }
            }
        }
    }

    for marker in ["youtu.be/", "/shorts/", "/embed/", "/live/"] {
        if let Some((_, rest)) = trimmed.split_once(marker) {
            let id = rest
                .split(['?', '&', '#', '/'])
                .next()
                .unwrap_or("")
                .trim();
            if !id.is_empty() {
                return Ok(id.to_string());
            }
        }
    }

    Err("err_youtube_video_id_missing".to_string())
}

fn comments_sidecar_path(output_file: &str, download_dir: &str, video_id: &str) -> PathBuf {
    let output = PathBuf::from(output_file);
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(download_dir));
    let stem = output
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| video_id.to_string());
    parent.join(format!("{}.comments.json", stem))
}

fn api_error_message(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return format!("err_youtube_comments_api:{}:{}", status.as_u16(), message);
        }
    }
    format!("err_youtube_comments_api:{}:{}", status.as_u16(), body)
}

#[tauri::command]
pub async fn download_youtube_top_comments(
    url: String,
    credentials_file: String,
    output_file: String,
    download_dir: String,
) -> Result<String, String> {
    let video_id = youtube_video_id(&url)?;
    let (_, api_key) = read_credentials(&credentials_file)?;

    let response = reqwest::Client::new()
        .get(COMMENT_THREADS_URL)
        .query(&[
            ("part", "snippet"),
            ("videoId", video_id.as_str()),
            ("maxResults", "10"),
            ("order", "relevance"),
            ("key", api_key.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("err_youtube_comments_request:{}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("err_youtube_comments_response:{}", e))?;
    if !status.is_success() {
        return Err(api_error_message(status, &body));
    }

    let api_response: CommentThreadsResponse = serde_json::from_str(&body)
        .map_err(|e| format!("err_youtube_comments_json:{}", e))?;

    let comments = api_response
        .items
        .into_iter()
        .take(TOP_COMMENT_COUNT)
        .enumerate()
        .map(|(index, thread)| {
            let reply_count = thread.snippet.total_reply_count;
            let snippet = thread.snippet.top_level_comment.snippet;
            ExportedComment {
                rank: index + 1,
                id: thread.id,
                author: snippet.author_display_name,
                author_channel_id: snippet.author_channel_id.map(|id| id.value).unwrap_or_default(),
                text: snippet.text_original,
                like_count: snippet.like_count,
                published_at: snippet.published_at,
                updated_at: snippet.updated_at,
                reply_count,
            }
        })
        .collect::<Vec<_>>();

    let export = CommentsExport {
        video_id: video_id.clone(),
        source: "youtube-data-api-v3",
        order: "relevance",
        count: comments.len(),
        downloaded_at: chrono::Utc::now().to_rfc3339(),
        comments,
    };
    let json = serde_json::to_string_pretty(&export)
        .map_err(|e| format!("err_youtube_comments_serialize:{}", e))?;
    let sidecar = comments_sidecar_path(&output_file, &download_dir, &video_id);
    std::fs::write(&sidecar, json).map_err(|e| format!("err_youtube_comments_write:{}", e))?;

    Ok(sidecar.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_common_youtube_video_urls() {
        assert_eq!(
            youtube_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=abc").unwrap(),
            "dQw4w9WgXcQ"
        );
        assert_eq!(
            youtube_video_id("https://youtu.be/dQw4w9WgXcQ?t=3").unwrap(),
            "dQw4w9WgXcQ"
        );
        assert_eq!(
            youtube_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ").unwrap(),
            "dQw4w9WgXcQ"
        );
    }

    #[test]
    fn extracts_api_key_recursively() {
        let value = serde_json::json!({
            "type": "service_account",
            "project_id": "demo",
            "extra": { "youtube_api_key": "AIza-demo" }
        });
        assert_eq!(find_api_key(&value).as_deref(), Some("AIza-demo"));
    }

    #[test]
    fn sidecar_is_next_to_completed_video() {
        let path = comments_sidecar_path("/tmp/video/final.mp4", "/fallback", "abc");
        assert_eq!(path, PathBuf::from("/tmp/video/final.comments.json"));
    }
}
