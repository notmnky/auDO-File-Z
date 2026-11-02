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

#[derive(Deserialize, Debug, Default)]
pub struct ResolutionItem {
    pub target: String,
    pub original: String,
    /// Scan-time values. When present, the files must still match them.
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub target_modified: Option<u64>,
    #[serde(default)]
    pub original_modified: Option<u64>,
}

fn modified_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Checks, right before anything is moved, that the duplicate and the copy
/// being kept still exist and still look the way they did at scan time.
/// Returns a short reason when the item must be skipped.
fn validate_resolution_item(item: &ResolutionItem) -> Result<(), String> {
    let target = Path::new(&item.target);
    let original = Path::new(&item.original);

    let t_meta = std::fs::symlink_metadata(target).map_err(|_| "duplicate no longer exists".to_string())?;
    if !t_meta.is_file() {
        return Err("duplicate is no longer a regular file".to_string());
    }
    let o_meta = std::fs::symlink_metadata(original).map_err(|_| "kept copy no longer exists".to_string())?;
    if !o_meta.is_file() {
        return Err("kept copy is no longer a regular file".to_string());
    }

    let t_canon = std::fs::canonicalize(target).map_err(|e| format!("cannot resolve duplicate path: {}", e))?;
    let o_canon = std::fs::canonicalize(original).map_err(|e| format!("cannot resolve kept copy path: {}", e))?;
    if t_canon == o_canon {
        return Err("duplicate and kept copy are the same file".to_string());
    }

    if t_meta.len() != o_meta.len() {
        return Err("duplicate and kept copy no longer have the same size".to_string());
    }
    if let Some(size) = item.size {
        if t_meta.len() != size {
            return Err("size changed since the scan".to_string());
        }
    }
    if let Some(m) = item.target_modified {
        if modified_ms(&t_meta) != Some(m) {
            return Err("duplicate was modified since the scan".to_string());
        }
    }
    if let Some(m) = item.original_modified {
        if modified_ms(&o_meta) != Some(m) {
            return Err("kept copy was modified since the scan".to_string());
        }
    }
    Ok(())
}

