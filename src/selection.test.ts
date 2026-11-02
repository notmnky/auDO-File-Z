import { describe, it, expect } from "vitest";
import { computeSelections, pickKeepIndex, type SelectableFile } from "./selection";

const f = (path: string, modified: number, created: number, read_only = false): SelectableFile => ({
  path,
  modified,
  created,
  read_only,
});

describe("computeSelections", () => {
  it("oldest mode keeps the newest copy and checks the rest", () => {
    const { selections } = computeSelections("oldest", [{ files: [f("a", 100, 1), f("b", 300, 2), f("c", 200, 3)] }]);
    expect(selections).toEqual({ a: true, b: false, c: true });
  });

  it("newest mode keeps the oldest copy and checks the rest", () => {
    const { selections } = computeSelections("newest", [{ files: [f("a", 100, 1), f("b", 300, 2), f("c", 200, 3)] }]);
    expect(selections).toEqual({ a: false, b: true, c: true });
  });

  it("breaks modified-time ties by created time (Finder copies)", () => {
    const files = [f("a", 100, 500), f("b", 100, 100), f("c", 100, 300)];
    expect(files[pickKeepIndex(files, "oldest")].path).toBe("a"); // newest created
    expect(files[pickKeepIndex(files, "newest")].path).toBe("b"); // oldest created
  });

  it("is deterministic when everything ties", () => {
    const files = [f("z", 1, 1), f("a", 1, 1), f("m", 1, 1)];
    const order = (arr: SelectableFile[]) => arr[pickKeepIndex(arr, "oldest")].path;
    expect(order(files)).toBe(order([...files].reverse()));
  });

  it("never checks every copy in a group", () => {
    for (const mode of ["oldest", "newest"] as const) {
      const { selections } = computeSelections(mode, [{ files: [f("a", 1, 1), f("b", 1, 1)] }]);
      expect(Object.values(selections).filter((v) => !v).length).toBeGreaterThanOrEqual(1);
    }
  });

  it("skips read-only copies and reports groups blocked by them", () => {
    const res = computeSelections("oldest", [
      { files: [f("a", 100, 1, true), f("b", 300, 2, true)] }, // all read-only
      { files: [f("c", 100, 1), f("d", 300, 2)] }, // normal
    ]);
    expect(res.selections).toEqual({ a: false, b: false, c: true, d: false });
    expect(res.readOnlyBlockedGroups).toBe(1);
  });

  it("does not count a group as blocked when something was checked", () => {
    const res = computeSelections("oldest", [{ files: [f("a", 100, 1, true), f("b", 300, 2), f("c", 200, 3)] }]);
    expect(res.selections).toEqual({ a: false, b: false, c: true });
    expect(res.readOnlyBlockedGroups).toBe(0);
  });

  it("manual mode selects nothing", () => {
    expect(computeSelections("manual", [{ files: [f("a", 1, 1), f("b", 2, 2)] }]).selections).toEqual({});
  });
});
