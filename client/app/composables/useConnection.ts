import { callNative } from "~/utils/api/transport";
import { isConnectError, type ConnectError } from "~/utils/connection";

/** A working connection. `port: null` means the default port is in use. */
export interface Connected {
  host: string;
  port: number | null;
  version: string;
}

/** A failed attempt and the address it was made to. */
export interface Failure {
  host: string;
  port: number | null;
  error: ConnectError;
}

type Restored =
  | { state: "none" }
  | ({ state: "connected" } & Connected)
  | ({ state: "failed" } & Failure);

function toConnectError(error: unknown): ConnectError {
  if (isConnectError(error)) return error;
  // Not an error from our commands (e.g. the page is open outside the app).
  return { kind: "internal", detail: String(error) };
}

// The connection is one thing for the whole app: the header, the gate and the connection screen
// all read this state.
let restoration: Promise<void> | null = null;

/** The result of an attempt that does not change what the screen shows by itself. */
export type Attempt = { ok: true } | { ok: false; failure: Failure };

/**
 * The connection to the binone server. All of it happens on the native side; the key never reaches
 * this code. There are two connections, kept apart:
 * - the key's: `connect` (first connection, or fetching the key again) asks the address for a key;
 * - the usual one: `restore` at startup and `changeAddress` use the key that is held, and never
 *   fetch one. A server that does not accept the key is shown as such (`unauthorized`).
 */
export function useConnection() {
  const connected = useState<Connected | null>("binone-connected", () => null);
  const failure = useState<Failure | null>("binone-failure", () => null);
  const defaultPort = useState<number | null>("binone-default-port", () => null);
  /** True until the saved address and key were tried once at startup. */
  const restoring = useState<boolean>("binone-restoring", () => true);
  /** Whether a key is held. Without one, the screen is the first-connection form. */
  const hasKey = useState<boolean>("binone-has-key", () => false);
  /** The user chose "鍵を取り直す": the form that fetches a key is shown although one is held. */
  const reacquiring = useState<boolean>("binone-reacquiring", () => false);

  async function restoreOnce() {
    try {
      defaultPort.value = await callNative<number>("default_port");
      const restored = await callNative<Restored>("restore");
      if (restored.state === "connected") {
        hasKey.value = true;
        connected.value = { host: restored.host, port: restored.port, version: restored.version };
      } else if (restored.state === "failed") {
        hasKey.value = true;
        failure.value = { host: restored.host, port: restored.port, error: restored.error };
      }
    } catch (error) {
      failure.value = { host: "", port: null, error: toConnectError(error) };
    }
    restoring.value = false;
  }

  /** Startup: try the saved address and key. Runs once, however many screens ask. */
  function restore(): Promise<void> {
    restoration ??= restoreOnce();
    return restoration;
  }

  /**
   * The key's connection: the user confirmed an address to get a key from. The native side saves
   * the address and the key (replacing the one held) as soon as it has the key, then checks the
   * connection. A refused attempt (outside the allowed range, unreachable) leaves what is held as it was.
   */
  async function connect(host: string, port: number | null): Promise<Attempt> {
    failure.value = null;
    try {
      connected.value = await callNative<Connected>("connect", { host, port });
      hasKey.value = true;
      reacquiring.value = false;
      return { ok: true };
    } catch (error) {
      failure.value = { host, port, error: toConnectError(error) };
      return { ok: false, failure: failure.value };
    }
  }

  /**
   * The usual connection to another address, with the key that is held. Nothing is saved unless
   * the server there accepts the key. While connected, the failure is only returned (the screen
   * stays on the working connection); otherwise it is also what the connection screen shows.
   */
  async function changeAddress(host: string, port: number | null): Promise<Attempt> {
    try {
      connected.value = await callNative<Connected>("change_address", { host, port });
      failure.value = null;
      return { ok: true };
    } catch (error) {
      const failed: Failure = { host, port, error: toConnectError(error) };
      if (!connected.value) failure.value = failed;
      return { ok: false, failure: failed };
    }
  }

  /**
   * A call was refused with 401: the key is not valid for the server in use. The screen shows the
   * form that fetches the key again, filled with that server's address.
   */
  function askForKeyAgain() {
    const current = connected.value;
    if (current) {
      failure.value = {
        host: current.host,
        port: current.port,
        error: { kind: "unauthorized", detail: "POST /graphql: 401" },
      };
    }
    connected.value = null;
    reacquiring.value = true;
  }

  /** "接続をやり直す": the saved address and key are deleted and the screen is the first-connection form. */
  async function forget(): Promise<ConnectError | null> {
    try {
      await callNative<null>("forget_connection");
    } catch (error) {
      return toConnectError(error);
    }
    connected.value = null;
    failure.value = null;
    hasKey.value = false;
    reacquiring.value = false;
    return null;
  }

  /** `host` or `host:port`, with the default port shown when none was given. */
  function address(host: string, port: number | null) {
    const shownPort = port ?? defaultPort.value;
    return shownPort === null ? host : `${host}:${shownPort}`;
  }

  return {
    connected,
    failure,
    defaultPort,
    restoring,
    hasKey,
    reacquiring,
    restore,
    connect,
    changeAddress,
    askForKeyAgain,
    forget,
    address,
  };
}
