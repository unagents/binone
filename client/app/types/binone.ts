// Types of what the server returns, as the webview receives it.
//
// Two shapes meet here, and both are kept as the server sends them:
// - GraphQL objects (`Request`, `RequestSummary`) use camelCase field names;
// - values the server returns as JSON (`RequestDetail`, `SettingsView`, `Notification`, ...)
//   use snake_case, because they are the server's own serialized structs.
// Timestamps are milliseconds since the Unix epoch.

/** A lane is named by a string. The server's standard lanes are in `STANDARD_LANES`. */
export type LaneName = string;

export const STANDARD_LANES = ["draft", "todo", "prepare", "doing", "done", "archive"] as const;

export interface LaneDef {
  name: LaneName;
  label: string | null;
}

/** The editable settings. Replaced as a whole by `updateSettings`. */
export interface Settings {
  lanes: LaneDef[];
  kinds: string[];
  scheduler_interval_secs: number;
  bantou_endpoint: string | null;
  memory_endpoint: string | null;
}

/** The settings plus values that only the server's config file sets (read only here). */
export interface SettingsView {
  settings: Settings;
  allowed_ranges: string[];
  allowed_origins: string[];
  listen_port: number;
}

/** Where the content of a request (title, body, state) is kept: binone's own store or openspec-mcp requirements. */
export type ContentBackend = "sqlite" | "requirement";

export interface ServerInfo {
  version: string;
  contentBackend: ContentBackend | string;
  /** `false`: the place named by `contentBackend` cannot be used, so saving fails (it is not saved elsewhere). */
  contentSupported: boolean;
  /** The tools of openspec-mcp that are lacking (empty when supported). */
  contentMissingTools: string[];
  contentDetail: string | null;
}

/** One request. `version` is the version read; send it back when saving or submitting. */
export interface Request {
  id: string;
  version: number;
  title: string;
  /** `null` when the body cannot be read (`bodyMissing`). Never an empty text standing in for it. */
  body: string | null;
  /** The body lives in openspec-mcp, which has no way to read it. Do not show it as empty, do not send it back. */
  bodyMissing: boolean;
  project: string;
  kind: string;
  tags: string[];
  protocol: string | null;
  change: string | null;
  lane: LaneName;
  createdMs: number;
  updatedMs: number;
}

/** A row of the request list. `lane` is the real lane: folding hidden lanes is the client's job. */
export interface RequestSummary {
  id: string;
  /** The version read: send it back as the `version` of the check and the batch. */
  version: number;
  title: string;
  project: string;
  kind: string;
  tags: string[];
  lane: LaneName;
  currentStep: string | null;
  updatedMs: number;
}

/** Why a draft cannot become a todo. `message` is the server's sentence; `missing` is for `incomplete`. */
export type SubmitCode = "not_found" | "not_draft" | "version_conflict" | "incomplete";

/** The dry run of draft -> todo. A refusal is data (`ok: false`), not an error. */
export interface SubmitCheck {
  id: string;
  ok: boolean;
  code: SubmitCode | null;
  /** The fields still missing (`title`, `project`, `protocol`), only for `incomplete`. */
  missing: string[];
  message: string;
}

/** The batch: all pass and all become todo in one step, or nothing is written (`results` say why). */
export interface SubmitBatch {
  ok: boolean;
  /** One per item, in the order sent. */
  results: SubmitCheck[];
  /** The requests as they are now; only when `ok`. */
  requests: Request[];
  /** Ids that stayed `open` in openspec-mcp after a failed batch (normally empty). */
  unreverted?: string[];
}

export type OrderBy = "UPDATED_DESC" | "UPDATED_ASC" | "PROJECT_ASC" | "PROJECT_DESC";

/** Within `kinds` and within `tags` the match is OR; the server ANDs the fields. */
export interface RequestFilter {
  project?: string | null;
  kinds?: string[] | null;
  tags?: string[] | null;
  orderBy?: OrderBy | null;
}

/**
 * What `saveDraft` takes. No `id` creates; `id` with `version` updates (a stale version conflicts).
 * `title` and `body` left out mean "not changed": a body that was not read is never sent as an empty one.
 */
export interface DraftInput {
  id?: string | null;
  version?: number | null;
  title?: string;
  body?: string;
  project?: string;
  kind?: string;
  tags?: string[];
  protocol?: string | null;
  change?: string | null;
}

/** The server's own request record inside `RequestDetail` (snake_case, `change_ref`). */
export interface RequestRecord {
  id: string;
  title: string;
  project: string;
  kind: string;
  tags: string[];
  protocol: string | null;
  change_ref: string | null;
  lane: LaneName;
  /** `null` when `body_missing`. */
  body: string | null;
  body_missing?: boolean;
  version: number;
  created_ms: number;
  updated_ms: number;
}

export type StepStatus = "pending" | "active" | "done";

export interface Step {
  request_id: string;
  key: string;
  position: number;
  artifact: string | null;
  completion: string | null;
  status: StepStatus;
}

export type WaitKind =
  | { type: "time"; due_ms: number }
  | { type: "approval"; proposal_ref: string; proposal_version: number }
  | { type: "answer"; question_id: string };

export type WaitState = "open" | "approved" | "revision_requested" | "satisfied";

export interface Wait {
  id: number;
  request_id: string;
  step: string;
  run_no: number;
  kind: WaitKind;
  state: WaitState;
  note: string | null;
  created_ms: number;
}

export type IntakeState = "claimed" | "accepted" | "unknown" | "completed";

/** Whether work handed to bantou was accepted. `unknown` is never resent by the server. */
export interface Intake {
  request_id: string;
  step: string;
  run_no: number;
  state: IntakeState;
  detail: string | null;
  updated_ms: number;
}

/** What the request allows right now. */
export type AllowedOperation = "save_draft" | "submit_request" | "decide_approval" | "answer_question";

export interface HistoryEntry {
  /** `binone` (the user's operations), `bantou` or `openspec`. */
  source: string;
  at_ms: number;
  summary: string;
}

export type ExternalServiceName = "bantou" | "open_spec" | "memory";
export type ServerErrorKind =
  | "invalid_input"
  | "not_found"
  | "conflict"
  | "external_unavailable"
  | "external_outcome_unknown"
  | "internal";

/** Information that could not be read. Never show it as "empty". */
export interface MissingSource {
  /** What of the request could not be read (`body` …), when the server says. */
  item?: string | null;
  service: ExternalServiceName;
  kind: ServerErrorKind;
  detail: string;
}

export interface ExternalNote {
  request_id: string;
  service: string;
  kind: string;
  detail: string;
  first_ms: number;
  last_ms: number;
  checks: number;
}

export interface RequestDetail {
  request: RequestRecord;
  lane: LaneName;
  steps: Step[];
  waits: Wait[];
  intakes: Intake[];
  allowed_operations: AllowedOperation[];
  /** Newest first. */
  history: HistoryEntry[];
  missing_sources: MissingSource[];
  external_notes: ExternalNote[];
}

export interface Artifact {
  artifact_ref: string;
  version: number;
  content: string;
}

export interface Notification {
  id: number;
  source_session: string | null;
  summary: string;
  request_id: string | null;
  question_id: string | null;
  at_ms: number;
}

/** The decision on a proposal. A decision on an old `proposalVersion` is a conflict. */
export type Decision = { type: "approve" } | { type: "revise"; instruction: string };

export interface AgentMcpConfig {
  path: string;
  transport: string;
  port: number;
  auth: string;
}
