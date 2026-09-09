//! yt-dlp/FFmpeg 输出解析与下载事件分发（优化版：8KB 块读取、IPC 节流、非阻塞文件 I/O）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::time::Instant;

use super::model::DownloadProcessInfo;
use super::parser;

/// 进度事件 IPC 负载结构体（避免 serde_json::json! 动态 Map 分配）
#[derive(serde::Serialize)]
struct ProgressEventPayload<'a> {
    id: &'a str,
    percent: f64,
    speed: &'a str,
    eta: &'a str,
    downloaded: &'a str,
    total: &'a str,
    #[serde(rename = "fragmentIndex")]
    fragment_index: Option<u64>,
    #[serde(rename = "fragmentCount")]
    fragment_count: Option<u64>,
    status: &'a str,
}

/// 日志事件 IPC 负载结构体
#[derive(serde::Serialize)]
struct LogEventPayload<'a> {
    id: &'a str,
    line: &'a str,
}

/// 完成事件 IPC 负载结构体
#[derive(serde::Serialize)]
struct CompleteEventPayload<'a> {
    id: &'a str,
    #[serde(rename = "outputFile")]
    output_file: &'a str,
}

/// 错误事件 IPC 负载结构体
#[derive(serde::Serialize)]
struct ErrorEventPayload<'a> {
    id: &'a str,
    error: &'a str,
}

/// 下载进度事件节流器（基于 tokio::time::Instant）
///
/// 保证：
/// 1. 初始首个进度事件立即发送，提供即时视觉反馈。
/// 2. 状态或阶段切换（如 "downloading" -> "postprocessing"）立即发送。
/// 3. 100% 完成关键节点不被丢弃或延迟。
/// 4. 连续中间进度按指定时间间隔节流（默认 100ms = 10Hz）。
pub struct ProgressThrottler {
    last_emit: Option<Instant>,
    throttle_interval: Duration,
    last_percent: f64,
    last_status: Option<String>,
}

impl Default for ProgressThrottler {
    fn default() -> Self {
        Self::new(Duration::from_millis(100))
    }
}

impl ProgressThrottler {
    /// 创建指定节流周期的进度节流器（推荐 100ms 即 10Hz）
    pub fn new(interval: Duration) -> Self {
        Self {
            last_emit: None,
            throttle_interval: interval,
            last_percent: -1.0,
            last_status: None,
        }
    }

    /// 判定当前进度更新是否应派发给 Tauri IPC
    pub fn should_emit(&mut self, percent: f64, status: &str) -> bool {
        let now = Instant::now();

        // 1. 首次事件：始终立即派发
        if self.last_emit.is_none() {
            self.record_emit(now, percent, status);
            return true;
        }

        // 2. 状态或阶段变更（如从 downloading 转为 postprocessing）：立即派发
        if self.last_status.as_deref() != Some(status) {
            self.record_emit(now, percent, status);
            return true;
        }

        // 3. 100% 完成节点：立即派发
        if percent >= 100.0 && self.last_percent < 100.0 {
            self.record_emit(now, percent, status);
            return true;
        }

        // 4. 普通连续数字进度：按 throttle_interval 周期限制频率
        let last = self.last_emit.unwrap();
        if now.duration_since(last) >= self.throttle_interval {
            self.record_emit(now, percent, status);
            true
        } else {
            false
        }
    }

    #[inline]
    fn record_emit(&mut self, now: Instant, percent: f64, status: &str) {
        self.last_emit = Some(now);
        self.last_percent = percent;
        if self.last_status.as_deref() != Some(status) {
            self.last_status = Some(status.to_string());
        }
    }
}

