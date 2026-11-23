// "Load when the screen opens, again on request" without polling. A newer load makes the
// answers of older ones irrelevant, so a slow answer never overwrites a fresh one, and a screen
// that was left never gets one.
import { toApiError, type ApiError } from "~/utils/api/errors";

export interface LoaderState<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
}

export function createLoader<T>(fetcher: () => Promise<T>, onState: (state: LoaderState<T>) => void) {
  let latest = 0;
  let data: T | null = null;

  async function load(): Promise<void> {
    const mine = ++latest;
    onState({ data, error: null, loading: true });
    try {
      const value = await fetcher();
      if (mine !== latest) return;
      data = value;
      onState({ data, error: null, loading: false });
    } catch (error) {
      if (mine !== latest) return;
      onState({ data, error: toApiError(error), loading: false });
    }
  }

  /** The screen was left: answers still on the way are dropped. */
  function dispose() {
    latest++;
  }

  return { load, dispose };
}
