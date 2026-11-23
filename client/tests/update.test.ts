import { describe, expect, it } from "vitest";
import {
  MANUAL_COMMAND,
  badgeText,
  finishedText,
  isUpdateCheck,
  isUpdateOutcome,
  manualReasonText,
  statusText,
  unknownCheck,
  type UpdateCheck,
} from "../app/utils/update";
import { createSampleServer, SAMPLE_UPDATES } from "../app/utils/api/sample";

const available: UpdateCheck = { state: "available", current: "0.2.0", latest: "0.3.0" };
const latest: UpdateCheck = { state: "up_to_date", current: "0.2.0", latest: "0.2.0" };
const unknown: UpdateCheck = { state: "unknown", current: "0.2.0", reason: "HTTP 403" };

describe("statusText", () => {
  it("names the three states", () => {
    expect(statusText(latest, false)).toBe("最新です");
    expect(statusText(available, false)).toBe("新しい版があります");
    expect(statusText(unknown, false)).toBe("確認できません（HTTP 403）");
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
    for (const ok of [available, latest, unknown]) expect(isUpdateCheck(ok)).toBe(true);
    for (const bad of [null, "x", {}, { state: "available", current: "1" }, { state: "unknown", current: "1" }, { state: "other", current: "1", latest: "2" }]) {
      expect(isUpdateCheck(bad)).toBe(false);
    }
  });
  it("turns a failed call into an unknown check", () => {
    expect(unknownCheck("0.2.0", "boom")).toEqual({ state: "unknown", current: "0.2.0", reason: "boom" });
    expect(unknownCheck("0.2.0", { kind: "internal", detail: "no tauri" })).toMatchObject({ reason: "no tauri" });
    expect(unknownCheck("", 42)).toMatchObject({ state: "unknown" });
  });
  it("accepts an outcome and refuses the rest", () => {
    expect(isUpdateOutcome({ state: "busy" })).toBe(true);
    expect(isUpdateOutcome({ state: "manual", reason: "no_brew", command: MANUAL_COMMAND, url: "u" })).toBe(true);
    expect(isUpdateOutcome({ state: "finished", ok: true, code: 0, step: "s", output: "" })).toBe(true);
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
    expect(finishedText({ state: "finished", ok: true, code: 0, step: "s", output: "" })).toContain("再起動");
    const failed = finishedText({ state: "finished", ok: false, code: 1, step: "brew update", output: "" });
    expect(failed).toContain("終了 code 1");
    expect(failed).toContain("brew update");
    expect(finishedText({ state: "finished", ok: false, code: null, step: "s", output: "" })).toContain("終了 code なし");
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
    expect(states).toEqual(["up_to_date", "available", "unknown"]);
  });
  it("answers an update with the manual way", async () => {
    const answer = await createSampleServer()("run_update");
    expect(isUpdateOutcome(answer)).toBe(true);
    expect(answer).toMatchObject({ state: "manual", command: MANUAL_COMMAND });
  });
});
