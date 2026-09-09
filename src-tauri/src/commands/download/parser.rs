//! yt-dlp 输出解析模块（优化版：基于借用切片与零堆分配字段清洗）

/// 进度信息
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressInfo {
    pub percent: f64,
    pub speed: String,
    pub eta: String,
    pub downloaded: String,
    pub total: String,
    pub fragment_index: Option<u64>,
    pub fragment_count: Option<u64>,
    pub status: String,
}

/// 强类型借用切片反序列化结构体（避免 serde_json::Value 的 AST 树遍历与字符串堆分配）
#[derive(serde::Deserialize)]
struct RawProgress<'a> {
    #[serde(default, borrow)]
    percent: Option<&'a str>,
    #[serde(default, borrow)]
    speed: Option<&'a str>,
    #[serde(default, borrow)]
    eta: Option<&'a str>,
    #[serde(default, borrow)]
    downloaded: Option<&'a str>,
    #[serde(default, borrow)]
    total: Option<&'a str>,
    #[serde(default, rename = "fragmentIndex", deserialize_with = "de_flex_u64")]
    fragment_index: Option<u64>,
    #[serde(default, rename = "fragmentCount", deserialize_with = "de_flex_u64")]
    fragment_count: Option<u64>,
    #[serde(default, borrow)]
    status: Option<&'a str>,
}

/// 灵活解析 u64（兼容 JSON 整数如 14 与字符串整数如 "14"、空值、"NA"）
fn de_flex_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct FlexVisitor;
    impl<'de> serde::de::Visitor<'de> for FlexVisitor {
        type Value = Option<u64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("integer, string integer, or null")
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(if v >= 0 { Some(v as u64) } else { None })
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            let t = v.trim();
            if t.is_empty() || t.eq_ignore_ascii_case("na") {
                Ok(None)
            } else {
                Ok(t.parse::<u64>().ok())
            }
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(self)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    deserializer.deserialize_any(FlexVisitor)
}

/// 零堆分配大小写不敏感判断：检查字段是否包含 NA/Unknown 等无效占位符
/// 相比原有实现避免了调用 .to_ascii_lowercase() 带来的堆分配
#[inline]
pub fn is_clean_invalid(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if t.eq_ignore_ascii_case("na")
        || t.eq_ignore_ascii_case("n/a")
        || t.eq_ignore_ascii_case("none")
        || t.eq_ignore_ascii_case("null")
    {
        return true;
    }
    if t.len() >= 7 && t.as_bytes().windows(7).any(|w| w.eq_ignore_ascii_case(b"unknown")) {
        return true;
    }
    if t.len() >= 3 && t.as_bytes().windows(3).any(|w| w.eq_ignore_ascii_case(b"n/a")) {
        return true;
    }
    false
}

/// 清理 yt-dlp 输出字段：移除 NA/Unknown 等无效值（零分配判断）
#[inline]
fn clean_field(s: Option<&str>) -> String {
    match s {
        Some(v) => {
            let trimmed = v.trim();
            if is_clean_invalid(trimmed) {
                String::new()
            } else {
                trimmed.to_string()
            }
        }
        None => String::new(),
    }
}

/// 解析 --progress-template 输出的 JSON 进度行
/// 格式: PROGRESS_JSON:{"percent":" 45.2%","speed":"2.50MiB/s","eta":"00:11","downloaded":"22.68MiB","total":"50.35MiB"}
pub fn parse_progress_json(line: &str) -> Option<ProgressInfo> {
    let json_str = line.strip_prefix("PROGRESS_JSON:")?;
    let raw: RawProgress = serde_json::from_str(json_str).ok()?;

    let percent_str = raw.percent.unwrap_or("0%");
    let percent: f64 = percent_str
        .trim()
        .trim_end_matches('%')
        .parse()
        .unwrap_or(0.0);

    let speed = clean_field(raw.speed);
    let eta = clean_field(raw.eta);
    let downloaded = clean_field(raw.downloaded);
    let total = clean_field(raw.total);
    let fragment_index = raw.fragment_index;
    let fragment_count = raw.fragment_count;
    let status = raw.status.unwrap_or("downloading").to_string();

    Some(ProgressInfo {
        percent,
        speed,
        eta,
        downloaded,
        total,
        fragment_index,
        fragment_count,
        status,
    })
}

