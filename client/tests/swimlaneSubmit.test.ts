import { describe, expect, it } from "vitest";
import type { ApiError } from "../app/utils/api/errors";
import type { SubmitBatch, SubmitCheck } from "../app/types/binone";
import {
  canDrop,
  describeMissing,
  describeSubmitCode,
  grabbedIds,
  runSubmitFlow,
  toggleSelected,
  type BatchItem,
  type StopChoice,
  type StopProblem,
} from "../app/utils/swimlaneSubmit";

const items: BatchItem[] = [
  { id: "1", title: "one", version: 1 },
  { id: "2", title: "two", version: 1 },
  { id: "3", title: "three", version: 1 },
];
const pass = (id: string): SubmitCheck => ({ id, ok: true, code: null, missing: [], message: "" });
const incomplete = (id: string): SubmitCheck => ({ id, ok: false, code: "incomplete", missing: ["protocol"], message: "missing protocol" });
const conflict = (id: string): SubmitCheck => ({ id, ok: false, code: "version_conflict", missing: [], message: "moved on" });

interface Options {
  /** What `check` answers for an id, in turn (the last one repeats); a thrown value is a failed call. */
  checks?: Record<string, (SubmitCheck | ApiError)[]>;
  choices?: StopChoice[];
  batch?: SubmitBatch | ApiError | ((sent: BatchItem[]) => SubmitBatch | ApiError);
  /** The versions the server has now, for `reread`. */
  fresh?: Record<string, number>;
  /** Close the modal when this call is made: "check:2", "stop", "submit". */
  closeAt?: string;
}

function harness(options: Options = {}) {
  const log: string[] = [];
  const checked: string[] = [];
  const titles: string[] = [];
  const stops: StopProblem[] = [];
  const sent: BatchItem[][] = [];
  const turns = new Map<string, number>();
  let closed = false;
  const hooks = {
    check: async (item: BatchItem) => {
      log.push(`check:${item.id}@${item.version}`);
      checked.push(item.id);
      if (options.closeAt === `check:${item.id}`) closed = true;
      const list = options.checks?.[item.id] ?? [pass(item.id)];
      const turn = turns.get(item.id) ?? 0;
      turns.set(item.id, turn + 1);
      const answer = list[Math.min(turn, list.length - 1)]!;
      if ("kind" in answer) throw answer;
      return answer;
    },
    submit: async (list: BatchItem[]) => {
      log.push("submit");
      sent.push(list);
      const answer = typeof options.batch === "function" ? options.batch(list) : (options.batch ?? { ok: true, results: list.map((i) => pass(i.id)), requests: [] });
      if ("kind" in answer) throw answer;
      return answer;
    },
    reread: async (list: readonly BatchItem[]) => {
      log.push("reread");
      return list.map((i) => ({ ...i, version: options.fresh?.[i.id] ?? i.version }));
    },
    onChecking: (item: BatchItem) => titles.push(item.title),
    onSubmitting: () => log.push("locked"),
    onStop: async (problem: StopProblem) => {
      log.push("stop");
      stops.push(problem);
      if (options.closeAt === "stop") closed = true;
      return (options.choices ?? []).shift() ?? "abort";
    },
    isClosed: () => closed,
  };
  return { hooks, log, checked, titles, stops, sent, close: () => (closed = true) };
}

