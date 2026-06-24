use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};
use std::io::{self, Read};
use std::fs::File;
use std::collections::HashMap;
use rayon::prelude::*;
use tauri::Emitter;
use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DuplicateFile {
    pub filename: String,
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub modified: u64, // Epoch milliseconds
    pub created: u64,  // Epoch milliseconds
    pub read_only: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<DuplicateFile>,
}

#[derive(Deserialize, Debug)]
pub struct ResolutionItem {
    pub target: String,
    pub original: String,
}

fn write_to_log_file(message: &str) {
    if let Ok(home) = std::env::var("HOME") {
        let log_dir = Path::new(&home).join(".config").join("auDO-File-Z");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("app.log");
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
            
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(file, "[Epoch {}] {}", now, message);
        }
    }
}

#[tauri::command]
fn read_log_file() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let log_path = Path::new(&home).join(".config").join("auDO-File-Z").join("app.log");
    if !log_path.exists() {
        return Ok("No log records found yet.".to_string());
    }
    std::fs::read_to_string(&log_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_log_file() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let log_path = Path::new(&home).join(".config").join("auDO-File-Z").join("app.log");
    if log_path.exists() {
        std::fs::remove_file(&log_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn check_fda() -> bool {
    write_to_log_file("FDA Check executed.");
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        write_to_log_file("FDA Check failed: HOME env var not set.");
        return false;
    }
    let path = Path::new(&home).join("Library/Safari/Bookmarks.db");
    match File::open(path) {
        Ok(_) => {
            write_to_log_file("FDA status verified: Active.");
            true
        }
        Err(e) => {
            let has_fda = e.kind() != io::ErrorKind::PermissionDenied;
            write_to_log_file(&format!(
                "FDA status verified: {}. (File open error: {:?})",
                if has_fda { "Active" } else { "Restricted" },
                e
            ));
            has_fda
        }
    }
}

#[tauri::command]
fn open_folder_picker() -> Option<String> {
    let res = rfd::FileDialog::new().pick_folder();
    res.map(|p| p.to_string_lossy().into_owned())
}

// Helper to parse comma-separated extensions: e.g. ".mp3, .wav" -> ["mp3", "wav"]
// Strips all leading dots, trims whitespace, and converts to lowercase
fn parse_extensions(ext_str: &str) -> Vec<String> {
    ext_str
        .split(',')
        .map(|s| s.trim().to_lowercase().trim_start_matches('.').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// Recursive directory crawler (skips hidden files/directories and symlinks)
fn crawl_directory(
    dir: &Path,
    extensions: &[String],
    files: &mut Vec<(PathBuf, u64, u64, u64, bool)>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return, // Gracefully ignore unreadable/restricted folders
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        
        // Skip hidden files/directories (names starting with '.')
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        // Ignore symlinks as per target policy
        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            crawl_directory(&path, extensions, files);
        } else if file_type.is_file() {
            let (size, modified, created, read_only) = match entry.metadata() {
                Ok(meta) => {
                    let sz = meta.len();
                    let mod_time = meta.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let cre_time = meta.created()
                        .or_else(|_| meta.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let ro = meta.permissions().readonly();
                    (sz, mod_time, cre_time, ro)
                }
                Err(_) => continue,
            };

            // Filter by extension if constraints are provided
            if extensions.is_empty() {
                files.push((path, size, modified, created, read_only));
            } else {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if extensions.iter().any(|e| e == &ext_lower) {
                        files.push((path, size, modified, created, read_only));
                    }
                }
            }
        }
    }
}

// Chunk-buffered hashing (constant 8KB memory footprint)
fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Default)]
pub struct ScanState {
    pub is_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[tauri::command]
fn cancel_scan(state: tauri::State<'_, ScanState>) {
    state.is_cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    write_to_log_file("Scan cancellation requested by user.");
    println!("DEBUG: Scan cancellation requested.");
}

fn run_scan_internal<F>(
    path: &str,
    extensions: &str,
    is_cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress_fn: Option<F>,
) -> Result<Vec<DuplicateGroup>, String>
where
    F: Fn(usize) + Send + Sync,
{
    let target_path = Path::new(&path);
    if !target_path.exists() || !target_path.is_dir() {
        write_to_log_file(&format!("Scan failed: target path '{}' does not exist or is not a directory.", path));
        return Err("Target path does not exist or is not a directory.".to_string());
    }

    let parsed_exts = parse_extensions(extensions);
    write_to_log_file(&format!("Starting scan on directory '{}' with extensions '{}'", path, extensions));
    println!("DEBUG: Starting scan on directory: '{}'", path);
    println!("DEBUG: Raw extensions input: '{}'", extensions);
    println!("DEBUG: Parsed extensions filter: {:?}", parsed_exts);

    let mut all_files = Vec::new();
    crawl_directory(target_path, &parsed_exts, &mut all_files);

    write_to_log_file(&format!("Crawler found {} total files matching filter.", all_files.len()));
    println!("DEBUG: Crawler found {} total files matching filter.", all_files.len());

    if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        write_to_log_file("Scan cancelled by user before pre-filtering.");
        return Ok(Vec::new());
    }