fn write_to_log_file(message: &str) {
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
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
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).map_err(|e| e.to_string())?;
    let log_path = Path::new(&home).join(".config").join("auDO-File-Z").join("app.log");
    if !log_path.exists() {
        return Ok("No log records found yet.".to_string());
    }
    std::fs::read_to_string(&log_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_log_file() -> Result<(), String> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).map_err(|e| e.to_string())?;
    let log_path = Path::new(&home).join(".config").join("auDO-File-Z").join("app.log");
    if log_path.exists() {
        std::fs::remove_file(&log_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn check_fda() -> bool {
    write_to_log_file("FDA Check executed.");
    
    #[cfg(not(target_os = "macos"))]
    {
        write_to_log_file("FDA Check bypassed: platform is not macOS.");
        return true;
    }
    
    #[cfg(target_os = "macos")]
    {
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

/// `(device, inode)` for a file that has more than one hard link, else `None`.
/// Two paths with the same identity are one file on disk: trashing one frees
/// no space, so only the first path found is kept. Unix only; on Windows
/// hard links are rare and std exposes no stable file index.
#[cfg(unix)]
fn hard_link_identity(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}
#[cfg(not(unix))]
fn hard_link_identity(_meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

type CrawledFile = (PathBuf, u64, u64, u64, bool);

/// Crawls `dir` and returns every regular file matching `extensions`.
fn crawl(dir: &Path, extensions: &[String]) -> Vec<CrawledFile> {
    let mut files = Vec::new();
    let mut seen_links = std::collections::HashSet::new();
    crawl_directory(dir, extensions, &mut files, &mut seen_links);
    files
}

// Recursive directory crawler (skips hidden files/directories, symlinks,
// and extra names of an already-seen hard-linked file)
fn crawl_directory(
    dir: &Path,
    extensions: &[String],
    files: &mut Vec<CrawledFile>,
    seen_links: &mut std::collections::HashSet<(u64, u64)>,
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
            crawl_directory(&path, extensions, files, seen_links);
        } else if file_type.is_file() {
            let (size, modified, created, read_only, link_id) = match entry.metadata() {
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
                    (sz, mod_time, cre_time, ro, hard_link_identity(&meta))
                }
                Err(_) => continue,
            };

            // Filter by extension if constraints are provided
            let matches = extensions.is_empty()
                || path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| extensions.iter().any(|e| *e == ext.to_lowercase()))
                    .unwrap_or(false);
            if !matches {
                continue;
            }

            if let Some(id) = link_id {
                if !seen_links.insert(id) {
                    write_to_log_file(&format!(
                        "Skipping '{}': hard link to a file already scanned (same data on disk, removing it would free no space).",
                        path.display()
                    ));
                    continue;
                }
            }
            files.push((path, size, modified, created, read_only));
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

    let all_files = crawl(target_path, &parsed_exts);

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

/// Moves one file to the system Trash / Recycle Bin through the `trash` crate.
///
/// macOS uses `NSFileManager.trashItemAtURL`: no Automation permission prompt and
/// no Finder sound per file, and files on external volumes go to that volume's
/// own Trash instead of being copied into the home Trash. Trade-off: Finder's
/// "Put Back" may not be offered for these files (they can still be dragged out).
fn move_to_trash_silent(src: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let result = {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(src)
    };
    #[cfg(not(target_os = "macos"))]
    let result = trash::delete(src);

    result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

/// Creates a file symlink at `link` pointing to `original`.
fn create_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        // Fails without Developer Mode or admin rights; handled by the caller.
        std::os::windows::fs::symlink_file(original, link)
    }
}

/// Hidden name next to `target` used while a link is staged. Hidden, so the
/// crawler ignores it if one is ever left behind.
fn staging_link_path(target: &Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    target.with_file_name(format!(".{}.afz-link-{}", name, std::process::id()))
}

/// `trash_fn` moves one file to the Trash and `link_fn` creates a symlink.
/// The app passes `move_to_trash_silent` and `create_symlink`; tests pass
/// stand-ins, so `cargo test` never touches the developer's real Trash.
fn resolve_duplicates_internal<F, T, L>(
    items: Vec<ResolutionItem>,
    mode: String,
    trash_fn: T,
    link_fn: L,
    progress_fn: Option<F>,
) -> Result<String, String>
where
    F: Fn(usize) + Send + Sync,
    T: Fn(&Path) -> std::io::Result<()>,
    L: Fn(&Path, &Path) -> std::io::Result<()>,
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
    // Duplicates that reached the Trash but whose link could not be put in place.
    let mut unlinked_items: Vec<String> = Vec::new();

    for item in &items {
        let target_path = Path::new(&item.target);
        if let Err(reason) = validate_resolution_item(item) {
            write_to_log_file(&format!(
                "Warning: Skipped '{}' (kept copy '{}'): {}.",
                item.target, item.original, reason
            ));
            skipped_items.push(item.target.clone());
            if let Some(ref pf) = progress_fn {
                pf(resolved_count + skipped_items.len() + unlinked_items.len());
            }
            continue;
        }

        write_to_log_file(&format!("Resolving file: '{}' pointing to original: '{}'", item.target, item.original));

        // Symlink mode, step 1: stage the link under a hidden name BEFORE trashing,
        // so a link that cannot be created never costs the user a file.
        let staged_link = if mode == "symlink" {
            let staged = staging_link_path(target_path);
            let _ = std::fs::remove_file(&staged); // stale leftover from a crashed run
            if let Err(e) = link_fn(Path::new(&item.original), &staged) {
                write_to_log_file(&format!(
                    "Warning: Skipped '{}' — could not create a symlink to '{}': {}. Nothing was moved. On Windows, symlinks need Developer Mode or admin rights.",
                    item.target, item.original, e
                ));
                skipped_items.push(item.target.clone());
                if let Some(ref pf) = progress_fn {
                    pf(resolved_count + skipped_items.len() + unlinked_items.len());
                }
                continue;
            }
            Some(staged)
        } else {
            None
        };

        // Move the duplicate to the Trash
        if let Err(e) = trash_fn(target_path) {
            if let Some(ref staged) = staged_link {
                let _ = std::fs::remove_file(staged);
            }
            // Permission denied or other fs error: log as warning and skip this file
            // instead of aborting the entire batch operation.
            let reason = format!("{} (os error {})", e.kind().to_string(), e.raw_os_error().unwrap_or(0));
            write_to_log_file(&format!(
                "Warning: Skipped '{}' — could not move to Trash: {}. Hint: check file permissions (chmod/chown) or if the file is locked.",
                item.target, reason
            ));
            skipped_items.push(item.target.clone());
            if let Some(ref pf) = progress_fn {
                pf(resolved_count + skipped_items.len() + unlinked_items.len());
            }
            continue;
        }

        // Symlink mode, step 3: move the staged link into the duplicate's place.
        if let Some(ref staged) = staged_link {
            if let Err(e) = std::fs::rename(staged, target_path) {
                let _ = std::fs::remove_file(staged);
                write_to_log_file(&format!(
                    "Error: '{}' was moved to the Trash, but the symlink to '{}' could not be put in place: {}. Restore it from the Trash if needed.",
                    item.target, item.original, e
                ));
                unlinked_items.push(item.target.clone());
                if let Some(ref pf) = progress_fn {
                    pf(resolved_count + skipped_items.len() + unlinked_items.len());
                }
                continue;
            }
        }

        resolved_count += 1;
        if let Some(ref pf) = progress_fn {
            pf(resolved_count + skipped_items.len() + unlinked_items.len());
        }
    }

    if skipped_items.is_empty() && unlinked_items.is_empty() {
        write_to_log_file(&format!("Resolution complete. Successfully resolved {} of {} items.", resolved_count, items.len()));
        return Ok(format!("Successfully resolved {} duplicates.", resolved_count));
    }

    write_to_log_file(&format!(
        "Resolution complete with warnings. Resolved {} of {} items. Skipped {}. Trashed without link {} (see warnings above).",
        resolved_count, items.len(), skipped_items.len(), unlinked_items.len()
    ));
    // Return Ok with a summary message — the frontend will display it as a warning, not a hard error.
    let mut msg = format!("Resolved {} of {} files.", resolved_count, items.len());
    if !skipped_items.is_empty() {
        msg.push_str(&format!(
            " {} file(s) were skipped and left untouched (changed or missing since the scan, or could not be linked or moved to the Trash).",
            skipped_items.len()
        ));
    }
    if !unlinked_items.is_empty() {
        msg.push_str(&format!(
            " {} file(s) were moved to the Trash but their symlink could not be created; restore them from the Trash if needed.",
            unlinked_items.len()
        ));
    }
    msg.push_str(" Open Help > View App Log for details.");
    Ok(msg)
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
    resolve_duplicates_internal(items, mode, move_to_trash_silent, create_symlink, Some(progress_fn))
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

/// Fixed AppleScript for Finder's Get Info window. The file path is never
/// part of the script text: it arrives as `item 1 of argv`, so a file name
/// containing quotes or AppleScript cannot change what the script does.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const GET_INFO_SCRIPT: [&str; 6] = [
    "on run argv",
    "tell application \"Finder\"",
    "open information window of (POSIX file (item 1 of argv) as alias)",
    "activate",
    "end tell",
    "end run",
];

/// Arguments for `osascript`: one `-e` per script line, then the path as argv.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn get_info_osascript_args(path: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for line in GET_INFO_SCRIPT {
        args.push("-e".into());
        args.push(line.into());
    }
    args.push(path.as_os_str().to_os_string());
    args
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
        std::process::Command::new("osascript")
            .args(get_info_osascript_args(&path_buf))
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

#[derive(Serialize)]
pub struct AppMetadata {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub platform: String,
    pub architecture: String,
    pub build_platform: String,
}

const BUILD_ARCH: &str = if cfg!(target_arch = "aarch64") {
    "arm64"
} else if cfg!(target_arch = "x86_64") {
    "x64"
} else {
    "x64"
};

const BUILD_OS: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "windows") {
    "windows"
} else {
    "linux"
};

