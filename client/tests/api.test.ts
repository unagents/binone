import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { API_ERROR_KINDS, describeApiError, isApiError, pointsToConnection, toApiError } from "../app/utils/api/errors";
import { createLoader, type LoaderState } from "../app/utils/api/loader";
import { MUTATIONS, OPERATION_NAMES } from "../app/utils/api/operations";
import { createSampleServer } from "../app/utils/api/sample";

const ops = readFileSync(new URL("../src-tauri/src/api/ops.rs", import.meta.url), "utf8");
const errorRs = readFileSync(new URL("../src-tauri/src/api/error.rs", import.meta.url), "utf8");

describe("the operation names", () => {
  it("are the ones the native allowlist has", () => {
    const body = ops.slice(ops.indexOf("pub fn name(self)"), ops.indexOf("pub fn parse"));
    const native = [...body.matchAll(/=> "([A-Za-z]+)"/g)].map((m) => m[1]);
    expect([...OPERATION_NAMES].sort()).toEqual([...native].sort());
  });
  it("mark the same mutations as the native side", () => {
    const body = ops.slice(ops.indexOf("pub fn is_mutation"), ops.indexOf("pub fn shape"));
    const native = [...body.matchAll(/Operation::([A-Za-z]+)/g)].map((m) => (m[1] ?? "").replace(/^./, (c) => c.toLowerCase()));
    expect([...MUTATIONS].sort()).toEqual([...native].sort());
  });
});

describe("the error kinds", () => {
  it("are the ones the native side has", () => {
    const body = errorRs.slice(errorRs.indexOf("pub enum ApiErrorKind"), errorRs.indexOf("pub struct ApiError"));
    const native = [...body.matchAll(/^\s{4}([A-Z][A-Za-z]+),/gm)].map((m) => (m[1] ?? "").replace(/(?<!^)([A-Z])/g, "_$1").toLowerCase());
    expect([...API_ERROR_KINDS].sort()).toEqual([...native].sort());
  });
  it("each has a text for the screen", () => {
    for (const kind of API_ERROR_KINDS) {
      const text = describeApiError(kind, "192.168.0.10:47610");
      expect(text.title.length).toBeGreaterThan(0);
      expect(text.description.length).toBeGreaterThan(0);
    }
  });
  it("tell a possibly-accepted change apart from a refused one", () => {
    expect(describeApiError("outcome_unknown").description).toContain("重ねて送らず");
    expect(describeApiError("external_unavailable").description).toContain("受け付けられていません");
  });
  it("send the user to the connection screen only when the connection is the thing to check", () => {
    expect(pointsToConnection("unauthorized")).toBe(true);
    expect(pointsToConnection("conflict")).toBe(false);
  });
  it("turn anything thrown into an error of ours", () => {
    expect(isApiError({ kind: "conflict", detail: "x" })).toBe(true);
    expect(isApiError({ kind: "nope", detail: "x" })).toBe(false);
    expect(toApiError(new Error("boom"))).toEqual({ kind: "internal", detail: "Error: boom" });
  });
});

describe("createLoader", () => {
  function recorder() {
    const states: LoaderState<number>[] = [];
    return { states, onState: (s: LoaderState<number>) => states.push({ ...s }) };
  }
  it("reports loading, then the data", async () => {
    const r = recorder();
    await createLoader(async () => 1, r.onState).load();
    expect(r.states.map((s) => [s.loading, s.data])).toEqual([[true, null], [false, 1]]);
  });
  it("reports a failure as an ApiError and keeps the data read before", async () => {
    const r = recorder();
    let n = 0;
    const loader = createLoader(async () => {
      if (++n === 2) throw { kind: "unreachable", detail: "no" };
      return n;
    }, r.onState);
    await loader.load();
    await loader.load();
    expect(r.states.at(-1)).toEqual({ data: 1, error: { kind: "unreachable", detail: "no" }, loading: false });
  });
  it("drops the answer of an older load", async () => {
    const r = recorder();
    const resolvers: Array<(v: number) => void> = [];
    const loader = createLoader(() => new Promise<number>((res) => resolvers.push(res)), r.onState);
    const first = loader.load();
    const second = loader.load();
    resolvers[1]?.(2);
    await second;
    resolvers[0]?.(1);
    await first;
    expect(r.states.at(-1)?.data).toBe(2);
  });
  it("drops answers after the screen was left", async () => {
    const r = recorder();
    let resolve: (v: number) => void = () => {};
    const loader = createLoader(() => new Promise<number>((res) => (resolve = res)), r.onState);
    const pending = loader.load();
    loader.dispose();
    resolve(5);
    await pending;
    expect(r.states).toHaveLength(1);
  });
});

