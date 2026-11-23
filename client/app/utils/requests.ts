// The request screens' pure parts: the draft form, what the list shows, what the status shows.
// Nothing here talks to the server; the screens call these and the API separately.
import type { DraftInput, HistoryEntry, LaneDef, MissingSource, Request, RequestDetail, ServerInfo, Step } from "~/types/binone";

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
  /**
   * The saved body could not be read, and this screen has not written one since: the form's body
   * is not the saved one, so it is never sent (an empty text would replace the saved body).
   */
  bodyUnreadable?: boolean;
}

/** The body could not be read (`body` is `null`, or the server says `bodyMissing`). */
export function isBodyUnreadable(request: Pick<Request, "body" | "bodyMissing">): boolean {
  return request.body === null || request.bodyMissing === true;
}

/** The same for the status screen's record (snake_case), whose missing source also names the item. */
export function isDetailBodyUnreadable(detail: Pick<RequestDetail, "request" | "missing_sources">): boolean {
  return (
    detail.request.body === null ||
    detail.request.body_missing === true ||
    detail.missing_sources.some((s) => s.item === "body")
  );
}

/** What is shown in place of a body that cannot be read, when no reason is known (not an empty body). */
export const BODY_UNREADABLE_TEXT = "本文を取り出せません（openspec-mcp に本文を読む口がありません）";

const BODY_REASON_TEXT: Record<string, string> = {
  change_not_set: "change が未記入のため、openspec-mcp から本文を読めません",
  show_unsupported: "接続先の openspec-mcp が、本文を読む操作（show）に未対応です",
  not_found: "openspec-mcp に、この依頼の本文が見つかりません",
  unavailable: "openspec-mcp に届かないため、本文を読めません",
};

/**
 * The sentence for a body that cannot be read, by the reason the server gives on the `body` item of
 * `missing_sources`. No reason, or one not known here: "本文を取り出せません" with the server's `detail`
 * (the old sentence when there is no `detail` either).
 */
export function describeBodyUnreadable(sources: readonly MissingSource[] | null | undefined): string {
  const source = (sources ?? []).find((s) => s.item === "body");
  const known = source?.reason ? BODY_REASON_TEXT[source.reason] : undefined;
  if (known) return known;
  const detail = source?.detail?.trim();
  return detail ? `本文を取り出せません（${detail}）` : BODY_UNREADABLE_TEXT;
}

export function emptyForm(project = ""): DraftForm {
  return { title: "", body: "", project, kind: "", tags: [], protocol: "" };
}

export function formFromRequest(request: Request): DraftForm {
  return {
    title: request.title,
    body: request.body ?? "",
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
 *
 * An update sends only the title and the body that changed since `saved` (the server leaves out
 * what it is not given), and never a body that could not be read. Without `saved` every item that
 * may be sent is sent. A blank title is not sent on an update.
 */
export function toDraftInput(form: DraftForm, base: DraftBase | null, versionOverride?: number, saved?: DraftForm): DraftInput {
  const protocol = form.protocol.trim();
  const input: DraftInput = {
    project: form.project,
    kind: form.kind,
    tags: normalizeTags(form.tags),
    protocol: protocol === "" ? null : protocol,
  };
  if (!base) {
    input.title = form.title;
    input.body = form.body;
    return input;
  }
  input.id = base.id;
  input.version = versionOverride ?? base.version;
  input.change = base.change;
  if (form.title.trim() !== "" && (!saved || form.title !== saved.title)) input.title = form.title;
  if (!base.bodyUnreadable && (!saved || form.body !== saved.body)) input.body = form.body;
  return input;
}

/**
 * What the screen holds after a save succeeded. The answer to a save with an item left out does
 * not carry that item (title empty, body missing), so the item left out keeps what the screen had.
 */
export function afterSave(
  form: DraftForm,
  saved: DraftForm,
  input: DraftInput,
  result: Request,
  previous: DraftBase | null,
): { form: DraftForm; saved: DraftForm; base: DraftBase } {
  const fromServer = formFromRequest(result);
  const titleSent = input.title !== undefined;
  const bodySent = input.body !== undefined;
  const nextSaved: DraftForm = {
    ...fromServer,
    title: titleSent ? input.title ?? "" : saved.title,
    body: bodySent ? input.body ?? "" : saved.body,
  };
  return {
    // What the user typed in an item that was not sent stays on screen (and stays "changed").
    form: { ...nextSaved, title: titleSent ? nextSaved.title : form.title, body: bodySent ? nextSaved.body : form.body },
    saved: nextSaved,
    base: {
      id: result.id,
      version: result.version,
      change: result.change,
      // Unreadable stays so unless this save wrote a body (the answer says "missing" when it sent none,
      // also for a body this screen knows, so the answer is not what decides).
      bodyUnreadable: bodySent ? false : (previous?.bodyUnreadable ?? false),
    },
  };
}

export function baseFromRequest(request: Request): DraftBase {
  return { id: request.id, version: request.version, change: request.change, bodyUnreadable: isBodyUnreadable(request) };
}

/**
 * One save: builds what is sent, sends it once and returns what the screen holds afterwards. It
 * does not try again; a failure (an unknown outcome included) is thrown to the caller as it is.
 */
export async function saveOnce(
  send: (input: DraftInput) => Promise<Request>,
  form: DraftForm,
  base: DraftBase | null,
  saved: DraftForm,
  versionOverride?: number,
): Promise<{ request: Request; form: DraftForm; saved: DraftForm; base: DraftBase }> {
  const input = toDraftInput(form, base, versionOverride, saved);
  const request = await send(input);
  return { request, ...afterSave(form, saved, input, request, base) };
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
export function submitProblems(form: DraftForm, base: Pick<DraftBase, "bodyUnreadable"> | null = null): string[] {
  const problems: string[] = [];
  if (form.title.trim() === "") problems.push("タイトルを入れてください");
  if (form.project.trim() === "") problems.push("プロジェクトを選んでください");
  if (form.kind.trim() === "") problems.push("種別を選んでください");
  // A body that cannot be read is not known to be empty; the server judges it.
  if (!base?.bodyUnreadable && form.body.trim() === "") problems.push("本文を入れてください");
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

/**
 * Where a save goes, in a short line for the edge of the edit screen. `null` when the server did
 * not say (an older server has no such field): then nothing is claimed.
 */
export function describeSaveTarget(info: Partial<ServerInfo> | null): string | null {
  if (!info || typeof info.contentBackend !== "string") return null;
  if (info.contentBackend === "requirement") {
    if (info.contentSupported === false) {
      const missing = info.contentMissingTools ?? [];
      return `保存先: requirement（openspec-mcp）— 未対応のため保存できません${missing.length ? `（欠けているツール: ${missing.join("、")}）` : ""}`;
    }
    return "保存先: requirement（openspec-mcp）";
  }
  return info.contentBackend === "sqlite" ? "保存先: サーバーの中" : `保存先: ${info.contentBackend}`;
}