fn get_app_version() -> String {
    let json_str = include_str!("../../version.json");
    let v: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();
    v["version"].as_str().unwrap_or("0.1.0").to_string()
}

#[tauri::command]
fn get_app_metadata() -> AppMetadata {
    let os = BUILD_OS.to_string();
    let arch = BUILD_ARCH.to_string();
    let build_platform = format!("{}-{}", os, arch);
    let version = get_app_version();
    
    AppMetadata {
        version,
        os: os.clone(),
        arch: arch.clone(),
        platform: os,
        architecture: arch,
        build_platform,
    }
}

/// Only assets attached to this repository's GitHub releases may be downloaded.
const UPDATE_URL_PREFIX: &str = "https://github.com/notmnky/auDO-File-Z/releases/download/";
const UPDATE_EXTENSIONS: [&str; 5] = [".dmg", ".msi", ".exe", ".zip", ".tar.gz"];

/// Checks an update request from the frontend. Returns the file name to save as.
///
/// The URL must be a release-asset URL of this repository (https only, no query,
/// fragment, `..` or whitespace). The file name must be a plain base name with an
/// installer extension, and must equal the URL's last segment, so the frontend
/// cannot pick a different save location or name than the asset itself.
fn validate_update_request(url: &str, filename: &str) -> Result<String, String> {
    let rest = url
        .get(..UPDATE_URL_PREFIX.len())
        .filter(|p| p.eq_ignore_ascii_case(UPDATE_URL_PREFIX))
        .map(|_| &url[UPDATE_URL_PREFIX.len()..])
        .ok_or("update URL is not a release asset of notmnky/auDO-File-Z")?;
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '?' | '#' | '\\' | '%')) {
        return Err("update URL contains characters that are not allowed".into());
    }
    // Expect exactly "<tag>/<asset>".
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.len() != 2 || segments.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err("update URL does not have the form .../releases/download/<tag>/<asset>".into());
    }

    if filename.is_empty()
        || filename.starts_with('.')
        || filename.contains(['/', '\\', ':', '\0'])
        || filename.contains("..")
        || filename.chars().any(|c| c.is_control())
    {
        return Err("update file name is not a plain file name".into());
    }
    let lower = filename.to_ascii_lowercase();
    if !UPDATE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err("update file is not an installer (.dmg, .msi, .exe, .zip, .tar.gz)".into());
    }
    if filename != segments[1] {
        return Err("update file name does not match the release asset".into());
    }
    Ok(filename.to_string())
}

