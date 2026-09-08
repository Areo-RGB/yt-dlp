//! Cloudflare R2 upload commands for local video files.

use futures_util::TryStreamExt;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

const REGION: &str = "auto";
const SERVICE: &str = "s3";

/// AWS SigV4 URI encoding: percent-encode everything except unreserved
/// characters `A-Z a-z 0-9 - _ . ~`. Each object-key segment is encoded
/// individually so `/` separators are preserved.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'?')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'%')
    .add(b'|')
    .add(b'\\')
    .add(b'^')
    .add(b'[')
    .add(b']')
    .add(b'+')
    .add(b',')
    .add(b';')
    .add(b'=')
    .add(b'&')
    .add(b'$')
    .add(b'!')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b':')
    .add(b'@');

type HmacSha256 = Hmac<Sha256>;

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    hex_digest(Sha256::digest(value.as_ref()))
}

fn hmac(key: &[u8], value: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts arbitrary key lengths");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(date: &str, secret_access_key: &str) -> Vec<u8> {
    let date_key = hmac(format!("AWS4{secret_access_key}").as_bytes(), date);
    let region_key = hmac(&date_key, REGION);
    let service_key = hmac(&region_key, SERVICE);
    hmac(&service_key, "aws4_request")
}

fn encoded_key(key: &str) -> String {
    key.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .map(|part| utf8_percent_encode(part, PATH_SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("3gp") => "video/3gpp",
        Some("avi") => "video/x-msvideo",
        Some("flv") => "video/x-flv",
        Some("m4v") => "video/x-m4v",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("mp4") => "video/mp4",
        Some("mpeg") | Some("mpg") => "video/mpeg",
        Some("ts") => "video/mp2t",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("opus") => "audio/opus",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("webp") => "image/webp",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("vtt") => "text/vtt",
        Some("srt") => "application/x-subrip",
        _ => "application/octet-stream",
    }
}

fn truncate(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        message.to_string()
    } else {
        let truncated: String = message.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

const R2_VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "flv", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ts", "webm",
];

#[derive(Debug, Serialize)]
pub struct R2Library {
    pub bucket: String,
    pub folders: Vec<R2Folder>,
}

#[derive(Debug, Serialize)]
pub struct R2Folder {
    pub name: String,
    pub path: String,
    pub videos: Vec<R2Video>,
}

#[derive(Debug, Serialize)]
pub struct R2Video {
    pub name: String,
    pub key: String,
    pub size: u64,
    pub modified: Option<i64>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct R2ListResponse {
    #[serde(rename = "Contents", default)]
    contents: Vec<R2Object>,
    #[serde(rename = "IsTruncated", default)]
    is_truncated: bool,
    #[serde(rename = "NextContinuationToken")]
    next_continuation_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct R2Object {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "Size", default)]
    size: u64,
    #[serde(rename = "LastModified")]
    last_modified: Option<String>,
}

struct R2Config {
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    bucket: String,
    public_base_url: String,
    endpoint_host: String,
}

const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
const MULTIPART_THRESHOLD: u64 = 100 * 1024 * 1024;
const MULTIPART_PART_SIZE: u64 = 64 * 1024 * 1024;
const MULTIPART_CONCURRENCY: usize = 4;

#[derive(Default)]
pub struct R2UploadState {
    cancelled: Mutex<HashSet<String>>,
}

impl R2UploadState {
    fn is_cancelled(&self, upload_id: &str) -> bool {
        self.cancelled
            .lock()
            .map(|cancelled| cancelled.contains(upload_id))
            .unwrap_or(true)
    }

    fn cancel(&self, upload_id: String) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(upload_id);
        }
    }

    fn clear(&self, upload_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.remove(upload_id);
        }
    }
}

struct UploadProgress {
    app: AppHandle,
    upload_id: String,
    file_path: String,
    total: u64,
    uploaded: AtomicU64,
    started: Instant,
}

fn emit_upload_progress(progress: &UploadProgress) {
    let uploaded = progress.uploaded.load(Ordering::Relaxed);
    let elapsed = progress.started.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        uploaded as f64 / elapsed
    } else {
        0.0
    };
    let eta = if speed > 0.0 && uploaded < progress.total {
        Some(((progress.total - uploaded) as f64 / speed).ceil() as u64)
    } else {
        None
    };
    let _ = progress.app.emit(
        "r2-upload-progress",
        serde_json::json!({
            "uploadId": progress.upload_id,
            "filePath": progress.file_path,
            "uploaded": uploaded,
            "total": progress.total,
            "percent": if progress.total == 0 { 0.0 } else { uploaded as f64 * 100.0 / progress.total as f64 },
            "speed": speed,
            "eta": eta,
        }),
    );
}

