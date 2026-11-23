<script setup lang="ts">
import { BAND_SECONDS, bandText } from "~/utils/notifications";

// The strip that shows a new notification for a few seconds. It fetches the list when the screen
// changes (the notification screen fetches for itself), never on a timer.
const route = useRoute();
const { connected } = useConnection();
const { bands, refresh, dismiss } = useNotificationFeed();

watch(
  () => [route.path, connected.value !== null] as const,
  ([path, isConnected]) => {
    if (isConnected && path !== "/notifications") void refresh();
  },
  { immediate: true },
);

const timers = new Map<number, ReturnType<typeof setTimeout>>();
watch(
  bands,
  (list) => {
    for (const n of list) {
      if (!timers.has(n.id)) timers.set(n.id, setTimeout(() => { timers.delete(n.id); dismiss(n.id); }, BAND_SECONDS * 1000));
    }
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
});
</script>

<template>
  <div class="pointer-events-none fixed inset-x-0 top-14 z-50 flex flex-col items-center gap-2 px-6" data-testid="notification-bands">
    <div
      v-for="n in bands"
      :key="n.id"
      class="pointer-events-auto flex w-full max-w-xl items-center gap-3 rounded-lg border border-default bg-elevated/80 px-4 py-2 text-sm opacity-90 shadow-lg backdrop-blur"
      role="status"
      data-testid="notification-band"
    >
      <span class="shrink-0 font-mono text-xs text-muted">{{ bandText(n).source }}</span>
      <span class="min-w-0 flex-1 truncate text-default">{{ bandText(n).summary }}</span>
      <NuxtLink v-if="n.request_id" :to="`/requests/${n.request_id}`" class="shrink-0 text-xs text-primary underline">依頼へ</NuxtLink>
      <button type="button" class="shrink-0 text-xs text-muted hover:text-default" aria-label="閉じる" @click="dismiss(n.id)">×</button>
    </div>
  </div>
</template>
