<script setup lang="ts">
import { formatDateTime, relativeTime } from "~/utils/requests";

const { list, error, loading, refresh } = useNotificationFeed();
onMounted(() => void refresh());
const now = Date.now();
</script>

<template>
  <ScreenFrame title="通知" description="bantou のセッションからの質問や判断の依頼。新しい物は、画面を切り替えた時と読み込み直した時に帯で知らせます。">
    <template #actions>
      <UButton color="neutral" variant="outline" size="sm" :loading="loading" data-testid="reload" @click="refresh">読み込み直す</UButton>
    </template>

    <p v-if="loading && !list" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
    <ApiErrorAlert v-if="error" :error="error" />
    <ul v-if="list && list.length > 0" class="divide-y divide-default rounded-lg border border-default" data-testid="notifications">
      <li v-for="n in list" :key="n.id" class="flex items-start gap-3 px-4 py-3" data-testid="notification">
        <div class="min-w-0 flex-1">
          <p class="text-sm text-highlighted">{{ n.summary }}</p>
          <p class="mt-0.5 text-xs text-muted">
            <span class="font-mono">{{ n.source_session ?? "binone" }}</span>
            ・ {{ relativeTime(n.at_ms, now) }}（{{ formatDateTime(n.at_ms) }}）
          </p>
        </div>
        <UButton v-if="n.request_id" size="xs" color="neutral" variant="outline" :to="`/requests/${n.request_id}`">依頼を開く</UButton>
      </li>
    </ul>
    <p v-else-if="list && !error" class="rounded-lg border border-default p-6 text-center text-sm text-muted" data-testid="notifications-empty">通知はまだありません。</p>

  </ScreenFrame>
</template>
