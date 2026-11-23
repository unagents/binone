// The flow that turns the grabbed drafts into todos. Pure: the calls to the server and the
// questions to the user come in as functions, so the order of events can be tested.
//
// Rules (requirements "スイムレーン"):
// - the grabbed drafts are checked one at a time, in the grabbed order, each by a dry run that
//   changes nothing (`checkSubmitRequest`); the title of the one being checked is shown;
// - a draft that does not pass stops the flow, with its reason (code and missing fields). The user
//   fixes it elsewhere, then checks again, or aborts. There is no "skip this one";
// - only when every draft has passed is `submitRequests` called, once, for all of them. Until then
//   nothing was changed: closing the modal leaves every draft a draft;
// - once that call is made the modal cannot be closed until its answer is known. The answer is a
//   success, a refusal (nothing changed), a failure, or an unknown outcome (never sent again by
//   this code); the board is read again after it.
// - checking again reads the list again first, so a version that moved on (the user edited the
//   draft meanwhile) is taken, not reported as a conflict.
import type { ApiError } from "~/utils/api/errors";
import type { SubmitBatch, SubmitCheck, SubmitCode } from "~/types/binone";

/** A grabbed draft with the version read last. */
export interface BatchItem {
  id: string;
  title: string;
  version: number;
}

/** What stops the flow: the server's refusal of one draft, or a call that failed. */
export type StopProblem =
  | { kind: "refused"; item: BatchItem; check: SubmitCheck }
  /** The batch failed after something was sent to openspec-mcp: the causes and what became of the others. */
  | { kind: "batch"; failure: BatchFailure }
  | { kind: "error"; item: BatchItem | null; error: ApiError };

export type StopChoice = "retry" | "abort";

export interface SubmitFlowHooks {
  /** The dry run of one draft. Rejects with an `ApiError` (or anything: shown as `internal`). */
  check: (item: BatchItem) => Promise<SubmitCheck>;
  /** The one call for all of them. */
  submit: (items: BatchItem[]) => Promise<SubmitBatch>;
  /** The drafts as they are on the server now (the versions, the titles). A draft not found is left as it was. */
  reread: (items: readonly BatchItem[]) => Promise<BatchItem[]>;
  /** The draft now being checked (1-based position of `total`). */
  onChecking?: (item: BatchItem, position: number, total: number) => void;
  /** Called right before `submit`, once the modal can no longer be closed. */
  onSubmitting?: () => void;
  /** The flow is stopped. Resolves with the user's choice (a closed modal resolves "abort"). */
  onStop: (problem: StopProblem) => Promise<StopChoice>;
  /** True once the user closed the modal. Read before each check, after each answer and before `submit`. */
  isClosed: () => boolean;
}

export type SubmitFlowOutcome =
  /** Every draft became a todo. */
  | { status: "submitted"; batch: SubmitBatch }
  /** The modal was closed or the user aborted before `submit` was called: nothing changed. */
  | { status: "left" }
  /** `submit` was called and its answer is not a success: a refusal is nothing changed, an error may be more. */
  | { status: "failed"; error: ApiError }
  | { status: "unknown"; error: ApiError };

export function asApiError(error: unknown): ApiError {
  if (typeof error === "object" && error !== null && "kind" in error && "detail" in error) return error as ApiError;
  return { kind: "internal", detail: String(error) };
}

/** What became of a draft that was not the cause of a failed batch. */
export type BatchEffect = "rolled_back" | "rollback_failed" | "not_attempted";

const EFFECT_CODES: readonly string[] = ["rolled_back", "rollback_failed", "not_attempted"];
/** The codes of a failed batch of requirements; the others are the dry run's refusals. */
const SENT_CODES: readonly string[] = ["content_update_failed", ...EFFECT_CODES];

export interface BatchFailure {
  /** The drafts that made the batch fail: `content_update_failed` and the ones the check refused. Never a rolled back one. */
  causes: { item: BatchItem; check: SubmitCheck }[];
  /** The drafts that were not the cause. A `rollback_failed` one (or one in `unreverted`) is `rollback_failed`. */
  effects: { item: BatchItem; effect: BatchEffect }[];
  /** The drafts that stayed `open` in openspec-mcp while binone has them as drafts. */
  stuck: BatchItem[];
  /** True only when nothing changed anywhere: no draft stayed `open` in openspec-mcp. */
  nothingChanged: boolean;
}

/**
 * Sorts the results of a failed batch into the causes and the rest. A rolled back, not attempted or
 * rollback-failed draft is never taken for a cause, even when it comes first in the list.
 */
export function analyzeBatchFailure(items: readonly BatchItem[], batch: SubmitBatch): BatchFailure {
  const unreverted = new Set(batch.unreverted ?? []);
  const find = (id: string): BatchItem => items.find((candidate) => candidate.id === id) ?? { id, title: "", version: 0 };
  const causes: BatchFailure["causes"] = [];
  const effects: BatchFailure["effects"] = [];
  const stuckIds = new Set(unreverted);
  for (const check of batch.results) {
    if (check.ok) continue;
    if (check.code !== null && EFFECT_CODES.includes(check.code)) {
      if (check.code === "rollback_failed") stuckIds.add(check.id);
      effects.push({ item: find(check.id), effect: check.code as BatchEffect });
    } else {
      causes.push({ item: find(check.id), check });
    }
  }
  for (const id of unreverted) {
    // An id the server lists as unreverted but did not mark as such is still stuck.
    if (!effects.some(({ item }) => item.id === id) && !causes.some(({ item }) => item.id === id)) effects.push({ item: find(id), effect: "rollback_failed" });
  }
  return { causes, effects, stuck: [...stuckIds].map(find), nothingChanged: stuckIds.size === 0 };
}