describe("runSubmitFlow", () => {
  it("checks the drafts one at a time in order, showing each title, then submits them all in one call", async () => {
    const h = harness();
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.log).toEqual(["check:1@1", "check:2@1", "check:3@1", "locked", "submit"]);
    expect(h.titles).toEqual(["one", "two", "three"]);
    expect(h.sent).toHaveLength(1);
    expect(h.sent[0]!.map((i) => [i.id, i.version])).toEqual([["1", 1], ["2", 1], ["3", 1]]);
    expect(outcome.status).toBe("submitted");
  });

  it("stops at the draft that does not pass, with its code and missing fields, and calls nothing that changes", async () => {
    const h = harness({ checks: { "2": [incomplete("2")] }, choices: ["abort"] });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.log).toEqual(["check:1@1", "check:2@1", "stop"]);
    expect(h.stops).toHaveLength(1);
    const problem = h.stops[0]!;
    expect(problem.kind === "refused" && problem.check.code).toBe("incomplete");
    expect(problem.kind === "refused" && problem.check.missing).toEqual(["protocol"]);
    expect(problem.item?.title).toBe("two");
    expect(h.sent).toEqual([]);
    expect(outcome.status).toBe("left");
  });

  it("has no way to skip a draft: after a refusal only a new check or an abort go on", async () => {
    const h = harness({ checks: { "2": [incomplete("2")] }, choices: ["abort"] });
    await runSubmitFlow(items, h.hooks);
    expect(h.checked).toEqual(["1", "2"]);
    expect(h.sent).toEqual([]);
  });

  it("reads the list again and checks from the first draft when the user checks again, then submits with the new versions", async () => {
    const h = harness({ checks: { "2": [incomplete("2"), pass("2")] }, choices: ["retry"], fresh: { "2": 5 } });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.log).toEqual(["check:1@1", "check:2@1", "stop", "reread", "check:1@1", "check:2@5", "check:3@1", "locked", "submit"]);
    expect(h.sent[0]!.find((i) => i.id === "2")!.version).toBe(5);
    expect(outcome.status).toBe("submitted");
  });

  it("asks again each time a check fails again", async () => {
    const h = harness({ checks: { "1": [incomplete("1")] }, choices: ["retry", "retry", "abort"] });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.stops).toHaveLength(3);
    expect(h.sent).toEqual([]);
    expect(outcome.status).toBe("left");
  });

  it("reads again by itself when a check meets a version that moved on, then checks again", async () => {
    const h = harness({ checks: { "1": [conflict("1"), pass("1")] }, fresh: { "1": 4 } });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.log.slice(0, 3)).toEqual(["check:1@1", "reread", "check:1@4"]);
    expect(h.stops).toEqual([]);
    expect(outcome.status).toBe("submitted");
  });

  it("stops with the reason when the version is still in conflict after reading again", async () => {
    const h = harness({ checks: { "1": [conflict("1")] }, choices: ["abort"] });
    await runSubmitFlow(items, h.hooks);
    expect(h.log).toEqual(["check:1@1", "reread", "check:1@1", "stop"]);
    const problem = h.stops[0]!;
    expect(problem.kind === "refused" && problem.check.code).toBe("version_conflict");
  });

  it("stops on a failed check as an error with the reason, and nothing was sent", async () => {
    const down: ApiError = { kind: "unreachable", detail: "no route" };
    const h = harness({ checks: { "2": [down, pass("2")] }, choices: ["retry"] });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.stops[0]).toMatchObject({ kind: "error", error: down });
    expect(outcome.status).toBe("submitted");
  });

  it("shows a thrown value that is not an ApiError as internal", async () => {
    const h = harness();
    h.hooks.check = async () => {
      throw new Error("boom");
    };
    await runSubmitFlow(items, h.hooks);
    expect(h.stops[0]).toMatchObject({ kind: "error", error: { kind: "internal" } });
  });

  it("changes nothing when the modal is closed before the call: during a check, at a stop, or just before submit", async () => {
    for (const closeAt of ["check:1", "check:3", "stop"]) {
      const h = harness({ closeAt, checks: closeAt === "stop" ? { "2": [incomplete("2")] } : {}, choices: ["retry"] });
      const outcome = await runSubmitFlow(items, h.hooks);
      expect(h.sent, closeAt).toEqual([]);
      expect(outcome.status, closeAt).toBe("left");
    }
    const noMore = harness({ closeAt: "check:1" });
    await runSubmitFlow(items, noMore.hooks);
    expect(noMore.checked).toEqual(["1"]);
    const before = harness();
    before.close();
    expect((await runSubmitFlow(items, before.hooks)).status).toBe("left");
    expect(before.log).toEqual([]);
  });

  it("does nothing for no drafts", async () => {
    const h = harness();
    expect((await runSubmitFlow([], h.hooks)).status).toBe("left");
    expect(h.log).toEqual([]);
  });

  it("does not look at the closed flag once the call is made: the answer is the outcome", async () => {
    const h = harness();
    h.hooks.onSubmitting = () => h.close();
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.sent).toHaveLength(1);
    expect(outcome.status).toBe("submitted");
  });

  it("reports an unknown outcome and never sends the batch again", async () => {
    const unknown: ApiError = { kind: "outcome_unknown", detail: "no answer" };
    const h = harness({ batch: unknown, choices: ["retry"] });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(outcome).toEqual({ status: "unknown", error: unknown });
    expect(h.sent).toHaveLength(1);
    expect(h.stops).toEqual([]);
  });

  it("reports a failed call as a failure, not retried", async () => {
    const error: ApiError = { kind: "unreachable", detail: "no route" };
    const h = harness({ batch: error });
    expect(await runSubmitFlow(items, h.hooks)).toEqual({ status: "failed", error });
    expect(h.sent).toHaveLength(1);
  });

  it("turns a refused batch into a stop on the first refused draft, with nothing changed", async () => {
    const refused = (list: BatchItem[]): SubmitBatch => ({
      ok: false,
      results: list.map((i) => (i.id === "3" ? conflict("3") : pass(i.id))),
      requests: [],
    });
    const h = harness({ batch: refused, choices: ["abort"] });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.stops[0]).toMatchObject({ kind: "refused", item: { id: "3" }, check: { code: "version_conflict" } });
    expect(outcome.status).toBe("left");
    expect(h.sent).toHaveLength(1);
  });

  it("checks everything again after a refused batch when the user asks", async () => {
    let calls = 0;
    const h = harness({
      choices: ["retry"],
      fresh: { "3": 2 },
      batch: (list) => (++calls === 1 ? { ok: false, results: list.map((i) => (i.id === "3" ? conflict("3") : pass(i.id))), requests: [] } : { ok: true, results: list.map((i) => pass(i.id)), requests: [] }),
    });
    const outcome = await runSubmitFlow(items, h.hooks);
    expect(h.sent).toHaveLength(2);
    expect(h.sent[1]!.find((i) => i.id === "3")!.version).toBe(2);
    expect(outcome.status).toBe("submitted");
  });
});

describe("the texts of a refusal", () => {
  it("name each code and show an unknown one as it is", () => {
    for (const code of ["not_found", "not_draft", "version_conflict", "incomplete"]) expect(describeSubmitCode(code)).not.toBe(code);
    expect(describeSubmitCode("something_new")).toBe("something_new");
    expect(describeSubmitCode(null)).toBe("理由は不明です");
  });
  it("call the missing fields by the screens' names", () => {
    expect(describeMissing(["title", "project", "protocol", "other"])).toEqual(["タイトル", "プロジェクト", "protocol", "other"]);
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