/// 将秒数格式化为 HH:MM:SS
fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// 处理 yt-dlp/ffmpeg 的一行输出：解析进度（经节流过滤）并发送事件到前端
fn process_output_line(
    app: &AppHandle,
    task_id: &str,
    processes: &Arc<Mutex<HashMap<String, DownloadProcessInfo>>>,
    line: &str,
    throttler: &mut ProgressThrottler,
) {
    if line.starts_with("ERROR:") {
        if let Ok(mut map) = processes.lock() {
            if let Some(info) = map.get_mut(task_id) {
                info.last_error = Some(line.to_string());
            }
        }
    }

    // 1. 解析 --progress-template 输出的 JSON 进度
    if let Some(info) = parser::parse_progress_json(line) {
        if throttler.should_emit(info.percent, &info.status) {
            let payload = ProgressEventPayload {
                id: task_id,
                percent: info.percent,
                speed: &info.speed,
                eta: &info.eta,
                downloaded: &info.downloaded,
                total: &info.total,
                fragment_index: info.fragment_index,
                fragment_count: info.fragment_count,
                status: &info.status,
            };
            let _ = app.emit("download-progress", &payload);
        }
        return; // 进度行不需要转发到日志
    }

    // 2. 解析 ffmpeg 输出中的 time= 字段（用于时间裁剪场景的进度）
    if line.contains("time=") && line.contains("frame=") {
        if let Some(current_secs) = parser::parse_ffmpeg_time(line) {
            let clip_dur = processes
                .lock()
                .ok()
                .and_then(|map| map.get(task_id).and_then(|info| info.clip_duration));
            if let Some(duration) = clip_dur {
                let percent = (current_secs / duration * 100.0).min(100.0);
                if throttler.should_emit(percent, "downloading") {
                    let ffmpeg_speed = parser::parse_ffmpeg_speed(line);
                    let downloaded_str = format_duration(current_secs);
                    let total_str = format_duration(duration);
                    let payload = ProgressEventPayload {
                        id: task_id,
                        percent,
                        speed: &ffmpeg_speed,
                        eta: "",
                        downloaded: &downloaded_str,
                        total: &total_str,
                        fragment_index: None,
                        fragment_count: None,
                        status: "downloading",
                    };
                    let _ = app.emit("download-progress", &payload);
                }
            }
        }
        return; // ffmpeg 帧进度不转发到日志
    }

    // 3. 跟踪输出文件路径（从 [download] Destination 等行解析，作为备选方案）
    if let Some(dest) = parse_destination(line) {
        if let Ok(mut map) = processes.lock() {
            if let Some(info) = map.get_mut(task_id) {
                info.output_files.push(dest);
            }
        }
    }

    // 4. 转发日志到前端（不含进度 JSON 行，保持日志清晰）
    let payload = LogEventPayload {
        id: task_id,
        line,
    };
    let _ = app.emit("download-log", &payload);
}

