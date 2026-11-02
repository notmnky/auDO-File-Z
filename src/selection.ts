// Pure selection logic for duplicate groups. No React, no Tauri: easy to unit test.

export type SelectionMode = "manual" | "oldest" | "newest";

export interface SelectableFile {
  path: string;
  modified: number; // epoch ms
  created: number; // epoch ms
  read_only: boolean;
}

export interface SelectableGroup {
  files: SelectableFile[];
}

export interface SelectionResult {
  /** path -> true if the file is checked (marked for removal). */
  selections: Record<string, boolean>;
  /** Groups where auto-select checked nothing because of read-only copies. */
  readOnlyBlockedGroups: number;
}

/**
 * Order used to decide which copy to keep. Compares modified time first,
 * then created time, then path, so the result is deterministic even when
 * copies share a timestamp (e.g. Finder copies keep the original modified time).
 * Returns a negative number when `a` is older than `b`.
 */
export function compareAge(a: SelectableFile, b: SelectableFile): number {
  if (a.modified !== b.modified) return a.modified - b.modified;
  if (a.created !== b.created) return a.created - b.created;
  return a.path < b.path ? -1 : a.path > b.path ? 1 : 0;
}

/**
 * Index of the copy to keep.
 * "oldest"  = auto-select the older copies, so keep the NEWEST one.
 * "newest"  = auto-select the newer copies, so keep the OLDEST one.
 */
export function pickKeepIndex(files: SelectableFile[], mode: Exclude<SelectionMode, "manual">): number {
  let keep = 0;
  for (let i = 1; i < files.length; i++) {
    const cmp = compareAge(files[i], files[keep]);
    if (mode === "oldest" ? cmp > 0 : cmp < 0) keep = i;
  }
  return keep;
}

export function computeSelections(mode: SelectionMode, groups: SelectableGroup[]): SelectionResult {
  const selections: Record<string, boolean> = {};
  let readOnlyBlockedGroups = 0;
  if (mode === "manual") return { selections, readOnlyBlockedGroups };

  for (const group of groups) {
    if (group.files.length === 0) continue;
    const keep = pickKeepIndex(group.files, mode);
    let checkedInGroup = 0;
    let hasReadOnly = false;

    group.files.forEach((file, index) => {
      // The kept copy is never checked; read-only copies are never auto-checked.
      const checked = index !== keep && !file.read_only;
      if (file.read_only) hasReadOnly = true;
      if (checked) checkedInGroup++;
      selections[file.path] = checked;
    });

    if (checkedInGroup === 0 && hasReadOnly) readOnlyBlockedGroups++;
  }
  return { selections, readOnlyBlockedGroups };
}
