// Where a request's content is kept, and a body that cannot be read: what is sent, what is said.
import { afterEach, describe, expect, it } from "vitest";
import { callApi } from "../app/composables/useApi";
import { UNSUPPORTED_BY_OPENSPEC_MCP, describeApiError, type ApiError } from "../app/utils/api/errors";
import { setNativeCall } from "../app/utils/api/transport";
import type { DraftInput, Request } from "../app/types/binone";
import {
  BODY_UNREADABLE_TEXT,
  describeBodyUnreadable,
  afterSave,
  baseFromRequest,
  describeSaveTarget,
  emptyForm,
  formFromRequest,
  isBodyUnreadable,
  isDetailBodyUnreadable,
  saveOnce,
  submitProblems,
  toDraftInput,
} from "../app/utils/requests";

const unreadable: Request = {
  id: "r1",
  version: 4,
  title: "題名",
  body: null,
  bodyMissing: true,
  project: "p",
  kind: "task",
  tags: ["x"],
  protocol: "proto",
  change: null,
  lane: "draft",
  createdMs: 1,
  updatedMs: 2,
};
const readable: Request = { ...unreadable, body: "本文", bodyMissing: false };

let restore: (() => void) | null = null;
afterEach(() => {
  restore?.();
  restore = null;
});

/** A native side that records what `api_call` was given and answers with `answer` (or throws it). */
function nativeSide(answer: (args: Record<string, unknown>) => unknown) {
  const calls: Record<string, unknown>[] = [];
  restore = setNativeCall(async (command, args) => {
    if (command !== "api_call") throw new Error(command);
    calls.push(args ?? {});
    return answer(args ?? {});
  });
  return calls;
}

describe("a body that cannot be read is never written back empty", () => {
  it("is told from a null body or the bodyMissing flag", () => {
    expect(isBodyUnreadable(unreadable)).toBe(true);
    expect(isBodyUnreadable({ body: null, bodyMissing: false })).toBe(true);
    expect(isBodyUnreadable({ body: "", bodyMissing: true })).toBe(true);
    expect(isBodyUnreadable({ body: "", bodyMissing: false })).toBe(false);
    expect(isBodyUnreadable(readable)).toBe(false);
  });

  it("is not in the input of the update, and the title and the managed items are", () => {
    const form = { ...formFromRequest(unreadable), title: "新しい題名", kind: "bug" };
    expect(form.body).toBe("");
    const input = toDraftInput(form, baseFromRequest(unreadable), undefined, formFromRequest(unreadable));
    expect("body" in input).toBe(false);
    expect(input).toMatchObject({ id: "r1", version: 4, title: "新しい題名", kind: "bug" });
    // project did not change: left out ("not changed"), not sent again.
    expect("project" in input).toBe(false);
  });

  it("is not sent even when the form somehow holds a body", () => {
    const form = { ...formFromRequest(unreadable), body: "x" };
    expect("body" in toDraftInput(form, baseFromRequest(unreadable))).toBe(false);
    expect("body" in toDraftInput(form, baseFromRequest(unreadable), undefined, formFromRequest(unreadable))).toBe(false);
  });

  it("is not in the arguments handed to the communication layer", async () => {
    const calls = nativeSide(() => ({ ...unreadable, title: "" }));
    const form = { ...formFromRequest(unreadable), kind: "bug" };
    await saveOnce((input) => callApi("saveDraft", input), form, baseFromRequest(unreadable), formFromRequest(unreadable));
    expect(calls).toHaveLength(1);
    const args = calls[0]?.args as Record<string, unknown>;
    expect(calls[0]?.op).toBe("saveDraft");
    expect(Object.keys(args)).not.toContain("body");
    expect(Object.keys(args)).not.toContain("title");
    expect(args).toMatchObject({ id: "r1", version: 4, kind: "bug" });
  });

  it("does not make a todo of it for an empty body: the server judges it", () => {
    const form = formFromRequest(unreadable);
    expect(submitProblems(form)).toContain("本文を入れてください");
    expect(submitProblems(form, baseFromRequest(unreadable))).toEqual([]);
  });

  it("says why, in one sentence for both screens", () => {
    expect(BODY_UNREADABLE_TEXT).toBe("本文を取り出せません（openspec-mcp に本文を読む口がありません）");
    const record = { ...readable, body: null, body_missing: true } as never;
    expect(isDetailBodyUnreadable({ request: record, missing_sources: [] })).toBe(true);
    expect(isDetailBodyUnreadable({ request: { body: "t" } as never, missing_sources: [{ item: "body", service: "open_spec", kind: "unsupported" as never, detail: "" }] })).toBe(true);
    expect(isDetailBodyUnreadable({ request: { body: "t" } as never, missing_sources: [{ service: "open_spec", kind: "external_unavailable", detail: "" }] })).toBe(false);
  });
});