fn progress_stream<R>(
    reader: R,
    progress: Arc<UploadProgress>,
) -> impl futures_util::TryStream<Ok = bytes::Bytes, Error = std::io::Error> + Send
where
    R: AsyncRead + Unpin + Send + 'static,
{
    ReaderStream::new(reader).map_ok(move |chunk| {
        progress
            .uploaded
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
        emit_upload_progress(&progress);
        chunk
    })
}

async fn create_multipart_upload(
    client: &reqwest::Client,
    state: &R2UploadState,
    upload_id: &str,
    config: &R2Config,
    key: &str,
) -> Result<String, String> {
    let canonical_uri = format!("/{}/{}", config.bucket, key);
    let canonical_query = "uploads=";
    let request_url = format!("{}{}?{}", config.endpoint, canonical_uri, canonical_query);
    let (timestamp, date) = request_timestamp()?;
    let authorization = request_authorization(
        "POST",
        &canonical_uri,
        canonical_query,
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        UNSIGNED_PAYLOAD,
        &timestamp,
        &date,
    );
    let response = send_with_cancellation(
        client,
        state,
        upload_id,
        client
            .post(request_url)
            .header(HOST, &config.endpoint_host)
            .header("x-amz-content-sha256", UNSIGNED_PAYLOAD)
            .header("x-amz-date", &timestamp)
            .header("Authorization", authorization),
    )
    .await?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("err_r2_upload:{error}"))?;
    if !status.is_success() {
        return Err(format!(
            "err_r2_upload_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }
    #[derive(Deserialize)]
    struct InitiateResponse {
        #[serde(rename = "UploadId")]
        upload_id: String,
    }
    quick_xml::de::from_str::<InitiateResponse>(&body)
        .map(|response| response.upload_id)
        .map_err(|error| format!("err_r2_upload_parse:{}", truncate(&error.to_string(), 500)))
}

async fn upload_multipart_part(
    client: &reqwest::Client,
    state: &R2UploadState,
    progress: Arc<UploadProgress>,
    config: &R2Config,
    key: &str,
    multipart_id: &str,
    part_number: u64,
    offset: u64,
    length: u64,
) -> Result<String, String> {
    if state.is_cancelled(&progress.upload_id) {
        return Err("err_r2_upload_cancelled".to_string());
    }
    let mut file = File::open(&progress.file_path)
        .await
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
    let stream = progress_stream(file.take(length), progress.clone());
    let canonical_uri = format!("/{}/{}", config.bucket, key);
    let canonical_query = format!(
        "partNumber={part_number}&uploadId={}",
        utf8_percent_encode(multipart_id, PATH_SEGMENT)
    );
    let request_url = format!("{}{}?{}", config.endpoint, canonical_uri, canonical_query);
    let (timestamp, date) = request_timestamp()?;
    let authorization = request_authorization(
        "PUT",
        &canonical_uri,
        &canonical_query,
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        UNSIGNED_PAYLOAD,
        &timestamp,
        &date,
    );
    let response = send_with_cancellation(
        client,
        state,
        &progress.upload_id,
        client
            .put(request_url)
            .header(HOST, &config.endpoint_host)
            .header(CONTENT_LENGTH, length)
            .header("x-amz-content-sha256", UNSIGNED_PAYLOAD)
            .header("x-amz-date", &timestamp)
            .header("Authorization", authorization)
            .body(reqwest::Body::wrap_stream(stream)),
    )
    .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "err_r2_upload_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }
    response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| "err_r2_upload_missing_etag".to_string())
}

async fn complete_multipart_upload(
    client: &reqwest::Client,
    state: &R2UploadState,
    upload_id: &str,
    config: &R2Config,
    key: &str,
    multipart_id: &str,
    parts: &[(u64, String)],
) -> Result<(), String> {
    let parts_xml = parts
        .iter()
        .map(|(number, etag)| {
            format!("<Part><PartNumber>{number}</PartNumber><ETag>{etag}</ETag></Part>")
        })
        .collect::<String>();
    let body = format!("<CompleteMultipartUpload>{parts_xml}</CompleteMultipartUpload>");
    let canonical_uri = format!("/{}/{}", config.bucket, key);
    let canonical_query = format!(
        "uploadId={}",
        utf8_percent_encode(multipart_id, PATH_SEGMENT)
    );
    let request_url = format!("{}{}?{}", config.endpoint, canonical_uri, canonical_query);
    let (timestamp, date) = request_timestamp()?;
    let authorization = request_authorization(
        "POST",
        &canonical_uri,
        &canonical_query,
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        UNSIGNED_PAYLOAD,
        &timestamp,
        &date,
    );
    let response = send_with_cancellation(
        client,
        state,
        upload_id,
        client
            .post(request_url)
            .header(HOST, &config.endpoint_host)
            .header(CONTENT_TYPE, "application/xml")
            .header("x-amz-content-sha256", UNSIGNED_PAYLOAD)
            .header("x-amz-date", &timestamp)
            .header("Authorization", authorization)
            .body(body),
    )
    .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "err_r2_upload_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }
    Ok(())
}

async fn abort_multipart_upload(
    client: &reqwest::Client,
    config: &R2Config,
    key: &str,
    upload_id: &str,
) {
    let canonical_uri = format!("/{}/{}", config.bucket, key);
    let canonical_query = format!("uploadId={}", utf8_percent_encode(upload_id, PATH_SEGMENT));
    let request_url = format!("{}{}?{}", config.endpoint, canonical_uri, canonical_query);
    let Ok((timestamp, date)) = request_timestamp() else {
        return;
    };
    let authorization = request_authorization(
        "DELETE",
        &canonical_uri,
        &canonical_query,
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        UNSIGNED_PAYLOAD,
        &timestamp,
        &date,
    );
    let _ = client
        .delete(request_url)
        .header(HOST, &config.endpoint_host)
        .header("x-amz-content-sha256", UNSIGNED_PAYLOAD)
        .header("x-amz-date", timestamp)
        .header("Authorization", authorization)
        .send()
        .await;
}

async fn upload_single_object(
    client: &reqwest::Client,
    state: &R2UploadState,
    progress: Arc<UploadProgress>,
    config: &R2Config,
    key: &str,
    media_type: &str,
) -> Result<(), String> {
    let file = File::open(&progress.file_path)
        .await
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
    let canonical_uri = format!("/{}/{}", config.bucket, key);
    let (timestamp, date) = request_timestamp()?;
    let authorization = request_authorization(
        "PUT",
        &canonical_uri,
        "",
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        UNSIGNED_PAYLOAD,
        &timestamp,
        &date,
    );
    let request_url = format!("{}{}", config.endpoint, canonical_uri);
    let response = send_with_cancellation(
        client,
        state,
        &progress.upload_id,
        client
            .put(request_url)
            .header(HOST, &config.endpoint_host)
            .header(CONTENT_TYPE, media_type)
            .header(CONTENT_LENGTH, progress.total)
            .header("x-amz-content-sha256", UNSIGNED_PAYLOAD)
            .header("x-amz-date", &timestamp)
            .header("Authorization", authorization)
            .body(reqwest::Body::wrap_stream(progress_stream(
                file,
                progress.clone(),
            ))),
    )
    .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "err_r2_upload_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }
    Ok(())
}

async fn upload_multipart_object(
    client: &reqwest::Client,
    state: &R2UploadState,
    progress: Arc<UploadProgress>,
    config: &R2Config,
    key: &str,
) -> Result<(), String> {
    let multipart_id =
        create_multipart_upload(client, state, &progress.upload_id, config, key).await?;
    let part_count = (progress.total + MULTIPART_PART_SIZE - 1) / MULTIPART_PART_SIZE;
    let mut parts = Vec::with_capacity(part_count as usize);

    for batch_start in (1..=part_count).step_by(MULTIPART_CONCURRENCY) {
        if state.is_cancelled(&progress.upload_id) {
            abort_multipart_upload(client, config, key, &multipart_id).await;
            return Err("err_r2_upload_cancelled".to_string());
        }
        let futures = (batch_start
            ..=(batch_start + MULTIPART_CONCURRENCY as u64 - 1).min(part_count))
            .map(|part_number| {
                let offset = (part_number - 1) * MULTIPART_PART_SIZE;
                let length = (progress.total - offset).min(MULTIPART_PART_SIZE);
                upload_multipart_part(
                    client,
                    state,
                    progress.clone(),
                    config,
                    key,
                    &multipart_id,
                    part_number,
                    offset,
                    length,
                )
            })
            .collect::<Vec<_>>();
        for (offset, result) in futures_util::future::join_all(futures)
            .await
            .into_iter()
            .enumerate()
        {
            match result {
                Ok(etag) => parts.push((batch_start + offset as u64, etag)),
                Err(error) => {
                    abort_multipart_upload(client, config, key, &multipart_id).await;
                    return Err(error);
                }
            }
        }
    }

    if let Err(error) = complete_multipart_upload(
        client,
        state,
        &progress.upload_id,
        config,
        key,
        &multipart_id,
        &parts,
    )
    .await
    {
        abort_multipart_upload(client, config, key, &multipart_id).await;
        return Err(error);
    }
    Ok(())
}

async fn send_with_cancellation(
    client: &reqwest::Client,
    state: &R2UploadState,
    upload_id: &str,
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    let request = request
        .build()
        .map_err(|error| format!("err_r2_upload:{error}"))?;
    let send = client.execute(request);
    tokio::pin!(send);
    loop {
        if state.is_cancelled(upload_id) {
            return Err("err_r2_upload_cancelled".to_string());
        }
        tokio::select! {
            response = &mut send => {
                return response.map_err(|error| format!("err_r2_upload:{}", truncate(&error.to_string(), 500)));
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}

fn r2_config(
    account_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
    endpoint: &str,
    bucket: &str,
    public_base_url: &str,
) -> Result<R2Config, String> {
    let account_id = account_id.trim();
    let access_key_id = access_key_id.trim();
    let secret_access_key = secret_access_key.trim();
    let endpoint = endpoint.trim().trim_end_matches('/').to_string();
    let bucket = bucket.trim().trim_matches('/').to_string();
    let public_base_url = public_base_url.trim().trim_end_matches('/').to_string();

    if account_id.is_empty()
        || access_key_id.is_empty()
        || secret_access_key.is_empty()
        || endpoint.is_empty()
        || bucket.is_empty()
    {
        return Err("err_r2_config_missing".to_string());
    }

    if bucket.contains('/')
        || bucket.chars().any(char::is_whitespace)
        || bucket.contains('?')
        || bucket.contains('#')
        || bucket.len() > 63
    {
        return Err("err_r2_invalid_bucket".to_string());
    }

    let endpoint_url =
        reqwest::Url::parse(&endpoint).map_err(|error| format!("err_r2_endpoint:{error}"))?;
    if endpoint_url.scheme() != "https" && endpoint_url.scheme() != "http" {
        return Err("err_r2_endpoint_scheme".to_string());
    }
    let endpoint_host = endpoint_url
        .host_str()
        .ok_or_else(|| "err_r2_endpoint".to_string())?
        .to_string();
    if endpoint_host.trim().is_empty() {
        return Err("err_r2_endpoint".to_string());
    }

    Ok(R2Config {
        access_key_id: access_key_id.to_string(),
        secret_access_key: secret_access_key.to_string(),
        endpoint,
        bucket,
        public_base_url,
        endpoint_host,
    })
}

fn request_timestamp() -> Result<(String, String), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("err_r2_time:{error}"))?;
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp(now.as_secs() as i64, 0)
        .ok_or_else(|| "err_r2_time".to_string())?
        .format("%Y%m%dT%H%M%SZ")
        .to_string();
    let date = timestamp
        .get(..8)
        .ok_or_else(|| "err_r2_time".to_string())?
        .to_string();
    Ok((timestamp, date))
}

fn request_authorization(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    endpoint_host: &str,
    access_key_id: &str,
    secret_access_key: &str,
    payload_hash: &str,
    timestamp: &str,
    date: &str,
) -> String {
    let canonical_headers = format!(
        "host:{endpoint_host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{timestamp}\n"
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date}/{REGION}/{SERVICE}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request)
    );
    let signature = hex_digest(hmac(&signing_key(date, secret_access_key), &string_to_sign));
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

fn is_r2_video(key: &str) -> bool {
    key.rsplit_once('.')
        .map(|(_, extension)| {
            R2_VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
        .unwrap_or(false)
}

fn r2_modified_timestamp(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
}

fn r2_object_url(config: &R2Config, key: &str) -> Result<String, String> {
    if !config.public_base_url.is_empty() {
        return Ok(format!("{}/{}", config.public_base_url, encoded_key(key)));
    }
    presigned_r2_url(config, key)
}

fn presigned_r2_url(config: &R2Config, key: &str) -> Result<String, String> {
    let canonical_uri = format!("/{}/{}", config.bucket, encoded_key(key));
    let (timestamp, date) = request_timestamp()?;
    let scope = format!("{date}/{REGION}/{SERVICE}/aws4_request");
    let credential = format!("{}/{}", config.access_key_id, scope);
    let canonical_query = format!(
        "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential={}&X-Amz-Date={timestamp}&X-Amz-Expires=3600&X-Amz-SignedHeaders=host",
        utf8_percent_encode(&credential, PATH_SEGMENT)
    );
    let canonical_headers = format!("host:{}\n", config.endpoint_host);
    let canonical_request = format!(
        "GET\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\nhost\nUNSIGNED-PAYLOAD"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request)
    );
    let signature = hex_digest(hmac(
        &signing_key(&date, &config.secret_access_key),
        &string_to_sign,
    ));
    Ok(format!(
        "{}{}?{}&X-Amz-Signature={signature}",
        config.endpoint, canonical_uri, canonical_query
    ))
}

#[tauri::command]
pub async fn read_r2_env_file(path: String) -> Result<String, String> {
    let path = path.trim();
    let file_name = Path::new(path).file_name().and_then(|name| name.to_str());
    if file_name != Some(".env") {
        return Err("err_r2_env_file_name".to_string());
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("err_r2_env_file:{}", truncate(&error.to_string(), 300)))
}

#[tauri::command]
pub async fn list_r2_videos(
    account_id: String,
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    bucket: String,
    public_base_url: String,
) -> Result<R2Library, String> {
    let config = r2_config(
        &account_id,
        &access_key_id,
        &secret_access_key,
        &endpoint,
        &bucket,
        &public_base_url,
    )?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("err_r2_list:{error}"))?;
    let mut continuation_token = None;
    let mut objects = Vec::new();

    loop {
        let canonical_query = match continuation_token.as_deref() {
            Some(token) => format!(
                "continuation-token={}&list-type=2&max-keys=1000",
                utf8_percent_encode(token, PATH_SEGMENT)
            ),
            None => "list-type=2&max-keys=1000".to_string(),
        };
        let canonical_uri = format!("/{}", config.bucket);
        let request_url = format!("{}{}?{}", config.endpoint, canonical_uri, canonical_query);
        let payload_hash = sha256_hex("");
        let (timestamp, date) = request_timestamp()?;
        let authorization = request_authorization(
            "GET",
            &canonical_uri,
            &canonical_query,
            &config.endpoint_host,
            &config.access_key_id,
            &config.secret_access_key,
            &payload_hash,
            &timestamp,
            &date,
        );
        let response = client
            .get(&request_url)
            .header(HOST, &config.endpoint_host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &timestamp)
            .header("Authorization", authorization)
            .send()
            .await
            .map_err(|error| format!("err_r2_list:{}", truncate(&error.to_string(), 500)))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("err_r2_list:{}", truncate(&error.to_string(), 500)))?;
        if !status.is_success() {
            return Err(format!(
                "err_r2_list_http:{status}:{}",
                truncate(body.trim(), 1000)
            ));
        }
        let page: R2ListResponse = quick_xml::de::from_str(&body)
            .map_err(|error| format!("err_r2_list_parse:{}", truncate(&error.to_string(), 500)))?;
        objects.extend(page.contents);
        if !page.is_truncated {
            break;
        }
        continuation_token = page.next_continuation_token;
        if continuation_token.is_none() {
            return Err("err_r2_list_pagination".to_string());
        }
    }

    let mut grouped = BTreeMap::<String, Vec<R2Video>>::new();
    for object in objects {
        let key = object.key.trim_matches('/').to_string();
        if key.is_empty() || !is_r2_video(&key) {
            continue;
        }
        let (folder_path, name) = match key.rsplit_once('/') {
            Some((folder, name)) => (folder.to_string(), name.to_string()),
            None => (String::new(), key.clone()),
        };
        grouped.entry(folder_path).or_default().push(R2Video {
            name,
            key: key.clone(),
            size: object.size,
            modified: r2_modified_timestamp(object.last_modified.as_deref()),
            url: r2_object_url(&config, &key)?,
        });
    }

    let folders = grouped
        .into_iter()
        .map(|(path, mut videos)| {
            videos.sort_by_cached_key(|video| video.name.to_ascii_lowercase());
            let name = path
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("Unsorted")
                .to_string();
            R2Folder { name, path, videos }
        })
        .collect();

    Ok(R2Library {
        bucket: config.bucket,
        folders,
    })
}

