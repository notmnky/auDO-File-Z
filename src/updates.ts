// Pure update helpers: no React, no Tauri, so they can be unit tested.

export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

/** True when `latest` (e.g. "v0.1.9005") is newer than `current` ("0.1.9004"). */
export function isUpdateAvailable(current: string, latest: string): boolean {
  const parse = (v: string) => v.trim().replace(/^v/i, "").split(".").map((p) => parseInt(p, 10) || 0);
  const cur = parse(current);
  const lat = parse(latest);
  for (let i = 0; i < Math.max(cur.length, lat.length); i++) {
    const c = cur[i] || 0;
    const l = lat[i] || 0;
    if (l > c) return true;
    if (c > l) return false;
  }
  return false;
}

/**
 * Picks the installer for this machine from a GitHub release.
 *
 * Accepts both naming schemes in use:
 * - CI (tauri-action) names, e.g. `auDO.FileZ_0.1.9005_universal.dmg`,
 *   `…_aarch64.dmg`, `…_x64.dmg`, `…_x64-setup.exe`, `…_x64_en-US.msi`
 * - Local `scripts/rename-release.js` names, e.g. `auDO-File-Z-v0.1.9004-macos-arm64.dmg`,
 *   `…-macos-intel.dmg`, `…-windows-x64.exe`
 *
 * macOS: an exact-architecture .dmg is preferred, a universal .dmg is the fallback.
 * Windows: the .exe (NSIS) installer is preferred, the .msi is the fallback.
 * Returns undefined when nothing fits (for example a 32-bit Windows build).
 */
export function matchReleaseAsset<T extends ReleaseAsset>(assets: T[], os: string, arch: string): T | undefined {
  const o = os.toLowerCase();
  const a = arch.toLowerCase();
  const isArm = a === "arm64" || a === "aarch64";
  const isX64 = a === "x64" || a === "x86_64" || a === "intel";

  const rank = (raw: string): number => {
    const n = raw.toLowerCase();
    if (o === "macos") {
      if (!n.endsWith(".dmg")) return -1;
      if (isArm && (n.includes("macos-arm64") || n.includes("aarch64"))) return 0;
      if (isX64 && (n.includes("macos-intel") || n.includes("x86_64") || /[_-]x64[._-]/.test(n))) return 0;
      if ((isArm || isX64) && n.includes("universal")) return 1;
      return -1;
    }
    if (o === "windows" || o === "win") {
      if (!isX64 || n.includes("arm64") || !/(x64|x86_64)/.test(n)) return -1;
      if (n.endsWith(".exe")) return 0;
      if (n.endsWith(".msi")) return 1;
      return -1;
    }
    return -1;
  };

  let best: T | undefined;
  let bestRank = Infinity;
  for (const asset of assets) {
    if (!asset || typeof asset.name !== "string") continue;
    const r = rank(asset.name);
    if (r >= 0 && r < bestRank) {
      best = asset;
      bestRank = r;
    }
  }
  return best;
}

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

/**
 * "2026-11-02T09:15:00+01:00" -> "2 November 2026".
 * Uses the calendar date written in the string, so the result never shifts
 * a day with the viewer's time zone. Returns "" for anything unparseable.
 */
export function formatVersionDate(iso: string | undefined | null): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso ?? "");
  if (!m) return "";
  const [year, month, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (month < 1 || month > 12 || day < 1 || day > 31) return "";
  return `${day} ${MONTHS[month - 1]} ${year}`;
}