    // Step 1: Pre-filter by size (group files by size)
    let mut size_groups: HashMap<u64, Vec<(PathBuf, u64, u64, bool)>> = HashMap::new();
    for (file_path, size, modified, created, read_only) in &all_files {
        size_groups.entry(*size).or_default().push((file_path.clone(), *modified, *created, *read_only));
    }

    let total_unique_sizes = size_groups.len();
    let candidate_groups: Vec<(u64, Vec<(PathBuf, u64, u64, bool)>)> = size_groups
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .collect();

    let candidate_files_count: usize = candidate_groups.iter().map(|(_, paths)| paths.len()).sum();
    write_to_log_file(&format!(
        "Grouped into {} unique file sizes. Found {} candidate files sharing sizes (pre-filtered {} unique files).",
        total_unique_sizes,
        candidate_files_count,
        all_files.len() - candidate_files_count
    ));
    println!(
        "DEBUG: Grouped into {} unique file sizes. Found {} candidate files sharing sizes (pre-filtered {} unique files).",
        total_unique_sizes,
        candidate_files_count,
        all_files.len() - candidate_files_count
    );

    if candidate_files_count == 0 {
        write_to_log_file("Scan complete: No candidate files sharing sizes found.");
        return Ok(Vec::new());
    }

    // Step 2: Compute SHA-256 hashes
    let processed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut hashed_files = Vec::new();

    for (size, paths) in candidate_groups {
        if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            write_to_log_file("Scan cancelled by user during hashing.");
            return Ok(Vec::new());
        }

        let group_hashes: Vec<(String, u64, PathBuf, u64, u64, bool)> = paths
            .into_par_iter()
            .filter_map(|(p, modified, created, read_only)| {
                if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return None;
                }
                match hash_file(&p) {
                    Ok(hash) => {
                        let current = processed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let pct = (current * 100) / candidate_files_count;
                        if let Some(ref pf) = progress_fn {
                            pf(pct);
                        }
                        Some((hash, size, p, modified, created, read_only))
                    }
                    Err(e) => {
                        write_to_log_file(&format!("Warning: Failed to hash file '{}': {}", p.display(), e));
                        println!("DEBUG: Failed to hash file '{}': {}", p.display(), e);
                        let current = processed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let pct = (current * 100) / candidate_files_count;
                        if let Some(ref pf) = progress_fn {
                            pf(pct);
                        }
                        None
                    }
                }
            })
            .collect();

        hashed_files.extend(group_hashes);
    }

    if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        write_to_log_file("Scan cancelled by user after hashing completed.");
        return Ok(Vec::new());
    }

    // Step 3: Group hashed files by their cryptographic hash
    let mut hash_groups: HashMap<String, (u64, Vec<DuplicateFile>)> = HashMap::new();
    for (hash, size, p, modified, created, read_only) in hashed_files {
        let filename = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let file_path_str = p.to_string_lossy().into_owned();
        
        let entry = hash_groups.entry(hash.clone()).or_insert_with(|| (size, Vec::new()));
        entry.1.push(DuplicateFile {
            filename,
            path: file_path_str,
            size,
            hash,
            modified,
            created,
            read_only,
        });
    }

    // Step 4: Retain duplicate clusters (hash groups with >= 2 entries)
    let mut result = Vec::new();
    for (hash, (size, files)) in hash_groups {
        if files.len() >= 2 {
            result.push(DuplicateGroup {
                hash,
                size,
                files,
            });
        }
    }

    // Sort duplicates by file size descending (largest wastes first)
    result.sort_by(|a, b| b.size.cmp(&a.size));
    write_to_log_file(&format!("Found {} duplicate clusters after cryptographic hashing.", result.len()));
    println!("DEBUG: Found {} duplicate clusters after cryptographic hashing.", result.len());
    Ok(result)
}

