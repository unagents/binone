import { describe, expect, it } from "vitest";
import {
  describeConnectError,
  isConnectError,
  isValidIpv4,
  parsePort,
  type ConnectErrorKind,
} from "../app/utils/connection";

describe("isValidIpv4", () => {
  it("accepts dotted-quad addresses", () => {
    expect(isValidIpv4("192.168.0.10")).toBe(true);
    expect(isValidIpv4("0.0.0.0")).toBe(true);
    expect(isValidIpv4("255.255.255.255")).toBe(true);
  });
  it("rejects everything else", () => {
    for (const v of ["", "192.168.0", "192.168.0.256", "192.168.0.01", "a.b.c.d", "1.2.3.4.5", " 1.2.3.4", "host.local", "::1"]) {
      expect(isValidIpv4(v)).toBe(false);
    }
  });
});

describe("parsePort", () => {
  it("treats empty input as the default port", () => {
    expect(parsePort("")).toEqual({ ok: true, port: null });
    expect(parsePort("  ")).toEqual({ ok: true, port: null });
  });
  it("accepts 1 to 65535", () => {
    expect(parsePort("1")).toEqual({ ok: true, port: 1 });
    expect(parsePort(" 65535 ")).toEqual({ ok: true, port: 65535 });
  });
  it("rejects out-of-range and non-numeric input", () => {
    for (const v of ["0", "65536", "-1", "80a", "1.5", "080"]) {
      expect(parsePort(v)).toEqual({ ok: false });
    }
  });
});

describe("connect errors", () => {
  it("recognises the error object of the native commands", () => {
    expect(isConnectError({ kind: "forbidden", detail: "POST /key: 403" })).toBe(true);
    expect(isConnectError("boom")).toBe(false);
    expect(isConnectError(null)).toBe(false);
  });
  it("gives every kind of failure its own message", () => {
    const kinds: ConnectErrorKind[] = ["invalid_input", "unreachable", "forbidden", "unauthorized", "bad_response", "storage", "internal"];
    const titles = kinds.map((kind) => describeConnectError(kind, "10.0.0.1:47610").title);
    expect(new Set(titles).size).toBe(kinds.length);
    expect(titles[1]).toContain("10.0.0.1:47610");
  });
});