#[tauri::command]
async fn download_and_open_update(url: String, filename: String) -> Result<(), String> {
    let filename = validate_update_request(&url, &filename).map_err(|e| {
        write_to_log_file(&format!("Update download refused for '{}': {}", url, e));
        format!("Update refused: {}", e)
    })?;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|e| e.to_string())?;
    let downloads_dir = Path::new(&home).join("Downloads");
    let dest_path = downloads_dir.join(&filename);

    write_to_log_file(&format!("Starting download of update asset from '{}' to '{}'", url, dest_path.display()));

    // Run curl to download the file. https only, including redirects
    // (GitHub redirects asset downloads to its download CDN); --fail so an
    // error page is never saved and opened as if it were the installer.
    let status = std::process::Command::new("curl")
        .args(["--fail", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5"])
        .arg("-o")
        .arg(&dest_path)
        .arg("--")
        .arg(&url)
        .status()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !status.success() {
        write_to_log_file("Update download failed via curl.");
        return Err("curl failed to download the update asset.".to_string());
    }

    write_to_log_file(&format!("Update download successful. Opening asset: '{}'", dest_path.display()));

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dest_path)
            .spawn()
            .map_err(|e| format!("Failed to open downloaded update: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dest_path)
            .spawn()
            .map_err(|e| format!("Failed to open downloaded update: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
async fn window_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
async fn window_toggle_maximize(window: tauri::Window) -> Result<(), String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
async fn window_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ScanState::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle();
            
            // App Menu (default on macOS)
            let app_menu = Submenu::new(handle, "auDO FileZ", true)?;
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
            clear_log_file,
            get_app_metadata,
            download_and_open_update,
            window_minimize,
            window_toggle_maximize,
            window_close
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

    /// Fresh, unique folder under the OS temp dir. Never inside the repo or $HOME.
    fn temp_sandbox(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("afz-test-{}-{}-{}", name, std::process::id(), nanos));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Stand-in for the system Trash: moves the file into `trash_dir`.
    fn fake_trash(trash_dir: PathBuf) -> impl Fn(&Path) -> std::io::Result<()> {
        move |src: &Path| {
            fs::create_dir_all(&trash_dir)?;
            fs::rename(src, trash_dir.join(src.file_name().unwrap()))
        }
    }

    fn write_file(path: &Path, content: &[u8]) {
        File::create(path).unwrap().write_all(content).unwrap();
    }

    #[test]
    fn test_parse_extensions() {
        assert_eq!(parse_extensions(".mp3, .WAV, flac"), vec!["mp3", "wav", "flac"]);
        assert_eq!(parse_extensions(""), Vec::<String>::new());
        assert_eq!(parse_extensions("  , , .mp3  "), vec!["mp3"]);
        assert_eq!(parse_extensions("..wav, ...mp3"), vec!["wav", "mp3"]); // Tests trim_start_matches
    }

    #[test]
    fn test_hashing_and_crawler() {
        let sandbox = temp_sandbox("crawler");
        let test_dir = sandbox.join("scan");
        let trash_dir = sandbox.join("trash");
        let test_dir = test_dir.as_path();
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
        let crawled = crawl(test_dir, &["txt".to_string()]);
        
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
                ..Default::default()
            }
        ];
        
        let res = resolve_duplicates_internal(res_items, "delete".to_string(), fake_trash(trash_dir.clone()), create_symlink, None::<fn(usize)>);
        assert!(res.is_ok());
        // file_2 must be in the fake Trash, file_1 must still exist
        assert!(!file_2.exists());
        assert!(trash_dir.join("file2.txt").exists());
        assert!(file_1.exists());

        // Test circular reference check
        let bad_items = vec![
            ResolutionItem {
                target: file_1.to_string_lossy().into_owned(),
                original: file_1.to_string_lossy().into_owned(),
                ..Default::default()
            }
        ];
        let bad_res = resolve_duplicates_internal(bad_items, "symlink".to_string(), fake_trash(trash_dir.clone()), create_symlink, None::<fn(usize)>);
        assert!(bad_res.is_err()); // Circular link check failed

        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn trash_failure_skips_item_and_continues_batch() {
        let sandbox = temp_sandbox("trashfail");
        let orig = sandbox.join("orig.txt");
        let locked = sandbox.join("locked.txt");
        let ok = sandbox.join("ok.txt");
        for p in [&orig, &locked, &ok] {
            write_file(p, b"same");
        }
        let trash_dir = sandbox.join("trash");
        let inner = fake_trash(trash_dir.clone());
        let failing_trash = |p: &Path| {
            if p.ends_with("locked.txt") {
                Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"))
            } else {
                inner(p)
            }
        };
        let items = vec![
            ResolutionItem { target: locked.to_string_lossy().into(), original: orig.to_string_lossy().into(), ..Default::default() },
            ResolutionItem { target: ok.to_string_lossy().into(), original: orig.to_string_lossy().into(), ..Default::default() },
        ];
        let msg = resolve_duplicates_internal(items, "delete".to_string(), failing_trash, create_symlink, None::<fn(usize)>).unwrap();
        assert!(msg.contains("1 file(s) were skipped"), "{msg}");
        assert!(locked.exists());
        assert!(!ok.exists());
        assert!(trash_dir.join("ok.txt").exists());
        assert!(orig.exists());
        let _ = fs::remove_dir_all(&sandbox);
    }

    /// Item carrying the files' current size and modified times, as the frontend sends after a scan.
    fn scanned_item(target: &Path, original: &Path) -> ResolutionItem {
        let t = fs::metadata(target).unwrap();
        let o = fs::metadata(original).unwrap();
        ResolutionItem {
            target: target.to_string_lossy().into(),
            original: original.to_string_lossy().into(),
            size: Some(t.len()),
            target_modified: modified_ms(&t),
            original_modified: modified_ms(&o),
        }
    }

    #[test]
    fn validation_accepts_unchanged_files() {
        let sandbox = temp_sandbox("valid_ok");
        let (a, b) = (sandbox.join("a.txt"), sandbox.join("b.txt"));
        write_file(&a, b"same");
        write_file(&b, b"same");
        assert_eq!(validate_resolution_item(&scanned_item(&b, &a)), Ok(()));
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn validation_rejects_missing_kept_copy_and_nothing_is_trashed() {
        let sandbox = temp_sandbox("valid_missing");
        let (a, b) = (sandbox.join("a.txt"), sandbox.join("b.txt"));
        write_file(&a, b"same");
        write_file(&b, b"same");
        let item = scanned_item(&b, &a);
        fs::remove_file(&a).unwrap(); // kept copy disappears after the scan
        assert_eq!(validate_resolution_item(&item), Err("kept copy no longer exists".to_string()));

        let trash_dir = sandbox.join("trash");
        let msg = resolve_duplicates_internal(vec![item], "delete".to_string(), fake_trash(trash_dir.clone()), create_symlink, None::<fn(usize)>).unwrap();
        assert!(msg.contains("1 file(s) were skipped"), "{msg}");
        assert!(b.exists(), "the last remaining copy must not be trashed");
        assert!(!trash_dir.exists());
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn validation_rejects_missing_duplicate() {
        let sandbox = temp_sandbox("valid_gone");
        let (a, b) = (sandbox.join("a.txt"), sandbox.join("b.txt"));
        write_file(&a, b"same");
        write_file(&b, b"same");
        let item = scanned_item(&b, &a);
        fs::remove_file(&b).unwrap();
        assert_eq!(validate_resolution_item(&item), Err("duplicate no longer exists".to_string()));
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn validation_rejects_files_changed_since_scan() {
        let sandbox = temp_sandbox("valid_changed");
        let (a, b) = (sandbox.join("a.txt"), sandbox.join("b.txt"));
        write_file(&a, b"same");
        write_file(&b, b"same");

        // Same size, different modified time: rewritten in place after the scan.
        let mut item = scanned_item(&b, &a);
        item.target_modified = item.target_modified.map(|m| m - 5_000);
        assert_eq!(validate_resolution_item(&item), Err("duplicate was modified since the scan".to_string()));

        let mut item = scanned_item(&b, &a);
        item.original_modified = item.original_modified.map(|m| m - 5_000);
        assert_eq!(validate_resolution_item(&item), Err("kept copy was modified since the scan".to_string()));

        // Kept copy grows: sizes no longer match.
        let item = scanned_item(&b, &a);
        write_file(&a, b"same plus more");
        assert_eq!(
            validate_resolution_item(&item),
            Err("duplicate and kept copy no longer have the same size".to_string())
        );
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn validation_rejects_same_file_under_two_paths() {
        let sandbox = temp_sandbox("valid_same");
        let a = sandbox.join("a.txt");
        write_file(&a, b"same");
        fs::create_dir_all(sandbox.join("sub")).unwrap();
        let via_dotdot = sandbox.join("sub").join("..").join("a.txt");
        let item = ResolutionItem {
            target: via_dotdot.to_string_lossy().into(),
            original: a.to_string_lossy().into(),
            ..Default::default()
        };
        assert_eq!(validate_resolution_item(&item), Err("duplicate and kept copy are the same file".to_string()));
        let _ = fs::remove_dir_all(&sandbox);
    }

    /// Lists hidden staging links left in `dir` (there should never be any).
    fn leftover_staging_links(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".afz-link-"))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn symlink_mode_replaces_duplicate_with_link() {
        let sandbox = temp_sandbox("link_ok");
        let (orig, dup) = (sandbox.join("orig.txt"), sandbox.join("dup.txt"));
        write_file(&orig, b"same");
        write_file(&dup, b"same");
        let trash_dir = sandbox.join("trash");

        let msg = resolve_duplicates_internal(
            vec![scanned_item(&dup, &orig)], "symlink".to_string(),
            fake_trash(trash_dir.clone()), create_symlink, None::<fn(usize)>,
        ).unwrap();
        assert!(msg.starts_with("Successfully"), "{msg}");
        assert!(fs::symlink_metadata(&dup).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&dup).unwrap(), orig);
        assert!(trash_dir.join("dup.txt").exists());
        assert!(orig.is_file());
        assert!(leftover_staging_links(&sandbox).is_empty());
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn symlink_failure_moves_nothing_and_continues_batch() {
        let sandbox = temp_sandbox("link_fail");
        let orig = sandbox.join("orig.txt");
        let (bad, good) = (sandbox.join("bad.txt"), sandbox.join("good.txt"));
        for p in [&orig, &bad, &good] {
            write_file(p, b"same");
        }
        let trash_dir = sandbox.join("trash");
        // Fails for one item, works for the other (stands in for e.g. Windows without Developer Mode).
        let flaky_link = |o: &Path, link: &Path| {
            if link.to_string_lossy().contains("bad.txt") {
                Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no symlink privilege"))
            } else {
                create_symlink(o, link)
            }
        };
        let items = vec![scanned_item(&bad, &orig), scanned_item(&good, &orig)];
        let msg = resolve_duplicates_internal(items, "symlink".to_string(), fake_trash(trash_dir.clone()), flaky_link, None::<fn(usize)>).unwrap();

        assert!(msg.contains("1 file(s) were skipped"), "{msg}");
        assert!(bad.is_file(), "a file whose link failed must stay where it was");
        assert!(!trash_dir.join("bad.txt").exists());
        assert!(fs::symlink_metadata(&good).unwrap().file_type().is_symlink());
        assert!(leftover_staging_links(&sandbox).is_empty());
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn symlink_mode_trash_failure_removes_staged_link() {
        let sandbox = temp_sandbox("link_trashfail");
        let (orig, dup) = (sandbox.join("orig.txt"), sandbox.join("dup.txt"));
        write_file(&orig, b"same");
        write_file(&dup, b"same");
        let refuse = |_: &Path| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"));
        let msg = resolve_duplicates_internal(
            vec![scanned_item(&dup, &orig)], "symlink".to_string(), refuse, create_symlink, None::<fn(usize)>,
        ).unwrap();
        assert!(msg.contains("1 file(s) were skipped"), "{msg}");
        assert!(dup.is_file());
        assert!(leftover_staging_links(&sandbox).is_empty());
        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn get_info_passes_path_as_argument_not_script() {
        let evil = Path::new("/tmp/a\" & do shell script \"touch /tmp/pwned\" & \".mp3");
        let args = get_info_osascript_args(evil);
        // The path is exactly the last argument, untouched...
        assert_eq!(args.last().unwrap(), evil.as_os_str());
        // ...and appears nowhere in the script lines.
        for a in &args[..args.len() - 1] {
            assert!(!a.to_string_lossy().contains("pwned"));
        }
        // Every script line is preceded by -e.
        assert_eq!(args.len(), GET_INFO_SCRIPT.len() * 2 + 1);
        assert!(args.iter().step_by(2).take(GET_INFO_SCRIPT.len()).all(|a| a == "-e"));
    }

    #[test]
    fn update_request_accepts_this_repos_release_assets() {
        let base = "https://github.com/notmnky/auDO-File-Z/releases/download/v0.1.9005/";
        for name in ["auDO.FileZ_0.1.9005_aarch64.dmg", "auDO.FileZ_0.1.9005_x64_en-US.msi", "setup-windows-x64.exe", "audofilez.app.tar.gz"] {
            assert_eq!(validate_update_request(&format!("{base}{name}"), name), Ok(name.to_string()), "{name}");
        }
    }

    #[test]
    fn update_request_rejects_everything_else() {
        let ok_url = "https://github.com/notmnky/auDO-File-Z/releases/download/v1/app.dmg";
        let bad_urls = [
            "http://github.com/notmnky/auDO-File-Z/releases/download/v1/app.dmg",   // not https
            "https://evil.example/notmnky/auDO-File-Z/releases/download/v1/app.dmg", // other host
            "https://github.com/someone/else/releases/download/v1/app.dmg",         // other repo
            "https://github.com/notmnky/auDO-File-Z/releases/download/v1/../x/app.dmg",
            "https://github.com/notmnky/auDO-File-Z/releases/download/v1/app.dmg?x=1",
            "https://github.com/notmnky/auDO-File-Z/releases/download/v1/app%2F.dmg",
            "https://github.com/notmnky/auDO-File-Z/releases/download/app.dmg",     // no tag
            "file:///etc/passwd",
        ];
        for u in bad_urls {
            assert!(validate_update_request(u, "app.dmg").is_err(), "{u}");
        }
        let bad_names = ["../app.dmg", "/tmp/app.dmg", "..\\app.dmg", ".app.dmg", "app.sh", "other.dmg", ""];
        for n in bad_names {
            assert!(validate_update_request(ok_url, n).is_err(), "{n:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_not_reported_as_duplicates() {
        let sandbox = temp_sandbox("hardlink");
        let scan = sandbox.join("scan");
        fs::create_dir_all(scan.join("sub")).unwrap();
        let a = scan.join("a.txt");
        write_file(&a, b"hard linked content");
        fs::hard_link(&a, scan.join("sub").join("a-link.txt")).unwrap();

        // Only one name of the hard-linked file is crawled...
        assert_eq!(crawl(&scan, &[]).len(), 1);
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let groups = run_scan_internal(&scan.to_string_lossy(), "", &flag, None::<fn(usize)>).unwrap();
        assert!(groups.is_empty(), "a hard link is not a duplicate: {groups:?}");

        // ...but a real copy next to it is still found.
        write_file(&scan.join("copy.txt"), b"hard linked content");
        let groups = run_scan_internal(&scan.to_string_lossy(), "", &flag, None::<fn(usize)>).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        let _ = fs::remove_dir_all(&sandbox);
    }
}
