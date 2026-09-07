//! Cloudflare R2 upload commands for local video files.

use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::File;
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
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("m4v") => "video/x-m4v",
        _ => "video/mp4",
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

fn hash_reader(mut reader: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex_digest(hasher.finalize()))
}

fn hash_file(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
    hash_reader(file).map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))
}

#[cfg(test)]
mod tests {
    use super::hash_reader;
    use std::io::Cursor;

    #[test]
    fn hash_reader_processes_multiple_chunks() {
        let input = vec![b'a'; 65_537];
        let digest = hash_reader(Cursor::new(input)).expect("hashing should succeed");

        assert_eq!(
            digest,
            "008ffc88d3c96a9f307524eb361e47c5222a887fc45fa0c1fb8d429c5c23b430"
        );
    }
}

#[tauri::command]
pub async fn upload_local_video_to_r2(
    file_path: String,
    object_key: String,
    account_id: String,
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    bucket: String,
    public_base_url: String,
) -> Result<String, String> {
    let account_id = account_id.trim().to_string();
    let access_key_id = access_key_id.trim().to_string();
    let secret_access_key = secret_access_key.trim().to_string();
    let endpoint = endpoint.trim().trim_end_matches('/').to_string();
    let bucket = bucket.trim().trim_matches('/').to_string();
    let public_base_url = public_base_url.trim().trim_end_matches('/').to_string();

    if account_id.is_empty()
        || access_key_id.is_empty()
        || secret_access_key.is_empty()
        || endpoint.is_empty()
        || bucket.is_empty()
        || public_base_url.is_empty()
    {
        return Err("err_r2_config_missing".to_string());
    }

    // S3 bucket names never contain `/`, whitespace, `?` or `#`.
    if bucket.contains(['/', '\\', ' ', '\t', '\n', '\r', '?', '#']) || bucket.len() > 63 {
        return Err("err_r2_invalid_bucket".to_string());
    }

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

    // NOTE: `canonical_uri` starts with `/`, so concatenate WITHOUT adding an
    // extra `/` (a `//` prefix would break the SigV4 signature).
    let canonical_uri = format!("/{bucket}/{key}");
    let request_url = format!("{endpoint}{canonical_uri}");
    // Validate the final URL before signing so malformed keys fail fast
    // instead of surfacing as obscure builder errors later.
    reqwest::Url::parse(&request_url).map_err(|error| format!("err_r2_endpoint:{error}"))?;

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
    let hash_path = file_path.clone();
    let payload_hash = tokio::task::spawn_blocking(move || hash_file(Path::new(&hash_path)))
        .await
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))??;
    let media_type = content_type(path);

    let canonical_headers = format!(
        "content-type:{media_type}\nhost:{endpoint_host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{timestamp}\n"
    );
    let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
    let canonical_request =
        format!("PUT\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let scope = format!("{date}/{REGION}/{SERVICE}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request)
    );
    let signature = hex_digest(hmac(
        &signing_key(&date, secret_access_key.trim()),
        &string_to_sign,
    ));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    let file = File::open(path)
        .await
        .map_err(|error| format!("err_r2_file:{}", truncate(&error.to_string(), 300)))?;
    let stream = ReaderStream::new(file);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(|error| format!("err_r2_upload:{error}"))?;
    let response = client
        .put(request_url)
        .header(HOST, endpoint_host)
        .header(CONTENT_TYPE, media_type)
        .header(CONTENT_LENGTH, metadata.len())
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-date", timestamp)
        .header("Authorization", authorization)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .map_err(|error| format!("err_r2_upload:{}", truncate(&error.to_string(), 500)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "err_r2_upload_http:{status}:{}",
            truncate(body.trim(), 1000)
        ));
    }

    Ok(format!("{public_base_url}/{key}"))
}
