import { describe, expect, it } from "vitest";
import { bandText, newNotifications, withoutBand } from "../app/utils/notifications";
import type { Notification } from "../app/types/binone";

function n(id: number, at_ms: number, summary = "s", source_session: string | null = null): Notification {
  return { id, source_session, summary, request_id: null, question_id: null, at_ms };
}

describe("newNotifications", () => {
  it("finds nothing new on the first load", () => {
    expect(newNotifications(null, [n(1, 10), n(2, 20)])).toEqual([]);
  });
  it("returns only the ids not seen before, newest first", () => {
    const previous = [n(1, 10)];
    const next = [n(1, 10), n(2, 20), n(3, 30)];
    expect(newNotifications(previous, next).map((x) => x.id)).toEqual([3, 2]);
  });
  it("finds nothing when the list is the same or shorter", () => {
    expect(newNotifications([n(1, 10), n(2, 20)], [n(2, 20)])).toEqual([]);
    expect(newNotifications([], [])).toEqual([]);
  });
  it("treats a first list that was empty as known", () => {
    expect(newNotifications([], [n(1, 10)]).map((x) => x.id)).toEqual([1]);
  });
  it("breaks ties on the time by id", () => {
    expect(newNotifications([], [n(1, 10), n(2, 10)]).map((x) => x.id)).toEqual([2, 1]);
  });
});

describe("bandText", () => {
  it("names the session and keeps a short summary", () => {
    expect(bandText(n(1, 1, "どの案で進めますか", "worker-1"))).toEqual({ source: "worker-1", summary: "どの案で進めますか" });
  });
  it("cuts a long summary and falls back to a name without a session", () => {
    const text = bandText(n(1, 1, "あ".repeat(100)), 10);
    expect(text.summary).toBe(`${"あ".repeat(10)}…`);
    expect(text.source).toBe("binone");
  });
});

describe("withoutBand", () => {
  it("drops only the dismissed one", () => {
    expect(withoutBand([n(1, 1), n(2, 2)], 1).map((x) => x.id)).toEqual([2]);
  });
});
