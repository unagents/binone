import { describe, expect, it } from "vitest";
import { connectionView, describeConnectError, type ConnectErrorKind } from "../app/utils/connection";
import { describeApiError } from "../app/utils/api/errors";
import { createSampleServer, SAMPLE_KEY_REJECTED_HOST, SAMPLE_OUTSIDE_RANGE_HOST } from "../app/utils/api/sample";

const base = { restoring: false, connected: false, hasKey: false, reacquiring: false, failureKind: null as ConnectErrorKind | null };

describe("which screen the connection state calls for", () => {
  it("waits while the saved connection is tried", () => {
    expect(connectionView({ ...base, restoring: true })).toBe("restoring");
  });
  it("asks for the first connection only when no key is held", () => {
    expect(connectionView(base)).toBe("acquire");
    expect(connectionView({ ...base, failureKind: "forbidden" })).toBe("acquire");
  });
  it("does not show the key form once a key is held", () => {
    expect(connectionView({ ...base, hasKey: true, connected: true })).toBe("connected");
    expect(connectionView({ ...base, hasKey: true, failureKind: "unreachable" })).toBe("failed");
    expect(connectionView({ ...base, hasKey: true, failureKind: "bad_response" })).toBe("failed");
  });
  it("shows the invalid-key notice only when the server rejected the key", () => {
    expect(connectionView({ ...base, hasKey: true, failureKind: "unauthorized" })).toBe("key_invalid");
  });
  it("shows the key form again only when the user asked to fetch the key again", () => {
    expect(connectionView({ ...base, hasKey: true, failureKind: "unauthorized", reacquiring: true })).toBe("acquire");
  });
});

describe("what the user reads", () => {
  it("says a key cannot be fetched from this network (403), in Japanese, asking for a first connection from the server's network", () => {
    const { title } = describeConnectError("forbidden", "10.0.0.1");
    expect(title).toBe("この network からは鍵を取れません。server と同じ network から初回の接続をしてください");
    expect(describeApiError("forbidden").title).toBe(title);
  });
  it("says the key is invalid for this server (401)", () => {
    expect(describeConnectError("unauthorized", "10.0.0.1").title).toBe("この server では鍵が無効です");
    expect(describeApiError("unauthorized").title).toBe("この server では鍵が無効です");
  });
  it("gives a one-line title and a point to check for a server that is not reached or answers oddly", () => {
    for (const kind of ["unreachable", "bad_response"] as const) {
      const { title, description } = describeConnectError(kind, "10.0.0.1:47610");
      expect(title).not.toContain("\n");
      expect(title.length).toBeGreaterThan(0);
      expect(description.length).toBeGreaterThan(0);
    }
  });
  it("never shows where keys come from or the range they are given to", () => {
    for (const kind of ["forbidden", "unauthorized", "unreachable"] as const) {
      const { title, description, hint } = describeConnectError(kind, "10.0.0.1");
      expect(`${title}${description}${hint ?? ""}`).not.toMatch(/IP 範囲|\/key/);
    }
  });
});

describe("the connection, as the sample native side keeps it", () => {
  const call = (native: ReturnType<typeof createSampleServer>, command: string, args?: Record<string, unknown>) => native(command, args);

  it("changing the address keeps the key", async () => {
    const native = createSampleServer();
    expect(await call(native, "restore")).toMatchObject({ state: "connected" });
    expect(await call(native, "change_address", { host: "10.0.0.2", port: 5000 })).toMatchObject({ host: "10.0.0.2", port: 5000 });
    // Started again: the key is still held, at the new address.
    expect(await call(native, "restore")).toMatchObject({ state: "connected", host: "10.0.0.2", port: 5000 });
  });
  it("forgetting the connection goes back to the first screen, and a first connection holds a key again", async () => {
    const native = createSampleServer();
    await call(native, "forget_connection");
    expect(await call(native, "restore")).toEqual({ state: "none" });
    await expect(call(native, "change_address", { host: "10.0.0.2", port: null })).rejects.toMatchObject({ kind: "unauthorized" });
    await call(native, "connect", { host: "10.0.0.3", port: null });
    expect(await call(native, "restore")).toMatchObject({ state: "connected", host: "10.0.0.3" });
  });
  it("a server that rejects the key is a 401, and the address is not changed", async () => {
    const native = createSampleServer();
    await expect(call(native, "change_address", { host: SAMPLE_KEY_REJECTED_HOST, port: null })).rejects.toMatchObject({ kind: "unauthorized" });
    expect(await call(native, "restore")).toMatchObject({ host: "127.0.0.1" });
    // Fetching the key again replaces it.
    expect(await call(native, "connect", { host: SAMPLE_KEY_REJECTED_HOST, port: null })).toMatchObject({ host: SAMPLE_KEY_REJECTED_HOST });
  });
  it("a key refused outside the allowed range is a 403 and what is held stays", async () => {
    const native = createSampleServer();
    await expect(call(native, "connect", { host: SAMPLE_OUTSIDE_RANGE_HOST, port: null })).rejects.toMatchObject({ kind: "forbidden" });
    expect(await call(native, "restore")).toMatchObject({ state: "connected", host: "127.0.0.1" });
  });
});