#[tauri::command]
async fn start_scan(
    app: tauri::AppHandle,
    state: tauri::State<'_, ScanState>,
    path: String,
    extensions: String,
) -> Result<Vec<DuplicateGroup>, String> {
    state.is_cancelled.store(false, std::sync::atomic::Ordering::Relaxed);
    let progress_fn = move |pct| {
        let _ = app.emit("scan-progress", pct);
    };
    run_scan_internal(&path, &extensions, &state.is_cancelled, Some(progress_fn))
}

fn get_unique_trash_path(trash_dir: &Path, filename: &str) -> PathBuf {
    let mut dest = trash_dir.join(filename);
    if !dest.exists() {
        return dest;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| format!(".{}", e)).unwrap_or_default();
    
    let mut counter = 1;
    loop {
        let new_name = format!("{}_{}{}", stem, counter, ext);
        dest = trash_dir.join(&new_name);
        if !dest.exists() {
            return dest;
        }
        counter += 1;
    }
}

fn move_to_trash_silent(src: &Path) -> std::io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "HOME env var not set"));
    }
    let trash_dir = Path::new(&home).join(".Trash");
    if !trash_dir.exists() {
        std::fs::create_dir_all(&trash_dir)?;
    }
    
    let filename = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid source file path")
    })?;
    let filename_str = filename.to_string_lossy();
    let dest = get_unique_trash_path(&trash_dir, &filename_str);
    
    // Try std::fs::rename first
    if let Err(_) = std::fs::rename(src, &dest) {
        // Fallback for cross-device moves
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}

fn resolve_duplicates_internal<F>(
    items: Vec<ResolutionItem>,
    mode: String,
    progress_fn: Option<F>,
) -> Result<String, String>
where
    F: Fn(usize) + Send + Sync,
{
    if items.is_empty() {
        return Ok("No files to resolve.".to_string());
    }

    write_to_log_file(&format!("Starting resolution of {} files (mode: '{}').", items.len(), mode));

    // Circular symlink check: verify the target's original counterpart is not scheduled for deletion.
    let targets: std::collections::HashSet<&str> = items.iter().map(|i| i.target.as_str()).collect();
    if mode == "symlink" {
        for item in &items {
            if targets.contains(item.original.as_str()) {
                write_to_log_file(&format!("Safety Hazard aborted: circular reference detected. Target and original match: {}", item.original));
                return Err(format!(
                    "Safety Hazard: Preserved file '{}' is also marked for deletion. Aborting linking operation.",
                    item.original
                ));
            }
        }
    }

    let mut resolved_count = 0;
    let mut skipped_items: Vec<String> = Vec::new();

    for item in &items {
        let target_path = Path::new(&item.target);
        if !target_path.exists() {
            resolved_count += 1;
            write_to_log_file(&format!("Skipping file '{}' as it already does not exist.", item.target));
            if let Some(ref pf) = progress_fn {
                pf(resolved_count + skipped_items.len());
            }
            continue; // Skip if already resolved
        }

        write_to_log_file(&format!("Resolving file: '{}' pointing to original: '{}'", item.target, item.original));

        // 1. Move the target file to the macOS Trash silently
        if let Err(e) = move_to_trash_silent(target_path) {
            // Permission denied or other fs error: log as warning and skip this file
            // instead of aborting the entire batch operation.
            let reason = format!("{} (os error {})", e.kind().to_string(), e.raw_os_error().unwrap_or(0));
            write_to_log_file(&format!(
                "Warning: Skipped '{}' — could not move to Trash: {}. Hint: check file permissions (chmod/chown) or if the file is locked.",
                item.target, reason
            ));
            skipped_items.push(item.target.clone());
            if let Some(ref pf) = progress_fn {
                pf(resolved_count + skipped_items.len());
            }
            continue;
        }

        // 2. If Symlink Preservation is toggled, create a symbolic link pointing to the original
        if mode == "symlink" {
            let original_path = Path::new(&item.original);

            #[cfg(unix)]
            {
                if let Err(e) = std::os::unix::fs::symlink(original_path, target_path) {
                    write_to_log_file(&format!("Error: Failed to symlink '{}' to '{}': {:?}", item.target, item.original, e));
                    return Err(format!(
                        "Symlink Error: Failed to create symlink from '{}' to '{}': {}",
                        item.target, item.original, e
                    ));
                }
            }

            #[cfg(windows)]
            {
                if let Err(e) = std::os::windows::fs::symlink_file(original_path, target_path) {
                    write_to_log_file(&format!("Error: Failed to symlink on Windows: {:?}", e));
                    return Err(format!(
                        "Symlink Error: Failed to create symlink: {}",
                        e
                    ));
                }
            }
        }

        resolved_count += 1;
        if let Some(ref pf) = progress_fn {
            pf(resolved_count + skipped_items.len());
        }
    }

    if skipped_items.is_empty() {
        write_to_log_file(&format!("Resolution complete. Successfully resolved {} of {} items.", resolved_count, items.len()));
        Ok(format!("Successfully resolved {} duplicates.", resolved_count))
    } else {
        write_to_log_file(&format!(
            "Resolution complete with warnings. Resolved {} of {} items. Skipped {} items due to permission errors (see log).",
            resolved_count, items.len(), skipped_items.len()
        ));
        // Return Ok with a summary message — the frontend will display it as a warning, not a hard error.
        Ok(format!(
            "Resolved {} of {} files. {} file(s) were skipped due to permission errors — open Help > View App Log for details.",
            resolved_count, items.len(), skipped_items.len()
        ))
    }
}

