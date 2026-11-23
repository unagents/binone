import { describe, expect, it } from "vitest";
import {
  addTag,
  currentStep,
  describeMissingSource,
  emptyForm,
  formFromRequest,
  laneText,
  newestFirst,
  normalizeTags,
  relativeTime,
  sameForm,
  submitProblems,
  toDraftInput,
} from "../app/utils/requests";
import type { Request, Step } from "../app/types/binone";

const request: Request = {
  id: "req_1",
  version: 3,
  title: "t",
  body: "b",
  project: "p",
  kind: "task",
  tags: ["x"],
  protocol: null,
  change: "chg",
  lane: "draft",
  createdMs: 1,
  updatedMs: 2,
};

describe("tags", () => {
  it("trims, drops empty and repeated", () => {
    expect(normalizeTags([" a ", "", "a", "b"])).toEqual(["a", "b"]);
    expect(addTag(["a"], " b ")).toEqual(["a", "b"]);
    expect(addTag(["a"], "a")).toEqual(["a"]);
    expect(addTag(["a"], "  ")).toEqual(["a"]);
  });
});

describe("toDraftInput", () => {
  it("creates without id or version", () => {
    const input = toDraftInput({ ...emptyForm("p"), title: "x" }, null);
    expect(input.id).toBeUndefined();
    expect(input.version).toBeUndefined();
    expect(input.protocol).toBeNull();
  });
  it("updates with the version read and keeps the change", () => {
    const base = { id: request.id, version: request.version, change: request.change };
    const input = toDraftInput(formFromRequest(request), base);
    expect(input).toMatchObject({ id: "req_1", version: 3, change: "chg", tags: ["x"] });
  });
  it("overwrites with the version the user chose to take over", () => {
    const base = { id: "req_1", version: 3, change: null };
    expect(toDraftInput(formFromRequest(request), base, 5).version).toBe(5);
  });
});

describe("sameForm / submitProblems", () => {
  it("compares tags by their normalized form", () => {
    expect(sameForm({ ...emptyForm(), tags: ["a", "a"] }, { ...emptyForm(), tags: ["a"] })).toBe(true);
    expect(sameForm({ ...emptyForm(), title: "a" }, emptyForm())).toBe(false);
  });
  it("lists what is missing before todo", () => {
    expect(submitProblems(emptyForm())).toHaveLength(4);
    expect(submitProblems({ title: "t", body: "b", project: "p", kind: "k", tags: [], protocol: "" })).toEqual([]);
  });
});

describe("laneText", () => {
  it("uses the label when there is one", () => {
    expect(laneText("doing", [{ name: "doing", label: "作業中" }])).toBe("作業中");
    expect(laneText("doing", [{ name: "doing", label: null }])).toBe("doing");
    expect(laneText("x", null)).toBe("x");
  });
});

describe("relativeTime", () => {
  const now = 10 * 24 * 3_600_000;
  it("reads in minutes, hours and days", () => {
    expect(relativeTime(now - 10_000, now)).toBe("たった今");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5 分前");
    expect(relativeTime(now - 3 * 3_600_000, now)).toBe("3 時間前");
    expect(relativeTime(now - 2 * 24 * 3_600_000, now)).toBe("2 日前");
    expect(relativeTime(now - 8 * 24 * 3_600_000, now)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });
});

describe("history and steps", () => {
  it("sorts newest first and keeps order for equal times", () => {
    const sorted = newestFirst([
      { source: "binone", at_ms: 1, summary: "a" },
      { source: "bantou", at_ms: 3, summary: "b" },
      { source: "bantou", at_ms: 3, summary: "c" },
    ]);
    expect(sorted.map((e) => e.summary)).toEqual(["b", "c", "a"]);
  });
  it("finds the current step", () => {
    const step = (key: string, position: number, status: Step["status"]): Step => ({ request_id: "r", key, position, artifact: null, completion: null, status });
    expect(currentStep([])).toBeNull();
    expect(currentStep([step("a", 0, "done"), step("c", 2, "pending"), step("b", 1, "active")])?.key).toBe("b");
    expect(currentStep([step("a", 0, "done"), step("b", 1, "pending")])?.key).toBe("b");
    expect(currentStep([step("a", 0, "done")])).toBeNull();
  });
});

describe("describeMissingSource", () => {
  it("names the service and why it is missing", () => {
    const d = describeMissingSource({ service: "open_spec", kind: "external_unavailable", detail: "x" });
    expect(d.name).toBe("openspec");
    expect(d.reason).toContain("未接続");
    expect(describeMissingSource({ service: "bantou", kind: "invalid_input", detail: "x" }).reason).toContain("未対応");
  });
});