/** What the user reads for a draft that was not the cause. */
export function describeEffect(effect: BatchEffect): string {
  switch (effect) {
    case "rolled_back":
      return "draft に戻しました";
    case "rollback_failed":
      return "openspec-mcp の側で open のまま残っています。binone の側は draft のままです";
    case "not_attempted":
      return "送っていません（draft のままです）";
  }
}

/**
 * The draft the editor button opens: the one that stopped the check, the first cause of a failed
 * batch, or (when every result was put back or not sent, so there is no cause row) the first
 * draft the batch names.
 */
export function editTargetOf(problem: StopProblem | null): BatchItem | null {
  if (!problem) return null;
  if (problem.kind === "batch") return problem.failure.causes[0]?.item ?? problem.failure.effects[0]?.item ?? null;
  return problem.item;
}

/** The title as the screens show it. */
export function titleOf(item: BatchItem): string {
  return item.title || "（題名なし）";
}

/** The draft to stop on when the batch failed: what the dry run refuses stays as it was; the rest is a batch failure. */
function batchProblem(items: readonly BatchItem[], batch: SubmitBatch): StopProblem {
  const sent = batch.results.some((result) => !result.ok && result.code !== null && SENT_CODES.includes(result.code)) || (batch.unreverted ?? []).length > 0;
  if (sent) return { kind: "batch", failure: analyzeBatchFailure(items, batch) };
  return firstRefusal(items, batch);
}

/** The first draft the batch refused, with the server's reason. */
function firstRefusal(items: readonly BatchItem[], batch: SubmitBatch): StopProblem {
  const index = batch.results.findIndex((result) => !result.ok);
  const check = batch.results[index];
  const item = items.find((candidate) => candidate.id === check?.id);
  if (check && item) return { kind: "refused", item, check };
  return { kind: "error", item: null, error: { kind: "bad_response", detail: "the server refused the batch without saying which draft" } };
}

export async function runSubmitFlow(grabbed: readonly BatchItem[], hooks: SubmitFlowHooks): Promise<SubmitFlowOutcome> {
  let items = [...grabbed];
  if (items.length === 0) return { status: "left" };
  // A version conflict on the first look is a stale list, not the user's to settle: read and look again.
  let rereadOnConflict = true;
  for (;;) {
    let problem: StopProblem | null = null;
    for (let index = 0; index < items.length && !problem; index++) {
      if (hooks.isClosed()) return { status: "left" };
      let item = items[index]!;
      hooks.onChecking?.(item, index + 1, items.length);
      try {
        let check = await hooks.check(item);
        if (!check.ok && check.code === "version_conflict" && rereadOnConflict) {
          rereadOnConflict = false;
          items = await hooks.reread(items);
          item = items[index]!;
          check = await hooks.check(item);
        }
        if (!check.ok) problem = { kind: "refused", item, check };
      } catch (error) {
        problem = { kind: "error", item, error: asApiError(error) };
      }
    }
    if (!problem) {
      // The last look before the call: a modal closed up to here changes nothing.
      if (hooks.isClosed()) return { status: "left" };
      hooks.onSubmitting?.();
      try {
        const batch = await hooks.submit(items);
        if (batch.ok) return { status: "submitted", batch };
        problem = batchProblem(items, batch);
      } catch (error) {
        const apiError = asApiError(error);
        return apiError.kind === "outcome_unknown" ? { status: "unknown", error: apiError } : { status: "failed", error: apiError };
      }
    }
    const choice = hooks.isClosed() ? "abort" : await hooks.onStop(problem);
    if (choice === "abort" || hooks.isClosed()) return { status: "left" };
    rereadOnConflict = true;
    try {
      items = await hooks.reread(items);
    } catch (error) {
      // Could not read again: stay where we are and ask once more, with the reason.
      const again = await hooks.onStop({ kind: "error", item: null, error: asApiError(error) });
      if (again === "abort" || hooks.isClosed()) return { status: "left" };
    }
  }
}

const CODE_TEXT: Record<SubmitCode, string> = {
  not_found: "依頼が見つかりません（消えたか、番号が違います）",
  not_draft: "もう draft ではありません（他で todo などに変わりました）",
  version_conflict: "確かめた後に、他で更新されました（版が変わりました）",
  incomplete: "todo にするには足りない項目があります",
  content_update_failed: "openspec-mcp が open への更新を受け付けませんでした",
  rolled_back: describeEffect("rolled_back"),
  rollback_failed: describeEffect("rollback_failed"),
  not_attempted: describeEffect("not_attempted"),
};

/** What the user reads for a refusal's code. An unknown code is shown as it is. */
export function describeSubmitCode(code: string | null): string {
  if (code !== null && code in CODE_TEXT) return CODE_TEXT[code as SubmitCode];
  return code ?? "理由は不明です";
}

const MISSING_TEXT: Record<string, string> = { title: "タイトル", project: "プロジェクト", protocol: "protocol" };

/** The names of the missing fields as the screens call them; an unknown name is shown as it is. */
export function describeMissing(missing: readonly string[]): string[] {
  return missing.map((name) => MISSING_TEXT[name] ?? name);
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
