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
        title: "この network からは鍵を取れません。server と同じ network から初回の接続をしてください",
        description: "鍵は、server が決めた network からだけ受け取れます。鍵を持った後は、外の network からでも使えます。",
      };
    case "unauthorized":
      return {
        title: "この server では鍵が無効です",
        description: "この server は、持っている鍵を受け付けませんでした。「鍵を取り直す」で、server と同じ network から鍵を取り直してください。",
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

/** What the connection screen shows. */
export type ConnectionView =
  /** The saved connection is being tried. */
  | "restoring"
  /** A working connection. */
  | "connected"
  /** No key is held: the address form that fetches one (first start, after "接続をやり直す", or "鍵を取り直す"). */
  | "acquire"
  /** A key is held but the server at the saved address does not accept it. */
  | "key_invalid"
  /** A key is held and the connection failed for another reason: change the address or try again. */
  | "failed";

/**
 * Which screen the state calls for. The key-fetching form is shown only when no key is held
 * (`hasKey` false) or the user asked to fetch the key again (`reacquiring`); otherwise the usual
 * connection is the one that is shown, with the address and nothing about how a key is fetched.
 */
export function connectionView(state: {
  restoring: boolean;
  connected: boolean;
  hasKey: boolean;
  reacquiring: boolean;
  failureKind: ConnectErrorKind | null;
}): ConnectionView {
  if (state.restoring) return "restoring";
  if (state.connected) return "connected";
  if (!state.hasKey || state.reacquiring) return "acquire";
  return state.failureKind === "unauthorized" ? "key_invalid" : "failed";
}
