import type { Notification } from "~/types/binone";
import { toApiError, type ApiError } from "~/utils/api/errors";
import { newNotifications } from "~/utils/notifications";

/**
 * The server's notification list, shared by the notification screen and the band. It is fetched
 * when a screen asks (`refresh`): on opening the screen, on a screen change, and by hand. There is
 * no polling. What was new since the last fetch goes to `bands` for the band to show.
 */
export function useNotificationFeed() {
  /** `null` until the first fetch: the first fetch never counts as "new". */
  const list = useState<Notification[] | null>("binone-notifications", () => null);
  const bands = useState<Notification[]>("binone-notification-bands", () => []);
  const error = useState<ApiError | null>("binone-notification-error", () => null);
  const loading = useState<boolean>("binone-notification-loading", () => false);
  const api = useApi();

  async function refresh(): Promise<void> {
    if (loading.value) return;
    loading.value = true;
    try {
      const next = await api.notifications();
      const fresh = newNotifications(list.value, next);
      if (fresh.length > 0) bands.value = [...fresh, ...bands.value];
      list.value = next;
      error.value = null;
    } catch (e) {
      error.value = toApiError(e);
    } finally {
      loading.value = false;
    }
  }

  function dismiss(id: number) {
    bands.value = bands.value.filter((n) => n.id !== id);
  }

  return { list, bands, error, loading, refresh, dismiss };
}