describe("only the items that changed are sent", () => {
  it("sends the body of a request made on this screen, and then only when it changed", async () => {
    // New request: everything is sent.
    const created = toDraftInput({ ...emptyForm("p"), title: "t", body: "b" }, null);
    expect(created).toMatchObject({ title: "t", body: "b" });
    const saved = formFromRequest({ ...readable, title: "t", body: "b" });
    // The answer to a create carries the body that was sent.
    const first = afterSave({ ...emptyForm("p"), title: "t", body: "b" }, emptyForm("p"), created, { ...readable, title: "t", body: "b" }, null);
    expect(first.base.bodyUnreadable).toBe(false);
    // Update with the body changed: sent. Unchanged: left out, and the title likewise.
    const edited = { ...saved, body: "b2" };
    expect(toDraftInput(edited, first.base, undefined, saved)).toMatchObject({ body: "b2" });
    expect("title" in toDraftInput(edited, first.base, undefined, saved)).toBe(false);
    const untouched = toDraftInput({ ...saved, kind: "bug" }, first.base, undefined, saved);
    expect("body" in untouched).toBe(false);
    expect("title" in untouched).toBe(false);
  });

  it("keeps what the screen has for an item that was left out, though the answer says it is missing", () => {
    const base = { id: "r1", version: 4, change: null, bodyUnreadable: false };
    const saved = formFromRequest({ ...readable, title: "t", body: "b" });
    const input = toDraftInput({ ...saved, kind: "bug" }, base, undefined, saved);
    // What the server answers to a save that sent no title and no body.
    const answer: Request = { ...readable, version: 5, title: "", body: null, bodyMissing: true, kind: "bug" };
    const after = afterSave({ ...saved, kind: "bug" }, saved, input, answer, base);
    expect(after.saved).toMatchObject({ title: "t", body: "b", kind: "bug" });
    expect(after.form).toMatchObject({ title: "t", body: "b" });
    expect(after.base).toMatchObject({ version: 5, bodyUnreadable: false });
  });

  it("stays unreadable after a save that sent no body, and is readable after one that did", () => {
    const form = formFromRequest(unreadable);
    const base = baseFromRequest(unreadable);
    const input = toDraftInput({ ...form, kind: "bug" }, base, undefined, form);
    const answer: Request = { ...unreadable, version: 5, title: "", kind: "bug" };
    expect(afterSave({ ...form, kind: "bug" }, form, input, answer, base).base.bodyUnreadable).toBe(true);
    expect(afterSave({ ...form, kind: "bug" }, form, { ...input, body: "x" }, answer, base).base.bodyUnreadable).toBe(false);
  });

  it("does not send a blank title on an update", () => {
    const saved = formFromRequest(readable);
    const base = baseFromRequest(readable);
    expect("title" in toDraftInput({ ...saved, title: "  " }, base, undefined, saved)).toBe(false);
  });
});

describe("a failed save is told apart, and is sent once", () => {
  const failing = (error: ApiError) => {
    const calls = nativeSide(() => {
      throw error;
    });
    return calls;
  };
  const save = (form = formFromRequest(readable)) =>
    saveOnce((input: DraftInput) => callApi("saveDraft", input), form, baseFromRequest(readable), formFromRequest(readable));

  it("says openspec-mcp is unsupported and names the missing tools", async () => {
    const error: ApiError = { kind: "external_unavailable", detail: "openspec-mcp 未対応", reason: UNSUPPORTED_BY_OPENSPEC_MCP, missing: ["create", "update"] };
    const calls = failing(error);
    await expect(save()).rejects.toEqual(error);
    expect(calls).toHaveLength(1);
    const text = describeApiError(error.kind, "", error);
    expect(text.title).toContain("未対応");
    expect(text.description).toContain("create、update");
    expect(text.description).toContain("別の場所に保存し直すことはしていません");
  });

  it("says the outcome is unknown and does not send again", async () => {
    const error: ApiError = { kind: "outcome_unknown", detail: "timeout", reason: "outcome_unknown" };
    const calls = failing(error);
    await expect(save({ ...formFromRequest(readable), title: "x" })).rejects.toEqual(error);
    expect(calls).toHaveLength(1);
    const text = describeApiError(error.kind);
    expect(text.title).toBe("受け付けられたか分かりません");
    expect(text.description).toContain("自動では送り直しません");
    expect(text.description).toContain("重ねて送らず");
  });

  it("says the service is unavailable, as a refusal that was not accepted", async () => {
    const error: ApiError = { kind: "external_unavailable", detail: "openspec: not connected" };
    failing(error);
    await expect(save()).rejects.toEqual(error);
    const text = describeApiError(error.kind, "", error);
    expect(text.title).toBe("連携先のサービスが使えませんでした");
    expect(text.description).toContain("受け付けられていません");
    expect(text.description).not.toContain("未対応");
  });

  it("gives three different texts", () => {
    const titles = [
      describeApiError("external_unavailable", "", { reason: UNSUPPORTED_BY_OPENSPEC_MCP, missing: ["update"] }).title,
      describeApiError("outcome_unknown").title,
      describeApiError("external_unavailable").title,
    ];
    expect(new Set(titles).size).toBe(3);
  });

  it("names no tool when the server named none", () => {
    expect(describeApiError("external_unavailable", "", { reason: UNSUPPORTED_BY_OPENSPEC_MCP }).description).not.toContain("欠けているツール");
  });
});