#[tauri::command]
pub async fn delete_r2_object(
    key: String,
    account_id: String,
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    bucket: String,
    public_base_url: String,
) -> Result<(), String> {
    let config = r2_config(
        &account_id,
        &access_key_id,
        &secret_access_key,
        &endpoint,
        &bucket,
        &public_base_url,
    )?;
    let encoded_object_key = encoded_key(&key);
    if encoded_object_key.is_empty() || encoded_object_key.len() > 1024 {
        return Err("err_r2_invalid_key".to_string());
    }
    let canonical_uri = format!("/{}/{}", config.bucket, encoded_object_key);
    let request_url = format!("{}{}", config.endpoint, canonical_uri);
    let payload_hash = sha256_hex("");
    let (timestamp, date) = request_timestamp()?;
    let authorization = request_authorization(
        "DELETE",
        &canonical_uri,
        "",
        &config.endpoint_host,
        &config.access_key_id,
        &config.secret_access_key,
        &payload_hash,
        &timestamp,
        &date,
    );
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("err_r2_delete:{error}"))?
        .delete(request_url)
        .header(HOST, &config.endpoint_host)
        .header("x-amz-content-sha256", &payload_hash)
        .header("x-amz-date", &timestamp)
        .header("Authorization", authorization)
        .send()
        .await
        .map_err(|error| format!("err_r2_delete:{}", truncate(&error.to_string(), 500)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "err_r2_delete_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::content_type;
    use std::path::Path;

    #[test]
    fn content_type_covers_common_download_outputs() {
        assert_eq!(content_type(Path::new("audio.m4a")), "audio/mp4");
        assert_eq!(content_type(Path::new("thumbnail.webp")), "image/webp");
        assert_eq!(content_type(Path::new("captions.vtt")), "text/vtt");
        assert_eq!(
            content_type(Path::new("unknown.bin")),
            "application/octet-stream"
        );
    }
}

#[tauri::command]
pub async fn upload_local_video_to_r2(
    app: AppHandle,
    state: State<'_, R2UploadState>,
    upload_id: String,
    file_path: String,
    object_key: String,
    account_id: String,
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    bucket: String,
    public_base_url: String,
) -> Result<String, String> {
    let upload_id = upload_id.trim().to_string();
    if upload_id.is_empty() {
        return Err("err_r2_upload_id_missing".to_string());
    }
    state.clear(&upload_id);
    let result = async {
        let config = r2_config(
            &account_id,
            &access_key_id,
            &secret_access_key,
            &endpoint,
            &bucket,
            &public_base_url,
        )?;
        let path = Path::new(&file_path);
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
        if !metadata.is_file() {
            return Err("err_r2_file_not_found".to_string());
        }
        if metadata.len() == 0 {
            return Err("err_r2_file_empty".to_string());
        }

        let key = encoded_key(&object_key);
        if key.is_empty() || key.len() > 1024 {
            return Err("err_r2_invalid_key".to_string());
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60 * 60))
            .build()
            .map_err(|error| format!("err_r2_upload:{error}"))?;
        let progress = Arc::new(UploadProgress {
            app: app.clone(),
            upload_id: upload_id.clone(),
            file_path: file_path.clone(),
            total: metadata.len(),
            uploaded: AtomicU64::new(0),
            started: Instant::now(),
        });
        emit_upload_progress(&progress);
        let media_type = content_type(path);

        if metadata.len() >= MULTIPART_THRESHOLD {
            upload_multipart_object(&client, &state, progress, &config, &key).await?;
        } else {
            upload_single_object(&client, &state, progress, &config, &key, media_type).await?;
        }
        Ok(r2_object_url(&config, &object_key)?)
    }
    .await;
    state.clear(&upload_id);
    result
}

#[tauri::command]
pub fn cancel_r2_upload(state: State<'_, R2UploadState>, upload_id: String) -> Result<(), String> {
    let upload_id = upload_id.trim();
    if upload_id.is_empty() {
        return Err("err_r2_upload_id_missing".to_string());
    }
    state.cancel(upload_id.to_string());
    Ok(())
}
