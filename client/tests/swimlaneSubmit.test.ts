import { describe, expect, it } from "vitest";
import type { ApiError } from "../app/utils/api/errors";
import { canDrop, grabbedIds, runSubmitBatch, summarizeBatch, toggleSelected, type BatchItem, type StopChoice } from "../app/utils/swimlaneSubmit";

const items: BatchItem[] = [
  { id: "1", title: "one" },
  { id: "2", title: "two" },
  { id: "3", title: "three" },
];
const refuse: ApiError = { kind: "invalid_input", detail: "protocol is not set" };

function harness(failures: Record<string, ApiError> = {}, choices: StopChoice[] = [], closeAfterSubmit?: string) {
  const calls: string[] = [];
  const started: string[] = [];
  const stops: string[] = [];
  let closed = false;
  const hooks = {
    submit: async (item: BatchItem) => {
      calls.push(item.id);
      if (item.id === closeAfterSubmit) closed = true;
      const failure = failures[item.id];
      if (failure) throw failure;
    },
    onStart: (item: BatchItem) => started.push(item.title),
    onStop: async (item: BatchItem) => {
      stops.push(item.id);
      return choices.shift() ?? "abort";
    },
    isClosed: () => closed,
  };
  return { hooks, calls, started, stops, close: () => (closed = true) };
}

describe("runSubmitBatch", () => {
  it("submits every draft in order and shows the titles in turn", async () => {
    const h = harness();
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.calls).toEqual(["1", "2", "3"]);
    expect(h.started).toEqual(["one", "two", "three"]);
    expect(result.interrupted).toBe(false);
    expect(summarizeBatch(result.entries)).toEqual({ todo: 3, skipped: 0, unknown: 0, left: 0 });
  });

  it("stops at a refused draft and goes on after the user skips it", async () => {
    const h = harness({ "2": refuse }, ["skip"]);
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.stops).toEqual(["2"]);
    expect(h.calls).toEqual(["1", "2", "3"]);
    expect(result.entries.map((e) => e.status)).toEqual(["todo", "skipped", "todo"]);
    expect(result.entries[1]!.reason).toEqual(refuse);
    expect(result.interrupted).toBe(false);
  });

  it("leaves the rest as drafts when the user aborts, and keeps what is already a todo", async () => {
    const h = harness({ "2": refuse }, ["abort"]);
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.calls).toEqual(["1", "2"]);
    expect(result.entries.map((e) => e.status)).toEqual(["todo", "left", "left"]);
    expect(result.interrupted).toBe(true);
    expect(summarizeBatch(result.entries)).toEqual({ todo: 1, skipped: 0, unknown: 0, left: 2 });
  });

  it("can stop on the first draft and on the last", async () => {
    const first = await runSubmitBatch(items, harness({ "1": refuse }, ["abort"]).hooks);
    expect(first.entries.map((e) => e.status)).toEqual(["left", "left", "left"]);
    const last = await runSubmitBatch(items, harness({ "3": refuse }, ["skip"]).hooks);
    expect(last.entries.map((e) => e.status)).toEqual(["todo", "todo", "skipped"]);
    expect(last.interrupted).toBe(false);
  });

  it("asks again for each refused draft", async () => {
    const h = harness({ "1": refuse, "3": refuse }, ["skip", "skip"]);
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.stops).toEqual(["1", "3"]);
    expect(result.entries.map((e) => e.status)).toEqual(["skipped", "todo", "skipped"]);
  });

  it("does not send again when the outcome is unknown, and shows it as unknown", async () => {
    const unknown: ApiError = { kind: "outcome_unknown", detail: "no answer" };
    const h = harness({ "2": unknown }, ["abort"]);
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.calls).toEqual(["1", "2"]);
    expect(result.entries.map((e) => e.status)).toEqual(["todo", "unknown", "left"]);
    const skipped = await runSubmitBatch(items, harness({ "2": unknown }, ["skip"]).hooks);
    expect(skipped.entries.map((e) => e.status)).toEqual(["todo", "unknown", "todo"]);
  });

  it("stops before the next draft when the modal was closed during a call", async () => {
    const h = harness({}, [], "2");
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.calls).toEqual(["1", "2"]);
    expect(result.entries.map((e) => e.status)).toEqual(["todo", "todo", "left"]);
    expect(result.interrupted).toBe(true);
  });

  it("treats a close while the question is open as an abort, even if the answer is skip", async () => {
    const h = harness({ "2": refuse });
    h.hooks.onStop = async () => {
      h.close();
      return "skip";
    };
    const result = await runSubmitBatch(items, h.hooks);
    expect(h.calls).toEqual(["1", "2"]);
    expect(result.entries.map((e) => e.status)).toEqual(["todo", "left", "left"]);
    expect(result.interrupted).toBe(true);
  });

  it("does nothing when closed from the start, and for no cards", async () => {
    const h = harness();
    h.close();
    expect((await runSubmitBatch(items, h.hooks)).entries.every((e) => e.status === "left")).toBe(true);
    expect(h.calls).toEqual([]);
    expect(await runSubmitBatch([], harness().hooks)).toEqual({ entries: [], interrupted: false });
  });

  it("shows a non-ApiError failure as internal", async () => {
    const h = harness();
    h.hooks.submit = async () => {
      throw new Error("boom");
    };
    const result = await runSubmitBatch([items[0]!], h.hooks);
    expect(result.entries[0]!.reason?.kind).toBe("internal");
  });
});

describe("drag and selection", () => {
  it("allows only drafts onto todo", () => {
    expect(canDrop(["draft"], "todo")).toBe(true);
    expect(canDrop(["draft", "draft"], "todo")).toBe(true);
    expect(canDrop(["draft"], "doing")).toBe(false);
    expect(canDrop(["draft"], "draft")).toBe(false);
    expect(canDrop(["todo"], "todo")).toBe(false);
    expect(canDrop(["draft", "todo"], "todo")).toBe(false);
    expect(canDrop([], "todo")).toBe(false);
  });
  it("takes the selection when the grabbed card is in it, else the card alone, in screen order", () => {
    const order = ["a", "b", "c", "d"];
    expect(grabbedIds(["c", "a"], "c", order)).toEqual(["a", "c"]);
    expect(grabbedIds(["c", "a"], "b", order)).toEqual(["b"]);
    expect(grabbedIds([], "d", order)).toEqual(["d"]);
  });
  it("toggles a card in the selection", () => {
    expect(toggleSelected(["a"], "b")).toEqual(["a", "b"]);
    expect(toggleSelected(["a", "b"], "a")).toEqual(["b"]);
  });
});
