// The one place where the webview reaches the native side.
//
// `callNative` runs a Tauri command; `api_call` is the only way to the server's GraphQL. In a
// browser without Tauri (a headless check) the sample-data plugin swaps this out with
// `setNativeCall`; nothing else in the app changes.
import { invoke } from "@tauri-apps/api/core";

export type NativeCall = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

const tauriCall: NativeCall = (command, args) => invoke(command, args);

let current: NativeCall = tauriCall;

export function callNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return current(command, args) as Promise<T>;
}

/** Replaces the native side (sample data). Returns what to call to put it back. */
export function setNativeCall(call: NativeCall): () => void {
  current = call;
  return () => {
    current = tauriCall;
  };
}