/// 从 yt-dlp 输出行中解析目标文件路径（备选方案，可能有编码问题）
fn parse_destination(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // [download] Destination: /path/to/file.ext
    if let Some(rest) = trimmed.strip_prefix("[download] Destination: ") {
        return Some(rest.trim().to_string());
    }
    // [download] /path/to/file.ext has already been downloaded
    if trimmed.starts_with("[download] ") && trimmed.ends_with("has already been downloaded") {
        let inner = trimmed
            .strip_prefix("[download] ")?
            .strip_suffix("has already been downloaded")?
            .trim();
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    // [Merger] Merging formats into "file.ext"
    if trimmed.contains("[Merger] Merging formats into") {
        let start = trimmed.find('"')? + 1;
        let end = trimmed.rfind('"')?;
        if start < end {
            return Some(trimmed[start..end].to_string());
        }
    }
    None
}

/// 从临时文件中读取 yt-dlp --print-to-file 写出的最终文件路径
/// 返回最后一行（播放列表可能有多行）
async fn read_filepath_from_file(filepath_file: &str) -> Option<String> {
    let content = tokio::fs::read_to_string(filepath_file).await.ok()?;
    let last_line = content.trim().lines().last()?.trim().to_string();
    if last_line.is_empty() {
        None
    } else {
        Some(last_line)
    }
}

/// 启动异步任务读取子进程输出流（优化版：8KB 块缓冲区与切片扫描，替换逐字节读取）
/// 同时处理 \n 和 \r 作为行分隔符（ffmpeg 进度输出使用 \r）
pub(super) fn spawn_output_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    app: AppHandle,
    task_id: String,
    processes: Arc<Mutex<HashMap<String, DownloadProcessInfo>>>,
    mut reader: R,
) {
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        const CHUNK_SIZE: usize = 8192;
        const MAX_LINE_LEN: usize = 64 * 1024; // 64KB 行安全上限

        let mut chunk_buf = [0u8; CHUNK_SIZE];
        let mut line_buf = Vec::with_capacity(1024);
        let mut throttler = ProgressThrottler::new(Duration::from_millis(100));

        loop {
            match reader.read(&mut chunk_buf).await {
                Ok(0) => {
                    // EOF：处理缓冲区中剩余的内容
                    if !line_buf.is_empty() {
                        let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                        if !line.is_empty() {
                            process_output_line(&app, &task_id, &processes, &line, &mut throttler);
                        }
                    }
                    break;
                }
                Ok(n) => {
                    let mut start = 0;
                    while start < n {
                        let slice = &chunk_buf[start..n];
                        // 切片扫描 \n 或 \r 分隔符
                        let delimiter_rel_idx = slice.iter().position(|&b| b == b'\n' || b == b'\r');

                        match delimiter_rel_idx {
                            Some(rel_idx) => {
                                let delimiter_abs_idx = start + rel_idx;
                                let segment = &chunk_buf[start..delimiter_abs_idx];

                                if line_buf.len() + segment.len() <= MAX_LINE_LEN {
                                    line_buf.extend_from_slice(segment);
                                }

                                if !line_buf.is_empty() {
                                    let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                                    if !line.is_empty() {
                                        process_output_line(
                                            &app,
                                            &task_id,
                                            &processes,
                                            &line,
                                            &mut throttler,
                                        );
                                    }
                                    line_buf.clear();
                                }
                                start = delimiter_abs_idx + 1;
                            }
                            None => {
                                // 块内无更多分隔符，将剩余部分暂存到行缓冲区
                                let remaining = &chunk_buf[start..n];
                                if line_buf.len() + remaining.len() <= MAX_LINE_LEN {
                                    line_buf.extend_from_slice(remaining);
                                }
                                start = n;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// 启动异步任务等待子进程完成并发送结果事件
pub(super) fn spawn_completion_handler(
    app: AppHandle,
    task_id: String,
    processes: Arc<Mutex<HashMap<String, DownloadProcessInfo>>>,
    mut child: tokio::process::Child,
) {
    tokio::spawn(async move {
        let status = child.wait().await;

        let was_cancelled = processes
            .lock()
            .ok()
            .and_then(|map| map.get(&task_id).map(|info| info.cancelled))
            .unwrap_or(false);

        // 仅以 yt-dlp 退出码判定成功；不能用「日志里见过 Destination 行」做兜底，
        // 因为 yt-dlp 在开始写字节前就会先打印目标路径，下载半路超时也会留下这一行。
        let success = matches!(&status, Ok(s) if s.success());

        if success {
            let (output_file, _) = resolve_output_file(&processes, &task_id).await;
            let payload = CompleteEventPayload {
                id: &task_id,
                output_file: &output_file,
            };
            let _ = app.emit("download-complete", &payload);
        } else if !was_cancelled {
            // 失败时仍清理 --print-to-file 临时文件，避免遗留
            let _ = resolve_output_file(&processes, &task_id).await;
            let error_msg = processes
                .lock()
                .ok()
                .and_then(|map| map.get(&task_id).and_then(|info| info.last_error.clone()))
                .unwrap_or_else(|| {
                    status
                        .as_ref()
                        .map(|s| format!("err_exit_code:{}", s.code().unwrap_or(-1)))
                        .unwrap_or_else(|e| e.to_string())
                });
            let payload = ErrorEventPayload {
                id: &task_id,
                error: &error_msg,
            };
            let _ = app.emit("download-error", &payload);
        }

        // 清理进程记录
        if let Ok(mut map) = processes.lock() {
            map.remove(&task_id);
        }
    });
}

/// 解析最终输出文件路径
/// 优先从 --print-to-file 临时文件读取（UTF-8 可靠），回退到 stdout 解析结果
/// 避免在持有进程锁时执行文件 I/O
async fn resolve_output_file(
    processes: &Arc<Mutex<HashMap<String, DownloadProcessInfo>>>,
    task_id: &str,
) -> (String, bool) {
    // 1. 作用域锁：快速提取元数据并立即释放锁，防止阻塞其他任务
    let (fp_file, fallback_file, download_dir, has_output_files) = {
        let Ok(map) = processes.lock() else {
            return (String::new(), false);
        };
        let Some(info) = map.get(task_id) else {
            return (String::new(), false);
        };
        (
            info.filepath_file.clone(),
            info.output_files.last().cloned(),
            info.download_dir.clone(),
            !info.output_files.is_empty(),
        )
    }; // MutexGuard 在此处离开作用域并释放

    let mut file = String::new();

    // 2. 在锁外执行非阻塞的异步文件读取与删除
    if let Some(ref fp) = fp_file {
        if let Some(path) = read_filepath_from_file(fp).await {
            file = path;
        }
        let _ = tokio::fs::remove_file(fp).await;
    }

    // 3. 回退：从 stdout 解析的路径
    if file.is_empty() {
        file = fallback_file.unwrap_or_default();
        if !file.is_empty() && !std::path::Path::new(&file).is_absolute() {
            file = std::path::PathBuf::from(&download_dir)
                .join(&file)
                .to_string_lossy()
                .to_string();
        }
    }

    let has = has_output_files || !file.is_empty();
    (file, has)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_throttler_emits_first_event_immediately() {
        let mut throttler = ProgressThrottler::new(Duration::from_millis(100));
        assert!(throttler.should_emit(0.0, "downloading"));
        // 100ms 窗口内的后续调用被限流
        assert!(!throttler.should_emit(5.0, "downloading"));
    }

    #[tokio::test]
    async fn test_throttler_emits_after_interval_elapsed() {
        let mut throttler = ProgressThrottler::new(Duration::from_millis(20));
        assert!(throttler.should_emit(0.0, "downloading"));
        assert!(!throttler.should_emit(10.0, "downloading"));

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(throttler.should_emit(20.0, "downloading"));
    }

    #[tokio::test]
    async fn test_throttler_always_emits_100_percent_immediately() {
        let mut throttler = ProgressThrottler::new(Duration::from_millis(1000));
        assert!(throttler.should_emit(50.0, "downloading"));

        // 即使在 1000ms 窗口期内，100% 完成也必须立即发送
        assert!(throttler.should_emit(100.0, "downloading"));
    }

    #[tokio::test]
    async fn test_throttler_always_emits_status_transition_immediately() {
        let mut throttler = ProgressThrottler::new(Duration::from_millis(1000));
        assert!(throttler.should_emit(99.0, "downloading"));

        // 状态转换为 postprocessing 时必须立即发送
        assert!(throttler.should_emit(99.0, "postprocessing"));
        // 同一阶段内继续受限流控制
        assert!(!throttler.should_emit(99.5, "postprocessing"));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0.0), "00:00");
        assert_eq!(format_duration(59.0), "00:59");
        assert_eq!(format_duration(61.0), "01:01");
        assert_eq!(format_duration(3661.0), "01:01:01");
    }

    #[test]
    fn test_parse_destination() {
        assert_eq!(
            parse_destination("[download] Destination: /tmp/test.mp4"),
            Some("/tmp/test.mp4".to_string())
        );
        assert_eq!(
            parse_destination("[download] /tmp/test.mp4 has already been downloaded"),
            Some("/tmp/test.mp4".to_string())
        );
        assert_eq!(
            parse_destination(r#"[Merger] Merging formats into "/tmp/test.mp4""#),
            Some("/tmp/test.mp4".to_string())
        );
        assert_eq!(parse_destination("other line"), None);
    }
}
