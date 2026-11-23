// The errors of an API call, and what the user reads for each.
//
// Mirrors `ApiErrorKind` in src-tauri/src/api/error.rs. The kinds that the connection screen
// also has keep their names and texts (`describeConnectError`), so one wording serves both.
import { describeConnectError, type ConnectErrorKind } from "~/utils/connection";

export type ApiErrorKind =
  | ConnectErrorKind
  | "not_connected"
  | "not_found"
  | "conflict"
  | "external_unavailable"
  | "outcome_unknown"
  | "server_error"
  | "unsupported";

export const API_ERROR_KINDS: readonly ApiErrorKind[] = [
  "invalid_input",
  "unreachable",
  "forbidden",
  "unauthorized",
  "bad_response",
  "storage",
  "internal",
  "not_connected",
  "not_found",
  "conflict",
  "external_unavailable",
  "outcome_unknown",
  "server_error",
  "unsupported",
];

export interface ApiError {
  kind: ApiErrorKind;
  detail: string;
  /** The server's reason, when it gave one the app knows (see `UNSUPPORTED_BY_OPENSPEC_MCP`). */
  reason?: string;
  /** The tools of openspec-mcp that are lacking, for `unsupported_by_openspec_mcp`. */
  missing?: string[];
}

/** `reason` of an `external_unavailable` that says openspec-mcp lacks what keeping requests needs. */
export const UNSUPPORTED_BY_OPENSPEC_MCP = "unsupported_by_openspec_mcp";

export function isApiError(value: unknown): value is ApiError {
  if (typeof value !== "object" || value === null) return false;
  const { kind, detail } = value as Record<string, unknown>;
  return typeof kind === "string" && (API_ERROR_KINDS as readonly string[]).includes(kind) && typeof detail === "string";
}

/** Anything thrown by a call, as an `ApiError`. Not from our commands (the page is open outside the app): `internal`. */
export function toApiError(error: unknown): ApiError {
  if (isApiError(error)) return error;
  return { kind: "internal", detail: String(error) };
}

/** Kinds after which the saved connection is the thing to look at (the header link goes there). */
export function pointsToConnection(kind: ApiErrorKind): boolean {
  return ["unreachable", "forbidden", "unauthorized", "not_connected", "bad_response", "storage"].includes(kind);
}

/** What the user reads: what happened and what to check. `address` is the server's address. */
export function describeApiError(
  kind: ApiErrorKind,
  address = "",
  extras: Pick<ApiError, "reason" | "missing"> = {},
): { title: string; description: string; hint?: string } {
  if (kind === "external_unavailable" && extras.reason === UNSUPPORTED_BY_OPENSPEC_MCP) {
    const missing = extras.missing ?? [];
    return {
      title: "openspec-mcp が未対応のため、受け付けられませんでした",
      description:
        "依頼の中身を保管する openspec-mcp に、そのために要るツールがありません" +
        (missing.length ? `（欠けているツール: ${missing.join("、")}）` : "") +
        "。別の場所に保存し直すことはしていません。openspec-mcp を対応した版にしてから、もう一度操作してください。",
    };
  }
  switch (kind) {
    case "not_connected":
      return { title: "サーバーに接続していません", description: "上の接続の表示から接続の画面を開いてください。" };
    case "not_found":
      return { title: "対象が見つかりませんでした", description: "削除されたか、番号が違います。一覧に戻って開き直してください。" };
    case "conflict":
      return {
        title: "他の操作と重なりました",
        description: "読み込んだ後に、依頼の状態や版が変わりました。読み込み直して内容を確かめ、もう一度操作してください。",
      };
    case "external_unavailable":
      return {
        title: "連携先のサービスが使えませんでした",
        description: "bantou・openspec・memory のどれかに届きません。この操作は受け付けられていません。サービスの状態を確かめてから、もう一度操作してください。",
      };
    case "outcome_unknown":
      return {
        title: "受け付けられたか分かりません",
        description: "送った後で答えが返りませんでした。アプリは自動では送り直しません。同じ操作を重ねて送らず、一覧や状況を読み込み直して結果を確かめてください。",
      };
    case "server_error":
      return { title: "サーバーの中で失敗しました", description: "少し置いてからもう一度操作してください。続く時は、下の詳細を添えて知らせてください。" };
    case "unsupported":
      return {
        title: "サーバーが未対応です",
        description: "接続先のサーバーにこの操作がありません。サーバーとこのアプリの版が合っているかを確かめてください。",
      };
    default:
      return describeConnectError(kind, address);
  }
}
