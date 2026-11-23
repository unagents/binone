<script setup lang="ts">
// The right side of the request screens: the agent's chat. The conversation is not built yet, so
// the frame says so rather than showing a chat that does nothing.
defineProps<{ title: string }>();
</script>

<template>
  <aside class="flex w-72 shrink-0 flex-col border-l border-default bg-elevated/30" :aria-label="`${title}とのチャット`" data-testid="agent-pane">
    <div class="border-b border-default px-4 py-3">
      <h2 class="text-sm font-semibold text-highlighted">{{ title }}</h2>
    </div>
    <div class="flex flex-1 items-center justify-center p-4 text-center">
      <div>
        <p class="text-sm text-muted" data-testid="agent-unconnected">エージェントは未接続</p>
        <p class="mt-1 text-xs text-muted">会話はまだ使えません。</p>
      </div>
    </div>
  </aside>
</template>
