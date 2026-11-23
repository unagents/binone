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

/**
 * The connection to the binone server. All of it happens on the native side through the
 * `default_port`, `connect` and `restore` commands; the key never reaches this code.
 */
export function useConnection() {
  const connected = useState<Connected | null>("binone-connected", () => null);
  const failure = useState<Failure | null>("binone-failure", () => null);
  const defaultPort = useState<number | null>("binone-default-port", () => null);
  /** True until the saved address and key were tried once at startup. */
  const restoring = useState<boolean>("binone-restoring", () => true);

  async function restoreOnce() {
    try {
      defaultPort.value = await callNative<number>("default_port");
      const restored = await callNative<Restored>("restore");
      if (restored.state === "connected") {
        connected.value = { host: restored.host, port: restored.port, version: restored.version };
      } else if (restored.state === "failed") {
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
   * The user confirmed an address. The native side gets a key, saves the address and the key as
   * soon as it has the key, and then checks the connection; so a failure after the key was
   * acquired (the check itself) leaves the new address saved, and the next start tries it again.
   */
  async function connect(host: string, port: number | null) {
    failure.value = null;
    try {
      connected.value = await callNative<Connected>("connect", { host, port });
    } catch (error) {
      failure.value = { host, port, error: toConnectError(error) };
    }
  }

  /**
   * Back to the form to enter another address. The saved address and key stay until a key is
   * acquired from the new address: they are replaced at that point, before the connection check.
   */
  function change() {
    connected.value = null;
    failure.value = null;
  }

  /** `host` or `host:port`, with the default port shown when none was given. */
  function address(host: string, port: number | null) {
    const shownPort = port ?? defaultPort.value;
    return shownPort === null ? host : `${host}:${shownPort}`;
  }

  return { connected, failure, defaultPort, restoring, restore, connect, change, address };
}
