<script setup lang="ts">
// The modal of "draft -> todo": handles the grabbed drafts one at a time, shows the title being
// handled, and when the server refuses one, shows why and lets the user skip it or stop.
import { describeApiError, type ApiError } from "~/utils/api/errors";
import { runSubmitBatch, summarizeBatch, type BatchEntry, type BatchItem, type StopChoice } from "~/utils/swimlaneSubmit";

const props = defineProps<{ items: BatchItem[] }>();
// `changed` is true when at least one draft became a todo (the board must be read again).
const emit = defineEmits<{ close: [changed: boolean] }>();

const api = useApi();
const open = ref(true);
const current = ref<BatchItem | null>(null);
const position = ref(0);
const stopped = ref<{ item: BatchItem; error: ApiError } | null>(null);
const result = ref<{ entries: BatchEntry[]; interrupted: boolean } | null>(null);
const changed = ref(false);
let closed = false;
let answer: ((choice: StopChoice) => void) | null = null;

function choose(choice: StopChoice) {
  const resolve = answer;
  answer = null;
  stopped.value = null;
  resolve?.(choice);
}

function close() {
  closed = true;
  choose("abort");
  emit("close", changed.value);
}

watch(open, (value) => {
  if (!value) close();
});

async function submit(item: BatchItem) {
  const request = await api.request(item.id);
  if (request.lane !== "draft") throw { kind: "conflict", detail: `いまの状態は ${request.lane} です（draft ではありません）` } satisfies ApiError;
  await api.submitRequest(item.id, request.version);
  changed.value = true;
}

onMounted(async () => {
  result.value = await runSubmitBatch(props.items, {
    submit,
    onStart: (item, index) => {
      current.value = item;
      position.value = index + 1;
    },
    onStop: (item, error) =>
      new Promise<StopChoice>((resolve) => {
        answer = resolve;
        stopped.value = { item, error };
      }),
    isClosed: () => closed,
  });
  current.value = null;
});

const summary = computed(() => (result.value ? summarizeBatch(result.value.entries) : null));
const statusLabel: Record<string, string> = { todo: "todo にしました", skipped: "飛ばしました（draft のまま）", unknown: "受け付けられたか不明です（draft のままとは限りません）", left: "draft のまま" };
</script>

<template>
  <UModal v-model:open="open" title="todo にする" :dismissible="true" :ui="{ content: 'max-w-lg' }">
    <template #body>
      <div class="flex flex-col gap-4" data-testid="submit-modal">
        <p class="text-xs text-muted">エージェントの点検は未接続のため、サーバーの検証だけ行います。</p>

        <div v-if="!result && !stopped" class="flex items-center gap-3" data-testid="submit-running">
          <UIcon name="i-lucide-loader-circle" class="size-5 animate-spin text-primary" />
          <div class="min-w-0">
            <p class="text-sm text-highlighted">{{ position }} / {{ items.length }} 件目を処理しています</p>
            <p class="truncate text-sm text-muted" data-testid="submit-current">{{ current?.title }}</p>
          </div>
        </div>

        <div v-if="stopped" class="flex flex-col gap-3 rounded-lg border border-warning/50 bg-warning/10 p-3" data-testid="submit-stopped">
          <p class="text-sm font-medium text-highlighted">「{{ stopped.item.title }}」は todo にできませんでした</p>
          <p class="text-sm text-default">{{ describeApiError(stopped.error.kind).title }}</p>
          <p class="text-xs break-words text-muted" data-testid="submit-reason">{{ stopped.error.detail }}</p>
          <p v-if="stopped.error.kind === 'outcome_unknown'" class="text-xs text-muted">自動では送り直しません。一覧を読み込み直して結果を確かめてください。</p>
          <div class="flex gap-2">
            <UButton size="sm" color="neutral" variant="outline" data-testid="submit-skip" @click="choose('skip')">この 1 件を飛ばす</UButton>
            <UButton size="sm" color="error" variant="soft" data-testid="submit-abort" @click="choose('abort')">中断</UButton>
          </div>
        </div>

        <div v-if="result && summary" class="flex flex-col gap-2" data-testid="submit-result">
          <p class="text-sm text-highlighted">
            {{ result.interrupted ? "中断しました" : "終わりました" }}: todo {{ summary.todo }} 件、飛ばした {{ summary.skipped }} 件、不明 {{ summary.unknown }} 件、draft のまま {{ summary.left }} 件
          </p>
          <ul class="divide-y divide-default rounded-lg border border-default text-sm">
            <li v-for="entry in result.entries" :key="entry.id" class="flex flex-col gap-0.5 px-3 py-2">
              <span class="truncate text-highlighted">{{ entry.title }}</span>
              <span class="text-xs text-muted">{{ statusLabel[entry.status] }}<template v-if="entry.reason">: {{ entry.reason.detail }}</template></span>
            </li>
          </ul>
        </div>
        <p v-if="!result" class="text-xs text-muted" data-testid="submit-close-note">途中で閉じると、まだ処理していない draft は draft のまま残ります。すでに todo になった物は戻りません。</p>
      </div>
    </template>
    <template #footer>
      <div class="flex w-full justify-end">
        <UButton color="neutral" variant="outline" size="sm" data-testid="submit-close" @click="open = false">{{ result ? "閉じる" : "閉じて中断" }}</UButton>
      </div>
    </template>
  </UModal>
</template>