#[tauri::command]
async fn resolve_duplicates(
    app: tauri::AppHandle,
    items: Vec<ResolutionItem>,
    mode: String,
) -> Result<String, String> {
    let progress_fn = move |count| {
        let _ = app.emit("resolve-progress", count);
    };
    resolve_duplicates_internal(items, mode, Some(progress_fn))
}

#[tauri::command]
fn show_in_finder(path: String) -> Result<(), String> {
    write_to_log_file(&format!("Requesting Show in Finder for path: '{}'", path));
    let path_buf = std::path::PathBuf::from(path.clone());
    if !path_buf.exists() {
        write_to_log_file(&format!("Show in Finder failed: Path '{}' does not exist.", path));
        return Err("File or folder does not exist".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path_buf)
            .spawn()
            .map_err(|e| {
                write_to_log_file(&format!("Show in Finder Error for path '{}': {}", path, e));
                e.to_string()
            })?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path_buf)
            .spawn()
            .map_err(|e| {
                write_to_log_file(&format!("Show in Finder Error for path '{}': {}", path, e));
                e.to_string()
            })?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        write_to_log_file("Show in Finder failed: Platform not supported.");
        Err("Platform not supported".to_string())
    }
}

#[tauri::command]
fn get_info(path: String) -> Result<(), String> {
    write_to_log_file(&format!("Requesting Get Info for path: '{}'", path));
    let path_buf = std::path::PathBuf::from(path.clone());
    if !path_buf.exists() {
        write_to_log_file(&format!("Get Info failed: Path '{}' does not exist.", path));
        return Err("File or folder does not exist".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let path_str = path_buf.to_string_lossy();
        let script = format!(
            "tell application \"Finder\"\nopen information window of (POSIX file \"{}\" as alias)\nactivate\nend tell",
            path_str
        );
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
            .map_err(|e| {
                write_to_log_file(&format!("Get Info Error for path '{}': {}", path, e));
                e.to_string()
            })?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        write_to_log_file("Get Info failed: Windows not supported.");
        Err("Get Info is not supported on Windows yet".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        write_to_log_file("Get Info failed: Platform not supported.");
        Err("Platform not supported".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ScanState::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle();
            
            // App Menu (default on macOS)
            let app_menu = Submenu::new(handle, "auDO File Z", true)?;
            let about_item = PredefinedMenuItem::about(handle, None, None)?;
            let quit_item = PredefinedMenuItem::quit(handle, None)?;
            app_menu.append(&about_item)?;
            app_menu.append(&PredefinedMenuItem::separator(handle)?)?;
            app_menu.append(&quit_item)?;
            
            // View Menu
            let view_menu = Submenu::new(handle, "View", true)?;
            
            // Skins Submenu
            let skins_menu = Submenu::new(handle, "Skins", true)?;
            let doors_xp = MenuItem::with_id(handle, "doors_xp", "DoorsXP-Z (Retro)", true, None::<&str>)?;
            let vinyl_box = MenuItem::with_id(handle, "vinyl_box", "VinylBox-Z (Dark)", true, None::<&str>)?;
            skins_menu.append(&doors_xp)?;
            skins_menu.append(&vinyl_box)?;
            
            view_menu.append(&skins_menu)?;
            
            // Set Menu
            let menu = Menu::with_items(handle, &[&app_menu, &view_menu])?;
            handle.set_menu(menu)?;
            
            Ok(())
        })
        .on_menu_event(|app, event| {
            let id = event.id();
            if id.as_ref() == "doors_xp" {
                let _ = app.emit("change-theme", "doors");
            } else if id.as_ref() == "vinyl_box" {
                let _ = app.emit("change-theme", "vinyl");
            }
        })
        .invoke_handler(tauri::generate_handler![
            check_fda,
            open_folder_picker,
            start_scan,
            resolve_duplicates,
            show_in_finder,
            get_info,
            cancel_scan,
            read_log_file,
            clear_log_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/* UNIT TESTS */
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_parse_extensions() {
        assert_eq!(parse_extensions(".mp3, .WAV, flac"), vec!["mp3", "wav", "flac"]);
        assert_eq!(parse_extensions(""), Vec::<String>::new());
        assert_eq!(parse_extensions("  , , .mp3  "), vec!["mp3"]);
        assert_eq!(parse_extensions("..wav, ...mp3"), vec!["wav", "mp3"]); // Tests trim_start_matches
    }

    #[test]
    fn test_hashing_and_crawler() {
        // Create temporary test structure in the target folder (clean sandbox)
        let test_dir = Path::new("target/test_run_m4");
        let _ = fs::remove_dir_all(test_dir);
        fs::create_dir_all(test_dir).unwrap();

        let sub_a = test_dir.join("sub_a");
        let sub_b = test_dir.join("sub_b");
        fs::create_dir_all(&sub_a).unwrap();
        fs::create_dir_all(&sub_b).unwrap();

        // 1. Create duplicate files
        let file_1 = sub_a.join("file1.txt");
        let file_2 = sub_b.join("file2.txt");
        let mut f1 = File::create(&file_1).unwrap();
        let mut f2 = File::create(&file_2).unwrap();
        f1.write_all(b"identical text content").unwrap();
        f2.write_all(b"identical text content").unwrap();

        // 2. Create a non-duplicate file of the same size (22 bytes)
        let file_3 = sub_a.join("file3.txt");
        let mut f3 = File::create(&file_3).unwrap();
        f3.write_all(b"different text content").unwrap();

        // 3. Create a hidden file (should be ignored by crawler)
        let hidden_file = sub_a.join(".hidden.txt");
        let mut fh = File::create(&hidden_file).unwrap();
        fh.write_all(b"identical text content").unwrap();

        // Validate hash matching
        let hash1 = hash_file(&file_1).unwrap();
        let hash2 = hash_file(&file_2).unwrap();
        let hash3 = hash_file(&file_3).unwrap();
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);

        // Test crawler
        let mut crawled = Vec::new();
        crawl_directory(test_dir, &vec!["txt".to_string()], &mut crawled);
        
        // Assert: 3 files found (hidden file ignored)
        assert_eq!(crawled.len(), 3);

        // Test duplicate detection scan command
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let duplicates = run_scan_internal(
            &test_dir.to_string_lossy(),
            "txt",
            &cancel_flag,
            None::<fn(usize)>,
        ).unwrap();
        assert_eq!(duplicates.len(), 1); 
        assert_eq!(duplicates[0].files.len(), 2);
        assert!(duplicates[0].files.iter().any(|f| f.path == file_1.to_string_lossy().into_owned()));
        assert!(duplicates[0].files.iter().any(|f| f.path == file_2.to_string_lossy().into_owned()));
        assert_eq!(duplicates[0].files[0].created > 0, true);

        // Test resolution: delete mode
        // Let's resolve: delete file_2, pointing back to file_1
        let res_items = vec![
            ResolutionItem {
                target: file_2.to_string_lossy().into_owned(),
                original: file_1.to_string_lossy().into_owned(),
            }
        ];
        
        let res = resolve_duplicates_internal(res_items, "delete".to_string(), None::<fn(usize)>);
        assert!(res.is_ok());
        // file_2 must be gone, file_1 must still exist
        assert!(!file_2.exists());
        assert!(file_1.exists());

        // Test circular reference check
        let bad_items = vec![
            ResolutionItem {
                target: file_1.to_string_lossy().into_owned(),
                original: file_1.to_string_lossy().into_owned(),
            }
        ];
        let bad_res = resolve_duplicates_internal(bad_items, "symlink".to_string(), None::<fn(usize)>);
        assert!(bad_res.is_err()); // Circular link check failed

        // Clean up sandbox
        let _ = fs::remove_dir_all(test_dir);
    }
}
