// A stand-in for the native side with sample data, for drawing the screens where Tauri's
// `invoke` does not exist (a headless browser). It is switched on only by building with
// `NUXT_PUBLIC_SAMPLE_DATA=1` (see plugins/sample.client.ts); a normal build never loads it.
// It follows the same contract as the native side: operation name + arguments in, a value or an
// `ApiError` out, and it refuses a name that is not in the allowlist.
import { OPERATION_NAMES, type OperationName } from "~/utils/api/operations";
import type { ApiError, ApiErrorKind } from "~/utils/api/errors";
import type {
  LaneDef,
  Notification,
  Request,
  RequestDetail,
  RequestFilter,
  RequestSummary,
  SettingsView,
  SubmitCheck,
} from "~/types/binone";
import type { NativeCall } from "~/utils/api/transport";
import { MANUAL_COMMAND, type UpdateCheck, type UpdateOutcome } from "~/utils/update";

const MINUTE = 60_000;

function fail(kind: ApiErrorKind, detail: string): never {
  throw { kind, detail } satisfies ApiError;
}

function lanes(): LaneDef[] {
  return [
    { name: "draft", label: "下書き" },
    { name: "todo", label: "todo" },
    { name: "prepare", label: "準備" },
    { name: "doing", label: "作業中" },
    { name: "done", label: "完了" },
    { name: "archive", label: "保管" },
  ];
}

export interface SampleOptions {
  /** Every operation but `serverInfo` fails with this kind (to see the failure screens). */
  failWith?: ApiErrorKind;
  now?: number;
  /** What the version check finds (`?sample-update=latest|available|manual|unknown`). */
  update?: SampleUpdate;
}

export type SampleUpdate = "latest" | "available" | "manual" | "unknown";
export const SAMPLE_UPDATES: readonly SampleUpdate[] = ["latest", "available", "manual", "unknown"];

function sampleCheck(kind: SampleUpdate): UpdateCheck {
  const current = "0.2.0";
  if (kind === "available") return { state: "available", current, latest: "0.3.0", method: { kind: "brew" } };
  if (kind === "manual") return { state: "available", current, latest: "0.3.0", method: { kind: "manual", reason: "no_brew" } };
  if (kind === "unknown") return { state: "unknown", current, reason: "GitHub に断られました（回数の上限の可能性があります。HTTP 403）" };
  return { state: "up_to_date", current, latest: current };
}