describe("the sample server", () => {
  const call = (server: ReturnType<typeof createSampleServer>, op: string, args?: unknown) => server("api_call", { op, args });
  it("answers the connection commands", async () => {
    const server = createSampleServer();
    expect(await server("restore")).toMatchObject({ state: "connected" });
    expect(await server("default_port")).toBe(47610);
  });
  it("refuses a name that is not in the allowlist", async () => {
    await expect(call(createSampleServer(), "dropEverything", {})).rejects.toMatchObject({ kind: "invalid_input" });
  });
  it("fails like the native side when asked to", async () => {
    await expect(call(createSampleServer({ failWith: "unauthorized" }), "projects")).rejects.toMatchObject({ kind: "unauthorized" });
  });
  it("saves a draft, conflicts on an old version, and submits", async () => {
    const server = createSampleServer();
    const draft = (await call(server, "saveDraft", { title: "a", project: "binone", kind: "task" })) as { id: string; version: number; lane: string };
    expect(draft.lane).toBe("draft");
    const updated = (await call(server, "saveDraft", { id: draft.id, version: draft.version, title: "b" })) as { version: number };
    await expect(call(server, "saveDraft", { id: draft.id, version: draft.version, title: "c" })).rejects.toMatchObject({ kind: "conflict" });
    const submitted = (await call(server, "submitRequest", { id: draft.id, version: updated.version })) as { lane: string };
    expect(submitted.lane).toBe("todo");
  });
  it("checks a draft without changing it, and submits a batch all or nothing", async () => {
    const server = createSampleServer();
    const save = async (title: string, protocol: string) =>
      (await call(server, "saveDraft", { title, project: "binone", kind: "task", protocol })) as { id: string; version: number };
    const good = await save("a", "p");
    const bad = await save("b", "");
    const listed = (await call(server, "requests", {})) as { id: string; version: number }[];
    expect(listed.find((r) => r.id === good.id)?.version).toBe(good.version);

    expect(await call(server, "checkSubmitRequest", { id: good.id, version: good.version })).toMatchObject({ ok: true, code: null });
    expect(await call(server, "checkSubmitRequest", { id: bad.id, version: bad.version })).toMatchObject({ ok: false, code: "incomplete", missing: ["protocol"] });
    expect(await call(server, "checkSubmitRequest", { id: good.id, version: good.version + 9 })).toMatchObject({ code: "version_conflict" });
    expect(await call(server, "checkSubmitRequest", { id: "nope", version: 1 })).toMatchObject({ code: "not_found" });

    const refused = (await call(server, "submitRequests", { items: [good, bad].map(({ id, version }) => ({ id, version })) })) as { ok: boolean; requests: unknown[] };
    expect(refused.ok).toBe(false);
    expect(await call(server, "checkSubmitRequest", { id: good.id, version: good.version })).toMatchObject({ ok: true });
    const done = (await call(server, "submitRequests", { items: [{ id: good.id, version: good.version }] })) as { ok: boolean; requests: { lane: string }[] };
    expect(done.ok).toBe(true);
    expect(done.requests.map((r) => r.lane)).toEqual(["todo"]);
    expect(await call(server, "checkSubmitRequest", { id: good.id, version: good.version + 1 })).toMatchObject({ code: "not_draft" });
    await expect(call(server, "submitRequests", { items: [] })).rejects.toMatchObject({ kind: "invalid_input" });
  });
  it("keeps draft first in the settings", async () => {
    const server = createSampleServer();
    const view = (await call(server, "settings")) as { settings: { lanes: { name: string }[] } };
    const lanes = [...view.settings.lanes].reverse();
    await expect(call(server, "updateSettings", { settings: { ...view.settings, lanes } })).rejects.toMatchObject({ kind: "invalid_input" });
  });
});
