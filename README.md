# auDO File Z
> **Nostalgic Cryptographic File Deduplication for macOS**  
> *100% Air-Gapped, Offline Operational Security*

---

## 1. Overview & Context

**auDO File Z** (Version 9000) is a high-performance, native macOS full-stack desktop application designed to securely scan directories, identify identical files cryptographically, and resolve duplicates visually. 

The application is styled with a rigid, nostalgic **Windows XP (Luna Theme)** aesthetic, bringing back early-2000s desktop nostalgia while executing lightning-fast, modern backend file systems operations on Apple Silicon.

### Key Architectural Pillars:
*   **Total Data Privacy**: 100% offline. Zero networking logic, telemetry tracking, or update checkers are permitted.
*   **Size-First Optimized Indexing**: Files are indexed by exact byte size first. Hashing is only performed on size matches, skipping 90% of unique files automatically.
*   **Rayon Parallel Processing**: Utilizes multi-threaded SHA-256 cryptographic hashing to run scans across all CPU cores.
*   **Buffer-Streamed Hashing**: Reads file contents in constant 8KB chunks, preventing memory/RAM spikes on multi-gigabyte files.
*   **macOS Sandbox Compliance**: Moves resolved files to the native macOS Trash (`~/.Trash`) using Finder APIs instead of executing permanent destructive commands.

---

## 2. Installation & Setup

### Terminal Installation (via curl)
You can download, extract, and install the latest release directly into your `/Applications` folder using the following command in your terminal:
```bash
curl -L -s https://github.com/notMNKY/audofilez/releases/latest/download/audofilez.app.tar.gz | tar -xz -C /Applications
```

### Full Disk Access (FDA) Onboarding
To read protected user directories and calculate file hashes, macOS requires Full Disk Access:
1. Open **System Settings** -> **Privacy & Security** -> **Full Disk Access**.
2. Click the **Add (+)** button at the bottom of the list.
3. Select **auDO File Z** (or your Terminal program if running from source) and enable the toggle switch.

---

## 3. Configuration & Scanning

1.  **Select Directory**: Click the **Browse...** button to launch the native directory selector and target your search folder.
2.  **Filter Extensions**: Enter comma-separated formats in the extension textbox (e.g. `.mp3, .wav, .flac, .aif`).
    *   *Note*: The textbox is strictly limited to 300 characters. If left completely blank, the scanner automatically walks and checks all file types.
3.  **Run Scan**: Click the **Scan** button. Rayon threads will crawl, pre-filter, hash duplicate size candidates in parallel, and populate the retro grid.

---

## 4. Selection & Resolution Modes

### Dynamic Selection Mode
Using the dropdown above the table, choose how checkmarks are set:
*   **Manually Select**: Manually toggle checks.
*   **Auto-Select Oldest**: Automatically checks older duplicate copies, leaving the newest (most recently modified) copy unchecked (preserved as original).
*   **Auto-Select Newest**: Automatically checks newer duplicate copies, leaving the oldest copy unchecked (preserved as original).

### Resolution Engine Options
Before resolving duplicates, toggle the action type at the bottom:
1.  **Clean Delete (Move to Trash)**: Relocates checked duplicates to your macOS Trash.
2.  **Link Preservation**: Moves duplicate copies to Trash and instantly replaces them with symbolic links pointing directly back to the preserved original copy.

---

## 5. Dynamic Skin Swapping

Customize the application's visual layout dynamically to match your preference:
*   **DoorsXP-Z (Retro)**: The default nostalgic Windows XP Luna theme featuring a bright blue title bar, retro grey bevels, and classic `#ECE9D8` dialog boxes.
*   **VinylBox-Z (Dark)**: A dark, sleek, professional theme mimicking the Rekordbox / AlphaTheta DJ aesthetic, featuring matte black backgrounds (`#121212`), high-contrast neon orange accents (`#FF6600`), flat edges, and customized dark scrollbars.

You can toggle skins instantly by selecting them from the native macOS application menu under **View > Skins** or the custom in-window dropdown menu.

---

## 6. Safety Mitigation Guardrails

*   **Anti-Data Loss validation**: You can change checkpoints freely, but the application enforces that at least one copy in each duplicate group must remain unchecked. If you attempt to check all copies, a flashing warning displays and disables the action button.
*   **Circular Symlink Block**: The Rust backend validates that the target preserved file is not itself scheduled for deletion in the queue. If a circular reference loop is detected, the command aborts.

---

## 7. Development & Credits
*   **Author**: Nishank / notMNKY
*   **Email**: [nishank@gmx.de](mailto:nishank@gmx.de)
*   **Support Developer**: [Kofi Support Profile](https://ko-fi.com/nishank)
*   **Frameworks**: Tauri v2, React + TS, Tailwind CSS v3, Rust (std::fs, rayon, sha2, trash, rfd)