export function createSampleServer(options: SampleOptions = {}): NativeCall {
  const now = options.now ?? Date.now();
  let nextId = 100;
  const projects = ["binone", "bantou", "memory"];
  const requests: Request[] = [
    sample("req_1", "依頼の一覧の見た目を整える", "binone", "feature", ["ui"], "doing", now - 5 * MINUTE),
    sample("req_2", "通知の帯の表示時間を決める", "binone", "task", [], "todo", now - 60 * MINUTE),
    sample("req_3", "memory の検索を速くする", "memory", "feature", ["perf", "search"], "prepare", now - 3 * 60 * MINUTE),
    sample("req_4", "設定の画面の文言", "binone", "task", ["ui"], "draft", now - 26 * 60 * MINUTE),
    // The body is kept in openspec-mcp, which cannot show it: the screens say so, with the reason.
    { ...sample("req_5", "bantou の接続を見直す", "bantou", "feature", [], "done", now - 3 * 24 * 60 * MINUTE), body: null, bodyMissing: true },
  ];
  const settings: SettingsView = {
    settings: { lanes: lanes(), kinds: ["feature", "task", "bug"], scheduler_interval_secs: 30, bantou_endpoint: null, memory_endpoint: null },
    allowed_ranges: ["192.168.0.0/24"],
    allowed_origins: ["tauri://localhost"],
    listen_port: 47610,
  };
  const notifications: Notification[] = [
    { id: 1, source_session: "worker-1", summary: "どの案で進めますか", request_id: "req_1", question_id: "q_1", at_ms: now - 2 * MINUTE },
    { id: 2, source_session: null, summary: "定期処理が終わりました", request_id: null, question_id: null, at_ms: now - 40 * MINUTE },
  ];

  function summary(r: Request): RequestSummary {
    return { id: r.id, version: r.version, title: r.title, project: r.project, kind: r.kind, tags: r.tags, lane: r.lane, currentStep: r.lane === "doing" ? "実装" : null, updatedMs: r.updatedMs };
  }

  function find(id: string): Request {
    return requests.find((r) => r.id === id) ?? fail("not_found", `request ${id}`);
  }

  function filtered(filter: RequestFilter | null | undefined): RequestSummary[] {
    const f = filter ?? {};
    let rows = requests.filter(
      (r) =>
        (!f.project || r.project === f.project) &&
        (!f.kinds?.length || f.kinds.includes(r.kind)) &&
        (!f.tags?.length || r.tags.some((t) => f.tags?.includes(t))),
    );
    const order = f.orderBy ?? "UPDATED_DESC";
    rows = [...rows].sort((a, b) =>
      order === "UPDATED_ASC" ? a.updatedMs - b.updatedMs : order === "PROJECT_ASC" ? a.project.localeCompare(b.project) : order === "PROJECT_DESC" ? b.project.localeCompare(a.project) : b.updatedMs - a.updatedMs,
    );
    return rows.map(summary);
  }

  function detail(r: Request): RequestDetail {
    return {
      request: { id: r.id, title: r.title, project: r.project, kind: r.kind, tags: r.tags, protocol: r.protocol, change_ref: r.change, lane: r.lane, body: r.bodyMissing ? null : r.body, body_missing: r.bodyMissing, version: r.version, created_ms: r.createdMs, updated_ms: r.updatedMs },
      lane: r.lane,
      steps: [],
      waits: [],
      intakes: [],
      allowed_operations: r.lane === "draft" ? ["save_draft", "submit_request"] : [],
      history: [{ source: "binone", at_ms: r.createdMs, summary: "登録" }],
      missing_sources: r.bodyMissing
        ? [{ item: "body", service: "open_spec", kind: "external_unavailable", detail: "openspec-mcp has no tool that reads the body of a requirement", reason: "show_unsupported" }]
        : [],
      external_notes: [],
    };
  }

  /** What the server's judgment says about a draft (title, project, protocol must be set). */
  function judge(id: string, version: unknown): SubmitCheck {
    const refuse = (code: NonNullable<SubmitCheck["code"]>, missing: string[] = [], message: string = code): SubmitCheck => ({ id, ok: false, code, missing, message });
    const current = requests.find((r) => r.id === id);
    if (!current) return refuse("not_found");
    if (current.lane !== "draft") return refuse("not_draft");
    if (current.version !== version) return refuse("version_conflict");
    const missing = [current.title.trim() === "" ? "title" : "", current.project.trim() === "" ? "project" : "", current.protocol ? "" : "protocol"].filter(Boolean);
    return missing.length > 0 ? refuse("incomplete", missing, `request ${id} cannot become a todo yet: missing ${missing.join(", ")}`) : { id, ok: true, code: null, missing: [], message: "" };
  }

  const operations: Record<OperationName, (args: Record<string, unknown>) => unknown> = {
    serverInfo: () => ({ version: "sample", contentBackend: "sqlite", contentSupported: true, contentMissingTools: [], contentDetail: null }),
    requests: (a) => filtered(a.filter as RequestFilter | null | undefined),
    request: (a) => find(String(a.id)),
    requestDetail: (a) => detail(find(String(a.id))),
    artifact: () => fail("not_found", "no artifact in the sample data"),
    conversation: () => null,
    notifications: (a) => notifications.slice(0, typeof a.limit === "number" ? a.limit : undefined),
    projects: () => [...projects],
    settings: () => structuredClone(settings),
    agentMcpConfig: () => ({ path: "/mcp", transport: "http", port: 47610, auth: "bearer" }),
    saveDraft: (a) => {
      const id = typeof a.id === "string" ? a.id : null;
      if (id === null) {
        const created = sample(`req_${nextId++}`, String(a.title ?? ""), String(a.project ?? ""), String(a.kind ?? ""), (a.tags as string[] | undefined) ?? [], "draft", Date.now());
        created.body = String(a.body ?? "");
        created.protocol = typeof a.protocol === "string" && a.protocol !== "" ? a.protocol : null;
        requests.push(created);
        return created;
      }
      const current = find(id);
      if (current.version !== a.version) fail("conflict", "version moved on");
      // A title or a body that is not given is left as it is.
      Object.assign(current, { title: typeof a.title === "string" ? a.title : current.title, body: typeof a.body === "string" ? a.body : current.body, project: String(a.project ?? ""), kind: String(a.kind ?? ""), tags: (a.tags as string[] | undefined) ?? [], protocol: typeof a.protocol === "string" && a.protocol !== "" ? a.protocol : null, version: current.version + 1, updatedMs: Date.now() });
      return current;
    },
    submitRequest: (a) => {
      const current = find(String(a.id));
      if (current.version !== a.version) fail("conflict", "version moved on");
      if (current.lane !== "draft") fail("conflict", "not a draft");
      Object.assign(current, { lane: "todo", version: current.version + 1, updatedMs: Date.now() });
      return current;
    },
    checkSubmitRequest: (a) => judge(String(a.id), a.version),
    submitRequests: (a) => {
      const items = (a.items as { id: string; version: number }[] | undefined) ?? [];
      if (items.length === 0) fail("invalid_input", "no items");
      const results = items.map((item) => judge(item.id, item.version));
      if (!results.every((r) => r.ok)) return { ok: false, results, requests: [] };
      const done = items.map((item) => {
        const current = find(item.id);
        Object.assign(current, { lane: "todo", version: current.version + 1, updatedMs: Date.now() });
        return current;
      });
      return { ok: true, results, requests: done };
    },
    decideApproval: () => [],
    answerQuestion: () => true,
    createProject: (a) => {
      const name = String(a.name ?? "").trim();
      if (name === "") fail("invalid_input", "empty name");
      if (!projects.includes(name)) projects.push(name);
      return name;
    },
    updateSettings: (a) => {
      const next = a.settings as SettingsView["settings"];
      if (next.lanes[0]?.name !== "draft") fail("invalid_input", "the first lane must be `draft`");
      settings.settings = structuredClone(next);
      return structuredClone(settings);
    },
    registerConversation: () => true,
  };

  return async (command, args) => {
    switch (command) {
      case "default_port":
        return 47610;
      case "restore":
        return { state: "connected", host: "127.0.0.1", port: null, version: "sample" };
      case "connect":
        return { host: String(args?.host ?? "127.0.0.1"), port: (args?.port as number | null) ?? null, version: "sample" };
      case "check_update":
        return sampleCheck(options.update ?? "latest");
      case "run_update":
        // With Homebrew in the sample (`available`), an update that went through but could not refresh the tap.
        if (options.update === "available") {
          return { state: "finished", ok: true, code: 0, step: "brew upgrade --cask binone", output: "==> Upgrading 1 outdated package\nbinone 0.2.0 -> 0.3.0", timed_out: false, note: "tap unagents/tap は更新できませんでした（git pull が終了 code 1 で失敗しました）。" } satisfies UpdateOutcome;
        }
        // Without Homebrew in the sample, the screen shows the manual way.
        return { state: "manual", reason: "no_brew", command: MANUAL_COMMAND, url: "https://github.com/unagents/binone/releases/latest" } satisfies UpdateOutcome;
      case "restart_app":
      case "open_release_page":
        return null;
      case "api_call": {
        const op = String(args?.op);
        if (!(OPERATION_NAMES as readonly string[]).includes(op)) fail("invalid_input", "unknown operation");
        if (options.failWith && op !== "serverInfo") fail(options.failWith, "sample failure");
        const raw = args?.args;
        const handler = operations[op as OperationName];
        // A copy, as the real answer is: what the screen holds must not change behind its back.
        return structuredClone(handler((typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>));
      }
      default:
        return fail("internal", `unknown command ${command}`);
    }
  };
}

function sample(id: string, title: string, project: string, kind: string, tags: string[], lane: string, updatedMs: number): Request {
  return { id, version: 1, title, body: "", bodyMissing: false, project, kind, tags, protocol: null, change: null, lane, createdMs: updatedMs - 60_000, updatedMs };
}
