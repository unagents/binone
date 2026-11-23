import { describe, expect, it } from "vitest";
import { BODY_CHANGED_TEXT, TITLE_CHANGED_TEXT, canSend, draftOf, isAssistOutcome, newChat, resetChat, sendMessage, type AssistCall, type AssistOutcome } from "../app/utils/assist";
import { emptyForm } from "../app/utils/requests";
import { createSampleServer } from "../app/utils/api/sample";

const answer = (over: Partial<Extract<AssistOutcome, { state: "answer" }>> = {}): AssistOutcome => ({ state: "answer", reply: "了解です", session_id: "s1", ...over });
const form = () => ({ ...emptyForm("p"), title: "元の題名", body: "元の本文", kind: "task" });
const replying = (outcome: unknown): AssistCall => async () => outcome;

describe("sending", () => {
  it("marks the chat as waiting until the answer comes, and the button cannot be used meanwhile", async () => {
    const chat = newChat();
    let release!: (v: unknown) => void;
    const pending = sendMessage(chat, form(), "こんにちは", () => new Promise((r) => (release = r)));
    expect(chat.sending).toBe(true);
    expect(canSend(chat, "次の発言", true)).toBe(false);
    // A second send while waiting is refused.
    expect(await sendMessage(chat, form(), "もう一つ", replying(answer()))).toBe(false);
    expect(chat.lines.map((l) => l.text)).toEqual(["こんにちは"]);
    release(answer());
    await pending;
    expect(chat.sending).toBe(false);
    expect(canSend(chat, "次の発言", true)).toBe(true);
  });

  it("does not send an empty message, or when the screen cannot take one", () => {
    const chat = newChat();
    expect(canSend(chat, "   ", true)).toBe(false);
    expect(canSend(chat, "あ", false)).toBe(false);
  });

  it("hands over the form as it is now, and the session of the earlier answer", async () => {
    const chat = newChat();
    const f = form();
    const calls: unknown[][] = [];
    const call: AssistCall = async (...args) => (calls.push(args), answer());
    await sendMessage(chat, f, "一つ目", call);
    f.title = "利用者が直した題名";
    await sendMessage(chat, f, "二つ目", call);
    expect(calls[0]).toEqual(["一つ目", draftOf(form()), null]);
    expect(calls[1]).toEqual(["二つ目", { title: "利用者が直した題名", body: "元の本文", project: "p", kind: "task" }, "s1"]);
  });
});

describe("the answer", () => {
  it("shows the reply and leaves the form as the user wrote it", async () => {
    const chat = newChat();
    const f = form();
    await sendMessage(chat, f, "どう思う？", replying(answer({ reply: "良いと思います" })));
    expect(chat.lines.map((l) => [l.kind, l.text])).toEqual([["user", "どう思う？"], ["agent", "良いと思います"]]);
    expect(f).toEqual(form());
  });

  it("puts a new title in the form and says so; the body stays", async () => {
    const chat = newChat();
    const f = form();
    await sendMessage(chat, f, "題名を直して", replying(answer({ title: "新しい題名" })));
    expect(f.title).toBe("新しい題名");
    expect(f.body).toBe("元の本文");
    expect(chat.lines.at(-1)).toMatchObject({ kind: "change", text: TITLE_CHANGED_TEXT });
    expect(chat.lines.filter((l) => l.kind === "change")).toHaveLength(1);
  });

  it("puts a new body in the form and says so; both at once give both lines", async () => {
    const chat = newChat();
    const f = form();
    await sendMessage(chat, f, "全部直して", replying(answer({ title: "T", body: "# B" })));
    expect(f).toMatchObject({ title: "T", body: "# B", project: "p", kind: "task" });
    expect(chat.lines.filter((l) => l.kind === "change").map((l) => l.text)).toEqual([TITLE_CHANGED_TEXT, BODY_CHANGED_TEXT]);
  });

  it("a value equal to what is there is no change", async () => {
    const chat = newChat();
    await sendMessage(chat, form(), "そのまま", replying(answer({ title: "元の題名", body: "元の本文" })));
    expect(chat.lines.some((l) => l.kind === "change")).toBe(false);
  });

  it("an answer for a conversation that was dropped is not applied", async () => {
    const chat = newChat();
    const f = form();
    let release!: (v: unknown) => void;
    const pending = sendMessage(chat, f, "題名", () => new Promise((r) => (release = r)));
    resetChat(chat);
    release(answer({ title: "遅れて来た題名" }));
    await pending;
    expect(f.title).toBe("元の題名");
    expect(chat.lines).toEqual([]);
    expect(chat.sessionId).toBeNull();
    expect(chat.sending).toBe(false);
  });
});

describe("guidance", () => {
  it("shows the notice with the CLI's words", async () => {
    const chat = newChat();
    const notice: AssistOutcome = { state: "notice", kind: "not_found", message: "claude の CLI（Claude Code）が見つかりません。入れてください。" };
    await sendMessage(chat, form(), "こんにちは", replying(notice));
    expect(chat.lines.at(-1)).toMatchObject({ kind: "notice", text: notice.state === "notice" ? notice.message : "" });
    expect(chat.sending).toBe(false);
    await sendMessage(chat, form(), "もう一度", replying({ state: "notice", kind: "failed", message: "エラーです。", detail: "API Error: 500" }));
    expect(chat.lines.at(-1)).toMatchObject({ kind: "notice", detail: "API Error: 500" });
  });

  it("a failed call or a strange answer is a notice, not a crash", async () => {
    const chat = newChat();
    await sendMessage(chat, form(), "a", async () => Promise.reject("起動できません"));
    expect(chat.lines.at(-1)).toMatchObject({ kind: "notice", detail: "起動できません" });
    await sendMessage(chat, form(), "b", replying({ oops: 1 }));
    expect(chat.lines.at(-1)).toMatchObject({ kind: "notice", text: "応答の形が想定と違います。" });
    expect(chat.sending).toBe(false);
  });

  it("knows an outcome by its shape", () => {
    expect(isAssistOutcome(answer())).toBe(true);
    expect(isAssistOutcome({ state: "notice", kind: "failed", message: "x" })).toBe(true);
    expect(isAssistOutcome({ state: "answer" })).toBe(false);
    expect(isAssistOutcome(null)).toBe(false);
  });
});

describe("sample data", () => {
  it("answers the conversation without a native side, so the pane does not break", async () => {
    const server = createSampleServer({ now: 0 });
    const chat = newChat();
    const f = form();
    await sendMessage(chat, f, "題名と本文を直して", (message, draft, sessionId) => server("assist_send", { message, draft, sessionId }));
    expect(chat.lines.map((l) => l.kind)).toEqual(["user", "agent", "change", "change"]);
    expect(f.title).toBe("見本の題名");
    expect(chat.sessionId).toBe("sample-session");
  });
});
