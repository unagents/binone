import { describe, expect, it } from "vitest";
import {
  MANUAL_COMMAND,
  badgeText,
  canRunUpdate,
  manualReason,
  finishedText,
  isUpdateCheck,
  isUpdateOutcome,
  manualReasonText,
  statusText,
  unknownCheck,
  type UpdateCheck,
  type UpdateOutcome,
} from "../app/utils/update";
import { createSampleServer, SAMPLE_UPDATES } from "../app/utils/api/sample";

const available: UpdateCheck = { state: "available", current: "0.2.0", latest: "0.3.0", method: { kind: "brew" } };
const availableByHand: UpdateCheck = { state: "available", current: "0.2.0", latest: "0.3.0", method: { kind: "manual", reason: "no_brew" } };
type Finished = Extract<UpdateOutcome, { state: "finished" }>;
const done = (over: Partial<Finished> = {}): Finished => ({
  state: "finished",
  ok: false,
  code: 1,
  step: "brew upgrade --cask binone",
  output: "",
  timed_out: false,
  ...over,
});
const latest: UpdateCheck = { state: "up_to_date", current: "0.2.0", latest: "0.2.0" };
const unknown: UpdateCheck = { state: "unknown", current: "0.2.0", reason: "HTTP 403" };

describe("statusText", () => {
  it("names the three states", () => {
    expect(statusText(latest, false)).toBe("最新です");
    expect(statusText(available, false)).toBe("新しい版があります");
    expect(statusText(unknown, false)).toBe("確認できません。HTTP 403");
  });
  it("says so before the first check", () => {
    expect(statusText(null, true)).toBe("確認しています…");
    expect(statusText(null, false)).toBe("まだ確認していません");
  });
});

describe("badgeText", () => {
  it("shows only for a newer version", () => {
    expect(badgeText(available)).toBe("新しい版 0.3.0");
    expect(badgeText(latest)).toBeNull();
    expect(badgeText(unknown)).toBeNull();
    expect(badgeText(null)).toBeNull();
  });
});

describe("the shapes from the native side", () => {
  it("accepts a check and refuses the rest", () => {
    for (const ok of [available, availableByHand, latest, unknown]) expect(isUpdateCheck(ok)).toBe(true);
    const noMethod = { state: "available", current: "1", latest: "2" };
    const badMethods = [{ kind: "x" }, { kind: "manual" }, { kind: "manual", reason: "other" }, "brew", null];
    for (const method of badMethods) expect(isUpdateCheck({ ...noMethod, method })).toBe(false);
    for (const bad of [noMethod, null, "x", {}, { state: "available", current: "1" }, { state: "unknown", current: "1" }, { state: "other", current: "1", latest: "2" }]) {
      expect(isUpdateCheck(bad)).toBe(false);
    }
  });
  it("turns a failed call into an unknown check", () => {
    expect(unknownCheck("0.2.0", "boom")).toEqual({ state: "unknown", current: "0.2.0", reason: "boom" });
    expect(unknownCheck("0.2.0", { kind: "internal", detail: "no tauri" })).toMatchObject({ reason: "no tauri" });
    expect(unknownCheck("", 42)).toEqual({ state: "unknown", current: "", reason: "確認できませんでした" });
  });
  it("accepts an outcome and refuses the rest", () => {
    expect(isUpdateOutcome({ state: "busy" })).toBe(true);
    expect(isUpdateOutcome({ state: "manual", reason: "no_brew", command: MANUAL_COMMAND, url: "u" })).toBe(true);
    expect(isUpdateOutcome({ state: "finished", ok: true, code: 0, step: "s", output: "", timed_out: false })).toBe(true);
    expect(isUpdateOutcome({ state: "finished", ok: true, code: 0, step: "s", output: "" })).toBe(false);
    for (const bad of [null, {}, { state: "finished" }, { state: "manual" }, { state: "x" }]) expect(isUpdateOutcome(bad)).toBe(false);
  });
});

