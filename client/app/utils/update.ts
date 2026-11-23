// The version check and the update, as the screens see them. The work is on the native side
// (`check_update`, `run_update`, `restart_app`, `open_release_page`); this holds the shapes it
// answers with and the words for them. Pure.

/** The command to give to a person when the update cannot be run from the app. */
export const MANUAL_COMMAND = "brew upgrade --cask binone";

/** The native side stops an update that takes longer than this (in minutes). */
export const UPDATE_LIMIT_MINUTES = 10;

export type ManualReason = "no_brew" | "not_installed_by_cask";

/** How a newer version can be installed from the app (found out together with the check). */
export type UpdateMethod = { kind: "brew" } | { kind: "manual"; reason: ManualReason };

export type UpdateCheck =
  | { state: "up_to_date"; current: string; latest: string }
  | { state: "available"; current: string; latest: string; method: UpdateMethod }
  | { state: "unknown"; current: string; reason: string };

export type UpdateOutcome =
  | { state: "manual"; reason: ManualReason; command: string; url: string }
  | { state: "busy" }
  | { state: "finished"; ok: boolean; code: number | null; step: string; output: string; timed_out: boolean };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Whether what the native side answered has the shape of a check. */
export function isUpdateCheck(value: unknown): value is UpdateCheck {
  if (!isRecord(value) || typeof value.current !== "string") return false;
  if (value.state === "unknown") return typeof value.reason === "string";
  if (typeof value.latest !== "string") return false;
  return value.state === "up_to_date" || (value.state === "available" && isUpdateMethod(value.method));
}

function isUpdateMethod(value: unknown): value is UpdateMethod {
  if (!isRecord(value)) return false;
  if (value.kind === "brew") return true;
  return value.kind === "manual" && (value.reason === "no_brew" || value.reason === "not_installed_by_cask");
}

/** An answer that is not a check, or a failed call, as an `unknown` check (the screen stays whole). */
export function unknownCheck(current: string, error: unknown): UpdateCheck {
  const reason = isRecord(error) && typeof error.detail === "string" ? error.detail : typeof error === "string" ? error : "the check could not be made";
  return { state: "unknown", current, reason };
}

export function isUpdateOutcome(value: unknown): value is UpdateOutcome {
  if (!isRecord(value)) return false;
  switch (value.state) {
    case "busy":
      return true;
    case "manual":
      return typeof value.command === "string" && typeof value.url === "string";
    case "finished":
      return (
        typeof value.ok === "boolean" &&
        typeof value.step === "string" &&
        typeof value.output === "string" &&
        typeof value.timed_out === "boolean"
      );
    default:
      return false;
  }
}

/** The status line of the settings screen. */
export function statusText(check: UpdateCheck | null, checking: boolean): string {
  if (checking && !check) return "確認しています…";
  if (!check) return "まだ確認していません";
  switch (check.state) {
    case "up_to_date":
      return "最新です";
    case "available":
      return "新しい版があります";
    case "unknown":
      return `確認できません（${check.reason}）`;
  }
}

/** The small mark shown on every screen: only when there is a newer version. */
export function badgeText(check: UpdateCheck | null): string | null {
  return check?.state === "available" ? `新しい版 ${check.latest}` : null;
}

/** Why the update has to be done by hand: from the check (before the button is pressed) or from the run. */
export function manualReason(check: UpdateCheck | null, outcome: UpdateOutcome | null): ManualReason | null {
  if (outcome?.state === "manual") return outcome.reason;
  if (check?.state === "available" && check.method.kind === "manual") return check.method.reason;
  return null;
}

/** Whether the "update" button is offered: a newer version, and Homebrew can install it. */
export function canRunUpdate(check: UpdateCheck | null, outcome: UpdateOutcome | null): boolean {
  if (check?.state !== "available" || check.method.kind !== "brew") return false;
  return outcome?.state !== "manual" && !(outcome?.state === "finished" && outcome.ok);
}

export function manualReasonText(reason: ManualReason): string {
  return reason === "no_brew"
    ? "Homebrew が見つからないので、アプリからは更新できません。"
    : "このアプリは Homebrew の cask（binone）で入れたものではないので、アプリからは更新できません。";
}

/** One line about how an update ended. */
export function finishedText(outcome: Extract<UpdateOutcome, { state: "finished" }>): string {
  if (outcome.ok) return "更新しました。再起動すると新しい版で立ち上がります。";
  if (outcome.timed_out) return `更新が時間切れになりました（${outcome.step}、${UPDATE_LIMIT_MINUTES} 分を超えたので止めました）。もう一度「更新」を押せます。`;
  const code = outcome.code === null ? "終了 code なし" : `終了 code ${outcome.code}`;
  return `更新に失敗しました（${outcome.step}、${code}）`;
}
