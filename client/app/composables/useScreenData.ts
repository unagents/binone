import { createLoader } from "~/utils/api/loader";
import type { ApiError } from "~/utils/api/errors";

/**
 * Data for a screen: fetched when the screen opens (it is mounted again on every route change),
 * and again when `reload` is called. There is no polling. Answers that arrive after a newer load
 * or after the screen was left are dropped.
 *
 *   const { data, error, loading, reload } = useScreenData(() => api.requests());
 */
export function useScreenData<T>(fetcher: () => Promise<T>, options: { immediate?: boolean } = {}) {
  const data = shallowRef<T | null>(null);
  const error = ref<ApiError | null>(null);
  const loading = ref(false);

  const loader = createLoader(fetcher, (state) => {
    data.value = state.data;
    error.value = state.error;
    loading.value = state.loading;
  });

  onMounted(() => {
    if (options.immediate !== false) void loader.load();
  });
  onBeforeUnmount(loader.dispose);

  return { data, error, loading, reload: loader.load };
}
