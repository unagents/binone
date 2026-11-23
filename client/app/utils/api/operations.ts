// The operations the webview may ask the native side for, with their arguments and results.
//
// The names are the server's GraphQL field names and must equal `Operation::name` in
// src-tauri/src/api/ops.rs (tests/api.test.ts compares the two files). The webview sends only a
// name and arguments; the native side owns the address, the key and the document.
import type {
  AgentMcpConfig,
  Artifact,
  Decision,
  DraftInput,
  Notification,
  Request,
  RequestDetail,
  RequestFilter,
  RequestSummary,
  ServerInfo,
  Settings,
  SettingsView,
  SubmitBatch,
  SubmitCheck,
  Wait,
} from "~/types/binone";

export const OPERATION_NAMES = [
  "serverInfo",
  "requests",
  "request",
  "requestDetail",
  "artifact",
  "conversation",
  "notifications",
  "projects",
  "settings",
  "agentMcpConfig",
  "saveDraft",
  "submitRequest",
  "checkSubmitRequest",
  "submitRequests",
  "decideApproval",
  "answerQuestion",
  "createProject",
  "updateSettings",
  "registerConversation",
] as const;

export type OperationName = (typeof OPERATION_NAMES)[number];

/** Operations that change something on the server. Never retried after an unclear outcome. */
export const MUTATIONS: readonly OperationName[] = [
  "saveDraft",
  "submitRequest",
  "submitRequests",
  "decideApproval",
  "answerQuestion",
  "createProject",
  "updateSettings",
  "registerConversation",
];

export interface OperationTable {
  serverInfo: { args: undefined; result: ServerInfo };
  requests: { args: { filter?: RequestFilter | null } | undefined; result: RequestSummary[] };
  request: { args: { id: string }; result: Request };
  requestDetail: { args: { id: string }; result: RequestDetail };
  artifact: { args: { requestId: string; artifactRef: string }; result: Artifact };
  /** The id of the conversation registered for the request, or `null` when there is none. */
  conversation: { args: { requestId: string }; result: string | null };
  notifications: { args: { limit?: number | null } | undefined; result: Notification[] };
  projects: { args: undefined; result: string[] };
  settings: { args: undefined; result: SettingsView };
  agentMcpConfig: { args: undefined; result: AgentMcpConfig };
  saveDraft: { args: DraftInput; result: Request };
  /** draft -> todo. `version` is the one read last. */
  submitRequest: { args: { id: string; version: number }; result: Request };
  /** Dry run of draft -> todo: changes nothing. A refusal comes back as data. */
  checkSubmitRequest: { args: { id: string; version: number }; result: SubmitCheck };
  /** All of the drafts become todos in one step, or none does. A refusal comes back as data. */
  submitRequests: { args: { items: { id: string; version: number }[] }; result: SubmitBatch };
  decideApproval: {
    args: { requestId: string; proposalRef: string; proposalVersion: number; decision: Decision };
    result: Wait[];
  };
  answerQuestion: { args: { requestId: string; questionId: string; choice: string }; result: true };
  /** Returns the project's name as the server stored it. */
  createProject: { args: { name: string }; result: string };
  updateSettings: { args: { settings: Settings }; result: SettingsView };
  registerConversation: { args: { requestId: string; conversationId: string }; result: true };
}

export type ArgsOf<K extends OperationName> = OperationTable[K]["args"];
export type ResultOf<K extends OperationName> = OperationTable[K]["result"];