/// 解析 ffmpeg 输出中的 time= 字段，返回已处理的秒数
/// 格式: frame= 1234 fps=128 ... time=00:02:29.65 ...
pub fn parse_ffmpeg_time(line: &str) -> Option<f64> {
    let time_start = line.find("time=")?;
    let after = &line[time_start + 5..];
    let time_str = after.split_whitespace().next()?;
    // 格式: HH:MM:SS.xx 或 -HH:MM:SS.xx (负值表示尚未开始)
    if time_str.starts_with('-') || time_str == "N/A" {
        return None;
    }
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let s: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// 解析 ffmpeg 输出中的 speed= 字段，返回可读速度字符串
/// 格式: speed=2.13x 或 speed=N/A
pub fn parse_ffmpeg_speed(line: &str) -> String {
    if let Some(speed_start) = line.find("speed=") {
        let after = &line[speed_start + 6..];
        let speed_str = after.split_whitespace().next().unwrap_or("");
        // 排除 N/A 等无效值
        if !speed_str.is_empty() && speed_str != "N/A" && speed_str != "Unknown" {
            return speed_str.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_json_valid_input_returns_info() {
        let line = r#"PROGRESS_JSON:{"percent":" 45.2%","speed":"2.50MiB/s","eta":"00:11","downloaded":"22.68MiB","total":"50.35MiB"}"#;
        let info = parse_progress_json(line).unwrap();
        assert!((info.percent - 45.2).abs() < 0.01);
        assert_eq!(info.speed, "2.50MiB/s");
        assert_eq!(info.eta, "00:11");
        assert_eq!(info.downloaded, "22.68MiB");
        assert_eq!(info.total, "50.35MiB");
        assert_eq!(info.status, "downloading");
    }

    #[test]
    fn parse_progress_json_100_percent_returns_full() {
        let line = r#"PROGRESS_JSON:{"percent":"100%","speed":"","eta":"","downloaded":"50.35MiB","total":"50.35MiB"}"#;
        let info = parse_progress_json(line).unwrap();
        assert!((info.percent - 100.0).abs() < 0.01);
    }

    #[test]
    fn parse_progress_json_na_values_returns_empty_strings() {
        let line = r#"PROGRESS_JSON:{"percent":"0%","speed":"NA","eta":"Unknown","downloaded":"N/A","total":"N/A"}"#;
        let info = parse_progress_json(line).unwrap();
        assert!(info.speed.is_empty());
        assert!(info.eta.is_empty());
        assert!(info.downloaded.is_empty());
        assert!(info.total.is_empty());
    }

    #[test]
    fn parse_progress_json_unknown_speed_with_unit_returns_empty() {
        let line = r#"PROGRESS_JSON:{"percent":"0%","speed":"Unknown B/s","eta":"","downloaded":"","total":""}"#;
        let info = parse_progress_json(line).unwrap();
        assert!(info.speed.is_empty());
    }

    #[test]
    fn parse_progress_json_no_prefix_returns_none() {
        assert!(parse_progress_json("some random line").is_none());
    }

    #[test]
    fn parse_progress_json_invalid_json_returns_none() {
        assert!(parse_progress_json("PROGRESS_JSON:{not valid json}").is_none());
    }

    #[test]
    fn parse_progress_json_with_fragments() {
        // 数字格式 fragment
        let line1 = r#"PROGRESS_JSON:{"percent":"50.0%","fragmentIndex":12,"fragmentCount":24}"#;
        let info1 = parse_progress_json(line1).unwrap();
        assert_eq!(info1.fragment_index, Some(12));
        assert_eq!(info1.fragment_count, Some(24));

        // 字符串格式 fragment（yt-dlp 模板常见输出）
        let line2 = r#"PROGRESS_JSON:{"percent":"50.0%","fragmentIndex":"15","fragmentCount":"30"}"#;
        let info2 = parse_progress_json(line2).unwrap();
        assert_eq!(info2.fragment_index, Some(15));
        assert_eq!(info2.fragment_count, Some(30));

        // 无效值 "NA"
        let line3 = r#"PROGRESS_JSON:{"percent":"50.0%","fragmentIndex":"NA","fragmentCount":"NA"}"#;
        let info3 = parse_progress_json(line3).unwrap();
        assert_eq!(info3.fragment_index, None);
        assert_eq!(info3.fragment_count, None);
    }

    #[test]
    fn parse_progress_json_custom_status() {
        let line = r#"PROGRESS_JSON:{"percent":"100%","status":"postprocessing"}"#;
        let info = parse_progress_json(line).unwrap();
        assert_eq!(info.status, "postprocessing");
    }

    #[test]
    fn parse_ffmpeg_time_valid_input_returns_seconds() {
        let line = "frame= 1234 fps=128 q=28.0 size=   15360kB time=00:02:29.65 bitrate= 840.2kbits/s speed=2.13x";
        let secs = parse_ffmpeg_time(line).unwrap();
        assert!((secs - 149.65).abs() < 0.01);
    }

    #[test]
    fn parse_ffmpeg_time_zero_returns_zero() {
        let line =
            "frame=    1 fps=0.0 q=0.0 size=       0kB time=00:00:00.00 bitrate=N/A speed=N/A";
        let secs = parse_ffmpeg_time(line).unwrap();
        assert!((secs - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_ffmpeg_time_negative_returns_none() {
        let line =
            "frame=    0 fps=0.0 q=0.0 size=       0kB time=-577014:32:22.77 bitrate=N/A speed=N/A";
        assert!(parse_ffmpeg_time(line).is_none());
    }

    #[test]
    fn parse_ffmpeg_time_na_returns_none() {
        let line = "frame=    0 fps=0.0 q=0.0 size=       0kB time=N/A bitrate=N/A speed=N/A";
        assert!(parse_ffmpeg_time(line).is_none());
    }

    #[test]
    fn parse_ffmpeg_time_no_time_field_returns_none() {
        assert!(parse_ffmpeg_time("some random line without time field").is_none());
    }

    #[test]
    fn clean_field_none_returns_empty() {
        assert!(clean_field(None).is_empty());
    }

    #[test]
    fn clean_field_empty_string_returns_empty() {
        assert!(clean_field(Some("")).is_empty());
        assert!(clean_field(Some("   ")).is_empty());
    }

    #[test]
    fn clean_field_na_values_returns_empty() {
        assert!(clean_field(Some("NA")).is_empty());
        assert!(clean_field(Some("Unknown")).is_empty());
        assert!(clean_field(Some("N/A")).is_empty());
        assert!(clean_field(Some("none")).is_empty());
        assert!(clean_field(Some("null")).is_empty());
    }

    #[test]
    fn clean_field_valid_value_returns_trimmed() {
        assert_eq!(clean_field(Some("2.50MiB/s")), "2.50MiB/s");
        assert_eq!(clean_field(Some("  00:11  ")), "00:11");
    }

    #[test]
    fn parse_ffmpeg_speed_valid_returns_speed() {
        let line = "frame= 1234 fps=128 q=28.0 size=   15360kB time=00:02:29.65 bitrate= 840.2kbits/s speed=2.13x";
        assert_eq!(parse_ffmpeg_speed(line), "2.13x");
    }

    #[test]
    fn parse_ffmpeg_speed_na_returns_empty() {
        let line = "frame=    0 fps=0.0 q=0.0 size=       0kB time=00:00:00.00 bitrate=N/A speed=N/A";
        assert!(parse_ffmpeg_speed(line).is_empty());
    }

    #[test]
    fn parse_ffmpeg_speed_no_field_returns_empty() {
        assert!(parse_ffmpeg_speed("some random line without speed field").is_empty());
    }

    #[test]
    fn test_is_clean_invalid_checks() {
        assert!(is_clean_invalid(""));
        assert!(is_clean_invalid("   "));
        assert!(is_clean_invalid("na"));
        assert!(is_clean_invalid("NA"));
        assert!(is_clean_invalid("N/A"));
        assert!(is_clean_invalid("none"));
        assert!(is_clean_invalid("NULL"));
        assert!(is_clean_invalid("unknown"));
        assert!(is_clean_invalid("Unknown"));
        assert!(is_clean_invalid("Unknown B/s"));
        assert!(is_clean_invalid("download N/A info"));
        assert!(!is_clean_invalid("12.5 MiB/s"));
        assert!(!is_clean_invalid("01:23"));
    }
}
