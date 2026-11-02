import { describe, it, expect } from "vitest";
import { formatVersionDate, isUpdateAvailable, matchReleaseAsset } from "./updates";

const asset = (name: string) => ({ name, browser_download_url: `https://example/${name}` });
const pick = (names: string[], os: string, arch: string) => matchReleaseAsset(names.map(asset), os, arch)?.name;

// What tauri-action uploads for one release (universal macOS + Windows), incl. updater files.
const CI_RELEASE = [
  "auDO.FileZ_0.1.9005_universal.dmg",
  "auDO.FileZ_universal.app.tar.gz",
  "auDO.FileZ_universal.app.tar.gz.sig",
  "auDO.FileZ_0.1.9005_x64_en-US.msi",
  "auDO.FileZ_0.1.9005_x64_en-US.msi.sig",
  "auDO.FileZ_0.1.9005_x64-setup.exe",
  "auDO.FileZ_0.1.9005_x64-setup.exe.sig",
];

describe("isUpdateAvailable", () => {
  it("compares dotted versions numerically", () => {
    expect(isUpdateAvailable("0.1.9004", "v0.1.9005")).toBe(true);
    expect(isUpdateAvailable("0.1.9004", "v0.1.9004")).toBe(false);
    expect(isUpdateAvailable("0.1.9004", "v0.1.900")).toBe(false);
    expect(isUpdateAvailable("0.1.9", "0.2")).toBe(true);
    expect(isUpdateAvailable("0.2.0", "v0.1.9999")).toBe(false);
  });
});

describe("matchReleaseAsset", () => {
  it("finds the universal dmg on both Apple Silicon and Intel Macs", () => {
    expect(pick(CI_RELEASE, "macos", "arm64")).toBe("auDO.FileZ_0.1.9005_universal.dmg");
    expect(pick(CI_RELEASE, "macos", "x64")).toBe("auDO.FileZ_0.1.9005_universal.dmg");
  });

  it("prefers an exact-architecture dmg over universal", () => {
    const names = ["a_0.1_universal.dmg", "a_0.1_aarch64.dmg", "a_0.1_x64.dmg"];
    expect(pick(names, "macos", "arm64")).toBe("a_0.1_aarch64.dmg");
    expect(pick(names, "macos", "x64")).toBe("a_0.1_x64.dmg");
  });

  it("never gives an Intel Mac the Apple Silicon build, or the reverse", () => {
    expect(pick(["a_0.1_aarch64.dmg"], "macos", "x64")).toBeUndefined();
    expect(pick(["a_0.1_x64.dmg"], "macos", "arm64")).toBeUndefined();
  });

  it("prefers the Windows .exe installer and falls back to .msi", () => {
    expect(pick(CI_RELEASE, "windows", "x64")).toBe("auDO.FileZ_0.1.9005_x64-setup.exe");
    expect(pick(["a_0.1_x64_en-US.msi", "a_0.1_x64_en-US.msi.sig"], "windows", "x64")).toBe("a_0.1_x64_en-US.msi");
  });

  it("still accepts the names made by scripts/rename-release.js", () => {
    const local = [
      "auDO-File-Z-v0.1.9004-macos-arm64.dmg",
      "auDO-File-Z-v0.1.9004-macos-intel.dmg",
      "auDO-File-Z-v0.1.9004-windows-x64.exe",
    ];
    expect(pick(local, "macos", "arm64")).toBe(local[0]);
    expect(pick(local, "macos", "x64")).toBe(local[1]);
    expect(pick(local, "windows", "x64")).toBe(local[2]);
  });

  it("finds nothing for unsupported platforms or signature files only", () => {
    expect(pick(CI_RELEASE, "windows", "x86")).toBeUndefined();
    expect(pick(CI_RELEASE, "linux", "x64")).toBeUndefined();
    expect(pick(["a.dmg.sig", "a_x64-setup.exe.sig"], "windows", "x64")).toBeUndefined();
    expect(pick(["a_0.1_arm64-setup.exe"], "windows", "x64")).toBeUndefined();
  });
});

describe("formatVersionDate", () => {
  it("formats the calendar date without time-zone shifts", () => {
    expect(formatVersionDate("2026-11-02T23:30:00-08:00")).toBe("2 November 2026");
    expect(formatVersionDate("2026-01-09")).toBe("9 January 2026");
  });
  it("returns an empty string for bad input", () => {
    expect(formatVersionDate("")).toBe("");
    expect(formatVersionDate(undefined)).toBe("");
    expect(formatVersionDate("not a date")).toBe("");
    expect(formatVersionDate("2026-13-01")).toBe("");
  });
});
