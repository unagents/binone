// The request screens' pure parts: the draft form, what the list shows, what the status shows.
// Nothing here talks to the server; the screens call these and the API separately.
import type { DraftInput, HistoryEntry, LaneDef, MissingSource, Request, RequestDetail, Step } from "~/types/binone";

/** The editable fields of a draft, as the form holds them. */
export interface DraftForm {
  title: string;
  body: string;
  project: string;
  kind: string;
  tags: string[];
  protocol: string;
}

/** The request as read: the form's starting point, the version to send back and the `change`. */
export interface DraftBase {
  id: string;
  version: number;
  /** Not editable here; sent back unchanged so saving does not blank what the agent set. */
  change: string | null;
}

export function emptyForm(project = ""): DraftForm {
  return { title: "", body: "", project, kind: "", tags: [], protocol: "" };
}

export function formFromRequest(request: Request): DraftForm {
  return {
    title: request.title,
    body: request.body,
    project: request.project,
    kind: request.kind,
    tags: [...request.tags],
    protocol: request.protocol ?? "",
  };
}

/** Tags are trimmed, empty ones dropped, repeats kept once in first-seen order. */
export function normalizeTags(tags: readonly string[]): string[] {
  const seen: string[] = [];
  for (const tag of tags) {
    const trimmed = tag.trim();
    if (trimmed !== "" && !seen.includes(trimmed)) seen.push(trimmed);
  }
  return seen;
}

/** Adds the typed tag to `tags` (unchanged when it is empty or already there). */
export function addTag(tags: readonly string[], typed: string): string[] {
  return normalizeTags([...tags, typed]);
}

/**
 * What `saveDraft` takes. A new request has no `base`; an update sends the version it read, so a
 * request that moved on in between is a conflict rather than being overwritten silently.
 */
export function toDraftInput(form: DraftForm, base: DraftBase | null, versionOverride?: number): DraftInput {
  const protocol = form.protocol.trim();
  const input: DraftInput = {
    title: form.title,
    body: form.body,
    project: form.project,
    kind: form.kind,
    tags: normalizeTags(form.tags),
    protocol: protocol === "" ? null : protocol,
  };
  if (base) {
    input.id = base.id;
    input.version = versionOverride ?? base.version;
    input.change = base.change;
  }
  return input;
}

export function sameForm(a: DraftForm, b: DraftForm): boolean {
  return (
    a.title === b.title &&
    a.body === b.body &&
    a.project === b.project &&
    a.kind === b.kind &&
    a.protocol === b.protocol &&
    JSON.stringify(normalizeTags(a.tags)) === JSON.stringify(normalizeTags(b.tags))
  );
}

/** What must be filled before a draft becomes a todo. The server checks again. */
export function submitProblems(form: DraftForm): string[] {
  const problems: string[] = [];
  if (form.title.trim() === "") problems.push("タイトルを入れてください");
  if (form.project.trim() === "") problems.push("プロジェクトを選んでください");
  if (form.kind.trim() === "") problems.push("種別を選んでください");
  if (form.body.trim() === "") problems.push("本文を入れてください");
  return problems;
}

/** The lane as the user reads it: the configured label when there is one, else the name. */
export function laneText(lane: string, lanes: readonly LaneDef[] | null | undefined): string {
  const def = lanes?.find((l) => l.name === lane);
  return def?.label && def.label !== "" ? def.label : lane;
}

const LANE_COLORS: Record<string, "neutral" | "info" | "warning" | "primary" | "success"> = {
  draft: "neutral",
  todo: "info",
  prepare: "warning",
  doing: "primary",
  done: "success",
  archive: "neutral",
};

export function laneColor(lane: string) {
  return LANE_COLORS[lane] ?? "neutral";
}

/** "5 分前" / "3 時間前" / "2 日前"; older than a week: the date. */
export function relativeTime(ms: number, now: number): string {
  const diff = Math.max(0, now - ms);
  const minute = 60_000;
  if (diff < minute) return "たった今";
  if (diff < 60 * minute) return `${Math.floor(diff / minute)} 分前`;
  if (diff < 24 * 60 * minute) return `${Math.floor(diff / (60 * minute))} 時間前`;
  if (diff < 7 * 24 * 60 * minute) return `${Math.floor(diff / (24 * 60 * minute))} 日前`;
  return formatDateTime(ms);
}

export function formatDateTime(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const SERVICE_NAMES: Record<string, string> = { bantou: "bantou", open_spec: "openspec", memory: "memory" };

/**
 * Sources the server could not read. They are shown by name and cause, never as an empty
 * history: "nothing happened" and "could not look" are different.
 */
export function describeMissingSource(source: MissingSource): { name: string; reason: string; detail: string } {
  const reason =
    source.kind === "external_unavailable"
      ? "未接続（届きません）"
      : source.kind === "invalid_input" || source.kind === "not_found"
        ? "未対応（サーバーがこの情報を返せません）"
        : source.kind === "external_outcome_unknown"
          ? "結果が分かりません"
          : "読み出しに失敗しました";
  return { name: SERVICE_NAMES[source.service] ?? source.service, reason, detail: source.detail };
}

const SOURCE_NAMES: Record<string, string> = { binone: "binone", bantou: "bantou", openspec: "openspec" };

export function historySourceName(entry: HistoryEntry): string {
  return SOURCE_NAMES[entry.source] ?? entry.source;
}

/** Newest first, whatever order the server sent (a stable sort on the time). */
export function newestFirst(history: readonly HistoryEntry[]): HistoryEntry[] {
  return history
    .map((entry, index) => ({ entry, index }))
    .sort((a, b) => b.entry.at_ms - a.entry.at_ms || a.index - b.index)
    .map(({ entry }) => entry);
}

/** The step the request is at: the first active one, else the first pending one; none if there are no steps. */
export function currentStep(steps: readonly Step[]): Step | null {
  const ordered = [...steps].sort((a, b) => a.position - b.position);
  return ordered.find((s) => s.status === "active") ?? ordered.find((s) => s.status === "pending") ?? null;
}

export function stepStatusText(status: Step["status"]): string {
  return status === "done" ? "完了" : status === "active" ? "進行中" : "これから";
}

/** The server allows editing as a draft only while it says so. */
export function canEditDraft(detail: Pick<RequestDetail, "allowed_operations">): boolean {
  return detail.allowed_operations.includes("save_draft");
}

/**
 * The missing sources that hold artifacts. Artifacts live in openspec, so that is the one that
 * counts; when it is missing the artifact area must not read as "none yet".
 */
export function missingArtifactSources(sources: readonly MissingSource[]): MissingSource[] {
  return sources.filter((s) => s.service === "open_spec");
}
