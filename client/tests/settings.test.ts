import { describe, expect, it } from "vitest";
import { canMoveLane, laneLabel, moveItem, validateKind, validateLaneName, validateProjectName } from "../app/utils/settings";

const lanes = [
  { name: "draft", label: null },
  { name: "todo", label: null },
  { name: "doing", label: "作業中" },
];

describe("moveItem", () => {
  it("moves an item and leaves the original", () => {
    const list = ["a", "b", "c"];
    expect(moveItem(list, 2, -1)).toEqual(["a", "c", "b"]);
    expect(list).toEqual(["a", "b", "c"]);
  });
  it("does nothing out of range", () => {
    expect(moveItem(["a", "b"], 0, -1)).toEqual(["a", "b"]);
    expect(moveItem(["a", "b"], 1, 1)).toEqual(["a", "b"]);
  });
});

describe("canMoveLane", () => {
  it("keeps draft first", () => {
    expect(canMoveLane(lanes, 0, 1)).toBe(false);
    expect(canMoveLane(lanes, 1, -1)).toBe(false);
    expect(canMoveLane(lanes, 1, 1)).toBe(true);
    expect(canMoveLane(lanes, 2, 1)).toBe(false);
  });
});

describe("the name checks", () => {
  it("accept what the server accepts for a lane", () => {
    expect(validateLaneName("review_2", lanes)).toBeNull();
    for (const bad of ["", "Review", "a b", "レビュー", "todo"]) expect(validateLaneName(bad, lanes)).not.toBeNull();
  });
  it("refuse an empty or repeated kind or project", () => {
    expect(validateKind("", [])).not.toBeNull();
    expect(validateKind("bug", ["bug"])).not.toBeNull();
    expect(validateKind("bug", ["task"])).toBeNull();
    expect(validateProjectName("  ", [])).not.toBeNull();
    expect(validateProjectName(" binone ", ["binone"])).not.toBeNull();
  });
  it("show a label when there is one", () => {
    expect(laneLabel(lanes[2]!)).toBe("作業中");
    expect(laneLabel(lanes[0]!)).toBe("draft");
  });
});
