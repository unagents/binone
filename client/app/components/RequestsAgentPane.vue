<script setup lang="ts">
import { APPLY_HELD_TEXT, WAITING_TEXT, canSend, type Chat } from "~/utils/assist";

// The right side of the request screens: the agent's chat. Only the registration screen has a
// conversation (it passes `chat`); the others show the frame and say it is not connected.
const props = defineProps<{
  title: string;
  chat?: Chat;
  /** Whether a message may be sent now (the form can be edited). */
  enabled?: boolean;
}>();
const emit = defineEmits<{ send: [text: string]; applyHeld: [lineId: number] }>();

const text = ref("");
const listEl = ref<HTMLElement | null>(null);
const allowed = computed(() => !!props.chat && canSend(props.chat, text.value, props.enabled !== false));

function onSend() {
  if (!allowed.value) return;
  emit("send", text.value);
  text.value = "";
}

function onKeydown(event: KeyboardEvent) {
  // Enter sends; Shift+Enter is a new line. Not while a Japanese conversion is under way.
  if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  onSend();
}

watch(
  () => [props.chat?.lines.length, props.chat?.sending],
  () => void nextTick(() => listEl.value?.scrollTo({ top: listEl.value.scrollHeight })),
);
</script>

<template>
  <aside class="flex w-72 shrink-0 flex-col border-l border-default bg-elevated/30" :aria-label="`${title}とのチャット`" data-testid="agent-pane">
    <div class="border-b border-default px-4 py-3">
      <h2 class="text-sm font-semibold text-highlighted">{{ title }}</h2>
    </div>

    <template v-if="chat">
      <div ref="listEl" class="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3" data-testid="chat-lines" aria-live="polite">
        <p v-if="!chat.lines.length && !chat.sending" class="text-xs text-muted" data-testid="chat-empty">
          今まとめている依頼を前提に答えます。「題名を短くして」「本文を整理して」のように頼むと、左の欄を書き換えます。
        </p>
        <template v-for="line in chat.lines" :key="line.id">
          <p v-if="line.kind === 'change'" class="text-center text-xs text-primary" data-testid="chat-change">{{ line.text }}</p>
          <div v-else-if="line.kind === 'held'" class="flex flex-col items-center gap-1 text-center" data-testid="chat-held">
            <p class="text-xs text-warning">{{ line.text }}</p>
            <UButton v-if="line.held" size="xs" color="warning" variant="outline" data-testid="chat-held-apply" @click="emit('applyHeld', line.id)">{{ APPLY_HELD_TEXT }}</UButton>
          </div>
          <div
            v-else
            class="max-w-full whitespace-pre-wrap break-words rounded-md px-3 py-2 text-sm"
            :class="{
              'self-end bg-primary/10 text-highlighted': line.kind === 'user',
              'self-start border border-default bg-default': line.kind === 'agent',
              'self-start border border-warning/50 bg-warning/10 text-highlighted': line.kind === 'notice',
            }"
            :data-testid="`chat-${line.kind}`"
          >
            {{ line.text }}
            <p v-if="line.detail" class="mt-1 whitespace-pre-wrap break-words text-xs text-muted" data-testid="chat-detail">{{ line.detail }}</p>
          </div>
        </template>
        <p v-if="chat.sending" class="flex items-center gap-2 text-xs text-muted" data-testid="chat-waiting">
          <UIcon name="i-lucide-loader-circle" class="size-4 animate-spin" />{{ WAITING_TEXT }}
        </p>
      </div>
      <form class="flex flex-col gap-2 border-t border-default p-3" @submit.prevent="onSend">
        <UTextarea v-model="text" :rows="3" class="w-full" placeholder="依頼について話す（Enter で送る、Shift+Enter で改行）" aria-label="エージェントへの発言" data-testid="chat-input" @keydown="onKeydown" />
        <UButton type="submit" size="sm" :disabled="!allowed" :loading="chat.sending" data-testid="chat-send">送信</UButton>
      </form>
    </template>
    <div v-else class="flex flex-1 items-center justify-center p-4 text-center">
      <div>
        <p class="text-sm text-muted" data-testid="agent-unconnected">エージェントは未接続</p>
        <p class="mt-1 text-xs text-muted">会話は依頼の登録の画面で使えます。</p>
      </div>
    </div>
  </aside>
</template>
