// The batch that turns the grabbed drafts into todos, one by one. Pure: the calls to the server
// and the questions to the user come in as functions, so the order of events can be tested.
//
// Rules (design §4, requirements "スイムレーン"):
// - one draft at a time, in the grabbed order, each by its own call;
// - a draft the server refuses stops the batch; the user then skips that one or aborts;
// - closing the modal stops the batch: drafts not yet handled stay drafts, and what already
//   became a todo is not undone;
// - a call whose outcome is unknown is never sent again by this code.
import type { ApiError } from "~/utils/api/errors";

export interface BatchItem {
  id: string;
  title: string;
}

export type BatchStatus = "todo" | "skipped" | "unknown" | "left";

export interface BatchEntry extends BatchItem {
  status: BatchStatus;
  /** Why it was refused or what is unknown. */
  reason?: ApiError;
}

export type StopChoice = "skip" | "abort";

export interface BatchHooks {
  /** Turns one draft into a todo. Rejects with an `ApiError` (or anything: shown as `internal`). */
  submit: (item: BatchItem) => Promise<void>;
  /** Called with the draft now being handled. */
  onStart?: (item: BatchItem, index: number) => void;
  /** Called when a draft is refused. Resolves with the user's choice (a closed modal resolves "abort"). */
  onStop: (item: BatchItem, error: ApiError) => Promise<StopChoice>;
  /** True once the user closed the modal. Read before each draft and after each answer. */
  isClosed: () => boolean;
}

export interface BatchResult {
  entries: BatchEntry[];
  /** True when the batch ended before every draft was handled (abort or closed). */
  interrupted: boolean;
}

function asApiError(error: unknown): ApiError {
  if (typeof error === "object" && error !== null && "kind" in error && "detail" in error) return error as ApiError;
  return { kind: "internal", detail: String(error) };
}

export async function runSubmitBatch(items: readonly BatchItem[], hooks: BatchHooks): Promise<BatchResult> {
  const entries: BatchEntry[] = items.map((item) => ({ ...item, status: "left" }));
  let interrupted = false;
  for (let index = 0; index < items.length; index++) {
    const item = items[index]!;
    const entry = entries[index]!;
    if (hooks.isClosed()) {
      interrupted = true;
      break;
    }
    hooks.onStart?.(item, index);
    try {
      await hooks.submit(item);
      entry.status = "todo";
      continue;
    } catch (error) {
      const apiError = asApiError(error);
      entry.reason = apiError;
      entry.status = apiError.kind === "outcome_unknown" ? "unknown" : "left";
    }
    // A draft that did not become a todo here: the user decides. A closed modal is an abort.
    const choice = hooks.isClosed() ? "abort" : await hooks.onStop(item, entry.reason!);
    if (choice === "skip" && !hooks.isClosed()) {
      if (entry.status === "left") entry.status = "skipped";
      continue;
    }
    interrupted = true;
    break;
  }
  return { entries, interrupted };
}

export function summarizeBatch(entries: readonly BatchEntry[]) {
  const count = (status: BatchStatus) => entries.filter((entry) => entry.status === status).length;
  return { todo: count("todo"), skipped: count("skipped"), unknown: count("unknown"), left: count("left") };
}

/** Whether a drop of these cards on this lane is allowed: only drafts, only onto the todo lane. */
export function canDrop(cardLanes: readonly string[], targetLane: string): boolean {
  return targetLane === "todo" && cardLanes.length > 0 && cardLanes.every((lane) => lane === "draft");
}

/**
 * The cards a drag takes: the selected ones when the grabbed card is among them, else only the
 * grabbed card. Order follows the order on screen.
 */
export function grabbedIds(selected: readonly string[], grabbed: string, screenOrder: readonly string[]): string[] {
  const ids = selected.includes(grabbed) ? selected : [grabbed];
  return screenOrder.filter((id) => ids.includes(id));
}

/** Click selection: with Shift the card is toggled in the selection; without, it opens. */
export function toggleSelected(selected: readonly string[], id: string): string[] {
  return selected.includes(id) ? selected.filter((value) => value !== id) : [...selected, id];
}
