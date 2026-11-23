// Input checks and messages for the connection screen. The connection itself (address and key
// storage, HTTP) lives on the native side: see src-tauri/src/lib.rs.

/** Dotted-quad IPv4 only (the user is asked for an IP; IPv6 and host names are not accepted). */
export function isValidIpv4(value: string): boolean {
  const parts = value.split(".");
  if (parts.length !== 4) return false;
  return parts.every((part) => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255);
}

/** Parses the optional port field. Empty input means the default port (`null`). */
export function parsePort(input: string): { ok: true; port: number | null } | { ok: false } {
  const text = input.trim();
  if (text === "") return { ok: true, port: null };
  if (!/^[1-9]\d{0,4}$/.test(text)) return { ok: false };
  const port = Number(text);
  if (port > 65535) return { ok: false };
  return { ok: true, port };
}

/** Mirrors `ErrorKind` in src-tauri/src/server.rs. */
export type ConnectErrorKind =
  | "invalid_input"
  | "unreachable"
  | "forbidden"
  | "unauthorized"
  | "bad_response"
  | "storage"
  | "internal";

export interface ConnectError {
  kind: ConnectErrorKind;
  detail: string;
}

export function isConnectError(value: unknown): value is ConnectError {
  if (typeof value !== "object" || value === null) return false;
  const { kind, detail } = value as Record<string, unknown>;
  return typeof kind === "string" && typeof detail === "string";
}

/**
 * Whether a keydown in a field of the connection form starts the connection: the Return key,
 * but not while a connection is being made, and not the Return that confirms an input method's
 * conversion. WebKit reports that Return after the composition has ended, so `isComposing` is
 * already false there; the key code 229 is what still marks it.
 */
export function submitsOnKeydown(
  event: { key: string; isComposing: boolean; keyCode: number },
  connecting: boolean,
): boolean {
  if (event.key !== "Enter") return false;
  if (event.isComposing || event.keyCode === 229) return false;
  return !connecting;
}

/**
 * What the user reads for each kind of failure: what happened and what to check. `hint` is a
 * further thing to check, shown after the description.
 */
export function describeConnectError(
  kind: ConnectErrorKind,
  address: string,
): { title: string; description: string; hint?: string } {
  switch (kind) {
    case "unreachable":
      return {
        title: `サーバーに届きませんでした（${address}）`,
        description: "IP と port が合っているか、サーバーが起動しているか、同じネットワークにいるかを確かめてください。",
        // The first connection to an address on the local network fails while macOS is still
        // asking for the permission, and keeps failing when it was denied.
        hint: "macOS が「ローカルネットワーク上のデバイスの検索と接続」の許可を求めていたら、許可してからもう一度「接続」を押してください。許可しなかった時は「システム設定 › プライバシーとセキュリティ › ローカルネットワーク」で binone をオンにしてください。",
      };
    case "forbidden":
      return {
        title: "サーバーが鍵の発行を断りました",
        description: "この Mac の IP が、サーバーの設定で鍵を渡してよい IP 範囲に入っているかを確かめてください。",
      };
    case "unauthorized":
      return {
        title: "サーバーが鍵を受け付けませんでした",
        description: "鍵を取り直しても通りませんでした。少し置いてからもう一度接続し、続く時はサーバーの状態を確かめてください。",
      };
    case "bad_response":
      return {
        title: "サーバーの応答が想定と違いました",
        description: "接続先が binone サーバーか（IP と port）、サーバーとこのアプリの版が合っているかを確かめてください。",
      };
    case "invalid_input":
      return {
        title: "接続先の形が正しくありません",
        description: "IP は 192.168.0.10 の形、port は 1 から 65535 の数字で入れてください。",
      };
    case "storage":
      return {
        title: "接続先をこの Mac に保存・読み出しできませんでした",
        description: "アプリを開き直しても続く時は、下の詳細を添えて知らせてください。",
      };
    case "internal":
      return {
        title: "アプリの中で通信を用意できませんでした",
        description: "サーバーには何も送っていません。アプリを開き直しても続く時は、下の詳細を添えて知らせてください。",
      };
  }
}