describe("the words about an update", () => {
  it("gives the command to type by hand", () => {
    expect(MANUAL_COMMAND).toBe("brew upgrade --cask binone");
    expect(manualReasonText("no_brew")).toContain("Homebrew");
    expect(manualReasonText("not_installed_by_cask")).toContain("cask");
  });
  it("tells success from failure with the exit code", () => {
    expect(finishedText(done({ ok: true, code: 0 }))).toContain("再起動");
    const failed = finishedText(done());
    expect(failed).toContain("終了 code 1");
    expect(failed).toContain("brew upgrade --cask binone");
    expect(failed).not.toContain("tap");
    expect(finishedText(done({ code: null }))).toContain("終了 code なし");
  });
  it("keeps what went wrong with the tap after the line, whatever way the update ended", () => {
    const note = "tap unagents/tap は更新できませんでした（git pull が時間切れになりました）。";
    expect(finishedText(done({ ok: true, code: 0, note }))).toContain(note);
    expect(finishedText(done({ note }))).toContain(note);
    expect(finishedText(done({ code: null, timed_out: true, note }))).toContain(note);
    expect(finishedText(done({ ok: true, code: 0, note: null }))).not.toContain("tap");
    expect(isUpdateOutcome({ ...done(), note })).toBe(true);
  });
  it("says that an update ran out of time, and that it can be tried again", () => {
    const text = finishedText(done({ code: null, timed_out: true, step: "brew upgrade --cask binone" }));
    expect(text).toContain("時間切れ");
    expect(text).toContain("10 分");
    expect(text).toContain("もう一度");
    expect(text).not.toContain("終了 code");
  });
});

describe("the way to update", () => {
  it("offers the update button only when Homebrew can do it", () => {
    expect(canRunUpdate(available, null)).toBe(true);
    expect(canRunUpdate(availableByHand, null)).toBe(false);
    expect(canRunUpdate(latest, null)).toBe(false);
    expect(canRunUpdate(unknown, null)).toBe(false);
    expect(canRunUpdate(null, null)).toBe(false);
  });
  it("offers it again after a failure or a timeout, not after a success or a manual answer", () => {
    expect(canRunUpdate(available, done())).toBe(true);
    expect(canRunUpdate(available, done({ code: null, timed_out: true }))).toBe(true);
    expect(canRunUpdate(available, done({ ok: true, code: 0 }))).toBe(false);
    expect(canRunUpdate(available, { state: "manual", reason: "no_brew", command: MANUAL_COMMAND, url: "u" })).toBe(false);
  });
  it("shows the way by hand from the check, before the button is pressed", () => {
    expect(manualReason(availableByHand, null)).toBe("no_brew");
    expect(manualReason(available, null)).toBeNull();
    expect(manualReason(latest, null)).toBeNull();
    expect(manualReason(null, null)).toBeNull();
    const byCask: UpdateCheck = { ...availableByHand, method: { kind: "manual", reason: "not_installed_by_cask" } };
    expect(manualReason(byCask, null)).toBe("not_installed_by_cask");
    // What the run found wins over what the check found.
    expect(manualReason(available, { state: "manual", reason: "not_installed_by_cask", command: "c", url: "u" })).toBe("not_installed_by_cask");
  });
});

describe("the sample data", () => {
  it("draws all three states of the check", async () => {
    const states: string[] = [];
    for (const update of SAMPLE_UPDATES) {
      const answer = await createSampleServer({ update })("check_update");
      expect(isUpdateCheck(answer)).toBe(true);
      states.push((answer as UpdateCheck).state);
    }
    expect(states).toEqual(["up_to_date", "available", "available", "unknown"]);
    const methods = await Promise.all(SAMPLE_UPDATES.map(async (update) => (await createSampleServer({ update })("check_update")) as UpdateCheck));
    expect(manualReason(methods[1] ?? null, null)).toBeNull();
    expect(manualReason(methods[2] ?? null, null)).toBe("no_brew");
  });
  it("words the unknown check in Japanese", async () => {
    const answer = (await createSampleServer({ update: "unknown" })("check_update")) as UpdateCheck;
    expect(statusText(answer, false)).toBe("確認できません。GitHub に断られました（回数の上限の可能性があります。HTTP 403）");
    expect(statusText(answer, false)).not.toMatch(/refused|rate limit/);
  });
  it("answers an update by Homebrew with a finished update that kept a note on the tap", async () => {
    const answer = await createSampleServer({ update: "available" })("run_update");
    expect(isUpdateOutcome(answer)).toBe(true);
    expect(answer).toMatchObject({ state: "finished", ok: true });
    expect(finishedText(answer as Finished)).toContain("unagents/tap");
  });
  it("answers an update with the manual way", async () => {
    const answer = await createSampleServer()("run_update");
    expect(isUpdateOutcome(answer)).toBe(true);
    expect(answer).toMatchObject({ state: "manual", command: MANUAL_COMMAND });
  });
});
