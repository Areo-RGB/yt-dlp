//! 本地视频库浏览命令。
//!
//! 目录扫描在 Rust 侧完成，前端只接收视频元数据，避免依赖额外的文件系统插件。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::path::BaseDirectory;
use tauri::Manager;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::utils;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v", "avi"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Serialize)]
pub struct LocalLibrary {
    pub root_path: String,
    pub folders: Vec<LocalFolder>,
}

#[derive(Debug, Serialize)]
pub struct LocalFolder {
    pub name: String,
    pub path: String,
    pub videos: Vec<LocalVideo>,
}

#[derive(Debug, Serialize)]
pub struct LocalVideo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub thumbnail: Option<String>,
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn find_thumbnail(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let parent = path.parent()?;
    IMAGE_EXTENSIONS.iter().find_map(|extension| {
        let candidate = parent.join(format!("{stem}.{extension}"));
        candidate
            .is_file()
            .then(|| candidate.to_string_lossy().into_owned())
    })
}

fn video_info(path: &Path) -> Result<LocalVideo, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("err_local_files_metadata:{error}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());

    Ok(LocalVideo {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        path: path.to_string_lossy().into_owned(),
        size: metadata.len(),
        modified,
        thumbnail: find_thumbnail(path),
    })
}

fn scan_videos(path: &Path) -> Result<Vec<LocalVideo>, String> {
    let mut videos = Vec::new();
    let entries =
        fs::read_dir(path).map_err(|error| format!("err_local_files_read_dir:{error}"))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("err_local_files_read_dir:{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("err_local_files_read_dir:{error}"))?;
        if file_type.is_symlink() {
            continue;
        }
        let file_path = entry.path();
        if file_type.is_file() && is_video(&file_path) {
            videos.push(video_info(&file_path)?);
        }
    }

    videos.sort_by_cached_key(|video| video.name.to_ascii_lowercase());
    Ok(videos)
}

fn canonical_root(root_path: &str) -> Result<PathBuf, String> {
    if root_path.trim().is_empty() {
        return Err("err_local_files_root_not_set".to_string());
    }

    let root =
        fs::canonicalize(root_path).map_err(|error| format!("err_local_files_root:{error}"))?;
    if !root.is_dir() {
        return Err("err_local_files_root_not_dir".to_string());
    }
    Ok(root)
}

#[tauri::command]
pub fn scan_local_files(root_path: String) -> Result<LocalLibrary, String> {
    let root = canonical_root(&root_path)?;
    let mut folders = Vec::new();
    let mut unsorted = scan_videos(&root)?;
    let entries =
        fs::read_dir(&root).map_err(|error| format!("err_local_files_read_dir:{error}"))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("err_local_files_read_dir:{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("err_local_files_read_dir:{error}"))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            folders.push(LocalFolder {
                name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string(),
                path: path.to_string_lossy().into_owned(),
                videos: scan_videos(&path)?,
            });
        }
    }

    if !unsorted.is_empty() {
        unsorted.sort_by_cached_key(|video| video.name.to_ascii_lowercase());
        folders.push(LocalFolder {
            name: "Unsorted".to_string(),
            path: root.to_string_lossy().into_owned(),
            videos: unsorted,
        });
    }

    folders.sort_by_cached_key(|folder| folder.name.to_ascii_lowercase());
    Ok(LocalLibrary {
        root_path: root.to_string_lossy().into_owned(),
        folders,
    })
}

/// 删除库根目录内的单个视频文件，拒绝删除目录外的路径或目录本身。
#[tauri::command]
pub fn delete_local_file(root_path: String, file_path: String) -> Result<(), String> {
    let root = canonical_root(&root_path)?;
    let file =
        fs::canonicalize(&file_path).map_err(|error| format!("err_local_files_file:{error}"))?;
    if !file.starts_with(&root) || file == root || !file.is_file() {
        return Err("err_local_files_outside_root".to_string());
    }
    fs::remove_file(file).map_err(|error| format!("err_local_files_delete:{error}"))
}

/// 删除库根目录内的文件夹，拒绝删除库根目录本身或目录外的路径。
#[tauri::command]
pub fn delete_local_folder(root_path: String, folder_path: String) -> Result<(), String> {
    let root = canonical_root(&root_path)?;
    let folder = fs::canonicalize(&folder_path)
        .map_err(|error| format!("err_local_files_folder:{error}"))?;
    if !folder.starts_with(&root) || folder == root || !folder.is_dir() {
        return Err("err_local_files_outside_root".to_string());
    }
    fs::remove_dir_all(folder).map_err(|error| format!("err_local_files_delete:{error}"))
}

#[tauri::command]
pub fn split_video_chapters(app: tauri::AppHandle, input_path: String) -> Result<(), String> {
    let input = PathBuf::from(&input_path);
    if !input.is_file() {
        return Err("err_split_input_not_file".to_string());
    }

    let script = app
        .path()
        .resolve("scripts/split_chapters.py", BaseDirectory::Resource)
        .ok()
        .filter(|path| path.is_file())
        .or_else(|| {
            let development_path =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/split_chapters.py");
            development_path.is_file().then_some(development_path)
        })
        .ok_or_else(|| "err_split_script_not_found".to_string())?;
    let ffmpeg = utils::get_ffmpeg_path(&app)?;
    let ffprobe = utils::get_ffprobe_path(&app)?;
    let python = which::which("py")
        .or_else(|_| which::which("python"))
        .map_err(|_| "err_split_python_not_found".to_string())?;
    let input_parent = input
        .parent()
        .ok_or_else(|| "err_split_input_not_file".to_string())?;

    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("cmd");
        command
            .arg("/K")
            .arg(&python)
            .arg(&script)
            .arg(&input)
            .arg("--ffmpeg")
            .arg(&ffmpeg)
            .arg("--ffprobe")
            .arg(&ffprobe)
            .current_dir(input_parent)
            .creation_flags(0x00000010);
        command
            .spawn()
            .map_err(|error| format!("err_split_terminal:{error}"))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let (terminal, terminal_args) = [
            ("x-terminal-emulator", ["-e"].as_slice()),
            ("gnome-terminal", ["--"].as_slice()),
            ("konsole", ["-e"].as_slice()),
        ]
        .iter()
        .find_map(|(name, args)| which::which(name).ok().map(|path| (path, *args)))
        .ok_or_else(|| "err_split_terminal_not_found".to_string())?;

        std::process::Command::new(terminal)
            .args(terminal_args)
            .arg(&python)
            .arg(&script)
            .arg(&input)
            .arg("--ffmpeg")
            .arg(&ffmpeg)
            .arg("--ffprobe")
            .arg(&ffprobe)
            .current_dir(input_parent)
            .spawn()
            .map_err(|error| format!("err_split_terminal:{error}"))?;
        Ok(())
    }
}