describe("where a save goes", () => {
  const info = { version: "1", contentBackend: "sqlite", contentSupported: true, contentMissingTools: [], contentDetail: null };
  it("says the server's own store for sqlite and requirement for openspec-mcp", () => {
    expect(describeSaveTarget(info)).toBe("保存先: サーバーの中");
    expect(describeSaveTarget({ ...info, contentBackend: "requirement" })).toBe("保存先: requirement（openspec-mcp）");
  });
  it("says it cannot be saved, with the missing tools, when openspec-mcp lacks them", () => {
    const text = describeSaveTarget({ ...info, contentBackend: "requirement", contentSupported: false, contentMissingTools: ["create", "update"] });
    expect(text).toContain("未対応のため保存できません");
    expect(text).toContain("create、update");
  });
  it("claims nothing when the server does not say (an older server)", () => {
    expect(describeSaveTarget(null)).toBeNull();
    expect(describeSaveTarget({ version: "0.1" })).toBeNull();
  });
});

describe("describeBodyUnreadable", () => {
  const body = (reason?: string | null, detail = "the server's sentence") => [
    { item: "other", service: "open_spec" as const, kind: "internal" as const, detail: "x", reason: "not_found" },
    { item: "body", service: "open_spec" as const, kind: "external_unavailable" as const, detail, reason },
  ];

  it("says the reason the server gives for the body", () => {
    expect(describeBodyUnreadable(body("change_not_set"))).toBe("change が未記入のため、openspec-mcp から本文を読めません");
    expect(describeBodyUnreadable(body("show_unsupported"))).toBe("接続先の openspec-mcp が、本文を読む操作（show）に未対応です");
    expect(describeBodyUnreadable(body("not_found"))).toBe("openspec-mcp に、この依頼の本文が見つかりません");
    expect(describeBodyUnreadable(body("unavailable"))).toBe("openspec-mcp に届かないため、本文を読めません");
  });

  it("falls back to the plain sentence with the server's detail when the reason is missing or unknown", () => {
    for (const reason of [undefined, null, "", "something_new"]) {
      expect(describeBodyUnreadable(body(reason))).toBe("本文を取り出せません（the server's sentence）");
    }
  });

  it("falls back to the old sentence when there is no body item or no detail", () => {
    expect(describeBodyUnreadable([])).toBe(BODY_UNREADABLE_TEXT);
    expect(describeBodyUnreadable(undefined)).toBe(BODY_UNREADABLE_TEXT);
    expect(describeBodyUnreadable(body("something_new", " "))).toBe(BODY_UNREADABLE_TEXT);
  });
});

describe("what the user emptied is cleared, what was not touched is not sent", () => {
  const saved = formFromRequest({ ...readable, protocol: "proto", tags: ["x", "y"], kind: "task", project: "p" });
  const base = baseFromRequest(readable);
  const sentArgs = async (form: typeof saved) => {
    const calls = nativeSide(() => ({ ...readable, version: 5 }));
    await saveOnce((input) => callApi("saveDraft", input), form, base, saved);
    expect(calls).toHaveLength(1);
    return calls[0]?.args as Record<string, unknown>;
  };

  it("sends an emptied protocol as an empty text, not as null (null means not changed)", async () => {
    const args = await sentArgs({ ...saved, protocol: "" });
    expect(args.protocol).toBe("");
    expect(Object.keys(args).sort()).toEqual(["id", "protocol", "version"]);
    expect((await sentArgs({ ...saved, protocol: "   " })).protocol).toBe("");
  });

  it("sends every emptied item in the contract's empty form", async () => {
    const args = await sentArgs({ ...saved, project: "", kind: "", tags: [], protocol: "" });
    expect(args).toMatchObject({ project: "", kind: "", tags: [], protocol: "" });
    expect(Object.keys(args)).not.toContain("title");
    expect(Object.keys(args)).not.toContain("body");
    expect(Object.keys(args)).not.toContain("change");
  });

  it("leaves out the items that did not change, an empty one included", async () => {
    const emptySaved = { ...saved, protocol: "", tags: [] as string[] };
    const input = toDraftInput({ ...emptySaved, kind: "bug" }, base, undefined, emptySaved);
    expect(input).toEqual({ id: "r1", version: 4, kind: "bug" });
    expect(toDraftInput(saved, base, undefined, saved)).toEqual({ id: "r1", version: 4 });
  });

  it("sends the tags that changed as they are, and notes a tag change that only normalizes as no change", () => {
    expect(toDraftInput({ ...saved, tags: ["x"] }, base, undefined, saved).tags).toEqual(["x"]);
    expect("tags" in toDraftInput({ ...saved, tags: [" x ", "y", "x"] }, base, undefined, saved)).toBe(false);
  });

  it("does not send a protocol for a new request that has none", () => {
    expect(toDraftInput({ ...emptyForm("p"), title: "t" }, null).protocol).toBeNull();
  });
});
