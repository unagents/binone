import { callNative } from "~/utils/api/transport";
import { isUpdateCheck, isUpdateOutcome, unknownCheck, type UpdateCheck, type UpdateOutcome } from "~/utils/update";

// Whether the check after startup was made. It is made once; after that, only the button asks.
let checkedAtStartup = false;

/**
 * The version check and the update. One state for the whole app: the header's mark and the
 * settings screen read the same answer. All of it happens on the native side; a failure is
 * shown as an "unknown" status and never reaches the other screens or the connection.
 */
export function useUpdate() {
  const check = useState<UpdateCheck | null>("binone-update-check", () => null);
  const checking = useState<boolean>("binone-update-checking", () => false);
  const outcome = useState<UpdateOutcome | null>("binone-update-outcome", () => null);
  const updating = useState<boolean>("binone-updating", () => false);
  const actionError = useState<string | null>("binone-update-action-error", () => null);

  async function runCheck(): Promise<void> {
    if (checking.value) return;
    checking.value = true;
    try {
      const answer = await callNative<unknown>("check_update");
      check.value = isUpdateCheck(answer) ? answer : unknownCheck(check.value?.current ?? "", "the answer has an unexpected shape");
    } catch (error) {
      check.value = unknownCheck(check.value?.current ?? "", error);
    } finally {
      checking.value = false;
    }
  }

  /** Startup: the first check, once however many screens ask. Not waited for by the caller. */
  function checkAtStartup(): void {
    if (checkedAtStartup) return;
    checkedAtStartup = true;
    void runCheck();
  }

  async function update(): Promise<void> {
    if (updating.value) return;
    updating.value = true;
    outcome.value = null;
    actionError.value = null;
    try {
      const answer = await callNative<unknown>("run_update");
      outcome.value = isUpdateOutcome(answer)
        ? answer
        : { state: "finished", ok: false, code: null, step: "update", output: "the answer has an unexpected shape", timed_out: false };
    } catch (error) {
      const detail = typeof error === "string" ? error : "the update could not be run";
      outcome.value = { state: "finished", ok: false, code: null, step: "update", output: detail, timed_out: false };
    } finally {
      updating.value = false;
    }
  }

  async function restart(): Promise<void> {
    actionError.value = null;
    try {
      await callNative("restart_app");
    } catch (error) {
      actionError.value = typeof error === "string" ? error : "the app could not be restarted";
    }
  }

  async function openReleasePage(): Promise<void> {
    try {
      await callNative("open_release_page");
    } catch (error) {
      actionError.value = typeof error === "string" ? error : "the browser could not be opened";
    }
  }

  return { check, checking, outcome, updating, actionError, runCheck, checkAtStartup, update, restart, openReleasePage };
}
