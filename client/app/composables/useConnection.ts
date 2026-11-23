import { invoke } from "@tauri-apps/api/core";
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
  return { kind: "storage", detail: String(error) };
}

/**
 * The connection to the binone server. All of it happens on the native side through the
 * `default_port`, `connect` and `restore` commands; the key never reaches this code.
 */
export function useConnection() {
  const connected = ref<Connected | null>(null);
  const failure = ref<Failure | null>(null);
  const defaultPort = ref<number | null>(null);

  /** Startup: try the saved address and key. */
  async function restore() {
    try {
      defaultPort.value = await invoke<number>("default_port");
      const restored = await invoke<Restored>("restore");
      if (restored.state === "connected") {
        connected.value = { host: restored.host, port: restored.port, version: restored.version };
      } else if (restored.state === "failed") {
        failure.value = { host: restored.host, port: restored.port, error: restored.error };
      }
    } catch (error) {
      failure.value = { host: "", port: null, error: toConnectError(error) };
    }
  }

  /**
   * The user confirmed an address. The native side gets a key, saves the address and the key as
   * soon as it has the key, and then checks the connection; so a failure after the key was
   * acquired (the check itself) leaves the new address saved, and the next start tries it again.
   */
  async function connect(host: string, port: number | null) {
    failure.value = null;
    try {
      connected.value = await invoke<Connected>("connect", { host, port });
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

  return { connected, failure, defaultPort, restore, connect, change };
}
