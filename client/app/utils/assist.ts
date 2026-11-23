// The request-writing assistant, as the registration screen sees it. The conversation is held on
// the native side by the user's own `claude` CLI (`assist_send`); this holds the shapes it answers
// with, the lines of the conversation and what an answer does to the form. Pure apart from the call
// it is given.
import type { DraftForm } from "~/utils/requests";

export type NoticeKind = "not_found" | "not_logged_in" | "failed" | "timed_out";

export type AssistOutcome =
  | { state: "answer"; reply: string; title?: string | null; body?: string | null; session_id?: string | null }
  | { state: "notice"; kind: NoticeKind; message: string; detail?: string | null };

export interface ChatLine {
  id: number;
  kind: "user" | "agent" | "change" | "notice";
  text: string;
  /** The CLI's own words, with a notice. */
  detail?: string;
}

/** One conversation: the lines, the session to continue (kept for a later resume), and whether an answer is awaited. */
export interface Chat {
  lines: ChatLine[];
  sessionId: string | null;
  sending: boolean;
  /** Counts the conversations on this screen; an answer to an earlier one is dropped. */
  generation: number;
}

/** What the screen says while the answer is awaited. */
export const WAITING_TEXT = "返事を待っています…（最長 2 分）";
export const TITLE_CHANGED_TEXT = "題名を書き換えました";
export const BODY_CHANGED_TEXT = "本文を書き換えました";

export function newChat(): Chat {
  return { lines: [], sessionId: null, sending: false, generation: 0 };
}

/** A new conversation in place of this one (another request is shown). An answer still awaited is dropped when it comes. */
export function resetChat(chat: Chat): void {
  Object.assign(chat, { lines: [], sessionId: null, sending: false, generation: chat.generation + 1 });
}

/** Whether the send button may be pressed. */
export function canSend(chat: Chat, text: string, enabled: boolean): boolean {
  return enabled && !chat.sending && text.trim() !== "";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Whether what the native side answered has the shape of an outcome. */
export function isAssistOutcome(value: unknown): value is AssistOutcome {
  if (!isRecord(value)) return false;
  if (value.state === "answer") return typeof value.reply === "string";
  return value.state === "notice" && typeof value.message === "string";
}

/** The request as the user has it now: what is handed to the assistant with every message. */
export function draftOf(form: DraftForm): { title: string; body: string; project: string; kind: string } {
  return { title: form.title, body: form.body, project: form.project, kind: form.kind };
}

export type AssistCall = (message: string, draft: ReturnType<typeof draftOf>, sessionId: string | null) => Promise<unknown>;

function push(chat: Chat, kind: ChatLine["kind"], text: string, detail?: string | null) {
  const id = (chat.lines.at(-1)?.id ?? 0) + 1;
  chat.lines.push({ id, kind, text, detail: detail || undefined });
}

/**
 * Sends one message and waits for the answer. The user's line goes in at once; the answer (or the
 * reason there is none) follows. A new title or body from the assistant is put in `form`, with a line
 * saying so; nothing else in the form is touched. Returns false when nothing was sent.
 */
export async function sendMessage(chat: Chat, form: DraftForm, text: string, call: AssistCall): Promise<boolean> {
  const message = text.trim();
  if (message === "" || chat.sending) return false;
  push(chat, "user", message);
  chat.sending = true;
  const generation = chat.generation;
  let outcome: AssistOutcome;
  try {
    // What is on the screen now, before the answer can change it.
    const answer = await call(message, draftOf(form), chat.sessionId);
    outcome = isAssistOutcome(answer) ? answer : { state: "notice", kind: "failed", message: "応答の形が想定と違います。" };
  } catch (error) {
    const detail = typeof error === "string" ? error : isRecord(error) && typeof error.detail === "string" ? error.detail : undefined;
    outcome = { state: "notice", kind: "failed", message: "会話を実行できませんでした。", detail };
  }
  // The screen moved to another request meanwhile: this answer is not for it.
  if (chat.generation !== generation) return true;
  if (outcome.state === "answer") {
    if (typeof outcome.session_id === "string" && outcome.session_id !== "") chat.sessionId = outcome.session_id;
    if (outcome.reply.trim() !== "") push(chat, "agent", outcome.reply.trim());
    if (typeof outcome.title === "string" && outcome.title !== form.title) {
      form.title = outcome.title;
      push(chat, "change", TITLE_CHANGED_TEXT);
    }
    if (typeof outcome.body === "string" && outcome.body !== form.body) {
      form.body = outcome.body;
      push(chat, "change", BODY_CHANGED_TEXT);
    }
  } else {
    push(chat, "notice", outcome.message, outcome.detail);
  }
  chat.sending = false;
  return true;
}
