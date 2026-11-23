<script setup lang="ts">
// The modal of "draft -> todo". It checks the grabbed drafts one at a time (a dry run that changes
// nothing) and shows the title being checked. A draft that does not pass stops it with the reason;
// the user fixes it elsewhere and checks again, or aborts. When every draft has passed, one call
// turns them all into todos. Until that call is made, closing the modal changes nothing; from then
// on it cannot be closed until the answer is shown, and the board is read again after that.
import { describeApiError } from "~/utils/api/errors";
import type { SubmitBatch } from "~/types/binone";
import { requestRoute } from "~/utils/routes";
import {
  describeEffect,
  describeMissing,
  describeSubmitCode,
  titleOf,
  runSubmitFlow,
  type BatchItem,
  type StopChoice,
  type StopProblem,
  type SubmitFlowOutcome,
} from "~/utils/swimlaneSubmit";

const props = defineProps<{ items: BatchItem[] }>();
const emit = defineEmits<{
  close: [];
  /** A call that may have changed something was made and answered: read the board again. */
  settled: [];
}>();

const api = useApi();
const open = ref(true);
const phase = ref<"checking" | "stopped" | "submitting" | "done">("checking");
const current = ref<BatchItem | null>(null);
const position = ref(0);
const stopped = ref<StopProblem | null>(null);
const outcome = ref<SubmitFlowOutcome | null>(null);
let closed = false;
let answer: ((choice: StopChoice) => void) | null = null;

function choose(choice: StopChoice) {
  const resolve = answer;
  answer = null;
  stopped.value = null;
  if (choice === "retry") phase.value = "checking";
  resolve?.(choice);
}

function close() {
  closed = true;
  choose("abort");
  emit("close");
}

watch(open, (value) => {
  if (!value) close();
});
// A way out of the screen (the sidebar) is refused while the call is in flight: its answer is shown here.
onBeforeRouteLeave(() => phase.value !== "submitting");
onBeforeUnmount(() => {
  closed = true;
  answer?.("abort");
});

/** The list is read again: the versions and titles as they are now. A draft not in it keeps what it had. */
async function reread(items: readonly BatchItem[]): Promise<BatchItem[]> {
  const rows = new Map((await api.requests({})).map((row) => [row.id, row]));
  return items.map((item) => {
    const row = rows.get(item.id);
    return row ? { id: item.id, title: row.title, version: row.version } : item;
  });
}

onMounted(async () => {
  outcome.value = await runSubmitFlow(props.items, {
    check: (item) => api.checkSubmitRequest(item.id, item.version),
    submit: (items): Promise<SubmitBatch> => api.submitRequests(items.map(({ id, version }) => ({ id, version }))),
    reread,
    onChecking: (item, index) => {
      current.value = item;
      position.value = index;
      phase.value = "checking";
    },
    onSubmitting: () => {
      phase.value = "submitting";
    },
    onStop: (problem) =>
      new Promise<StopChoice>((resolve) => {
        answer = resolve;
        stopped.value = problem;
        phase.value = "stopped";
      }),
    isClosed: () => closed,
  });
  current.value = null;
  if (outcome.value.status === "left") {
    // Nothing was called that could change anything; the modal is already closed or is closed now.
    if (!closed) close();
    return;
  }
  phase.value = "done";
  // The answer is on screen first; then the board is read again.
  await nextTick();
  emit("settled");
});

const refusal = computed(() => (stopped.value?.kind === "refused" ? stopped.value.check : null));
const failure = computed(() => (stopped.value?.kind === "batch" ? stopped.value.failure : null));
const errorText = computed(() => {
  const o = outcome.value;
  if (stopped.value?.kind === "error") return stopped.value.error;
  return o && (o.status === "failed" || o.status === "unknown") ? o.error : null;
});
const count = computed(() => (outcome.value?.status === "submitted" ? outcome.value.batch.requests.length || props.items.length : 0));
/** The draft to open in the editor: the one that stopped the check, or the first cause of a failed batch. */
const editTarget = computed(() => {
  const s = stopped.value;
  if (!s) return null;
  return s.kind === "batch" ? (s.failure.causes[0]?.item ?? null) : s.item;
});
const canClose = computed(() => phase.value !== "submitting");

function openEditor(item: BatchItem) {
  // Leaving this screen closes the modal; nothing has been changed yet.
  void navigateTo(requestRoute({ id: item.id, lane: "draft" }));
}
</script>

<template>
  <UModal v-model:open="open" title="todo にする" :dismissible="canClose" :close="canClose" :ui="{ content: 'max-w-lg' }">
    <template #body>
      <div class="flex flex-col gap-4" data-testid="submit-modal">
        <p class="text-xs text-muted">エージェントの点検は未接続のため、サーバーの検証だけ行います。</p>

        <div v-if="phase === 'checking'" class="flex items-center gap-3" data-testid="submit-running">
          <UIcon name="i-lucide-loader-circle" class="size-5 animate-spin text-primary" />
          <div class="min-w-0">
            <p class="text-sm text-highlighted">{{ position }} / {{ items.length }} 件目を確かめています</p>
            <p class="truncate text-sm text-muted" data-testid="submit-current">{{ current?.title }}</p>
          </div>
        </div>

        <div v-if="phase === 'submitting'" class="flex items-center gap-3" data-testid="submit-sending">
          <UIcon name="i-lucide-loader-circle" class="size-5 animate-spin text-primary" />
          <div class="min-w-0">
            <p class="text-sm text-highlighted">{{ items.length }} 件を todo にしています</p>
            <p class="text-xs text-muted">答えが返るまで、この画面は閉じられません。</p>
          </div>
        </div>

        <div v-if="stopped" class="flex flex-col gap-3 rounded-lg border border-warning/50 bg-warning/10 p-3" data-testid="submit-stopped">
          <template v-if="refusal && stopped.kind === 'refused'">
            <p class="text-sm font-medium text-highlighted">「{{ stopped.item.title || "（題名なし）" }}」は todo にできません</p>
            <p class="text-sm text-default" data-testid="submit-code"><code class="rounded bg-elevated px-1 text-xs">{{ refusal.code }}</code> {{ describeSubmitCode(refusal.code) }}</p>
            <p v-if="refusal.missing.length" class="text-sm text-default" data-testid="submit-missing">足りない項目: {{ describeMissing(refusal.missing).join("、") }}</p>
            <p v-if="refusal.message" class="text-xs break-words text-muted" data-testid="submit-reason">{{ refusal.message }}</p>
          </template>
          <template v-else-if="stopped.kind === 'error'">
            <p class="text-sm font-medium text-highlighted">{{ stopped.item ? `「${stopped.item.title || "（題名なし）"}」を確かめられませんでした` : "読み込み直せませんでした" }}</p>
            <p class="text-sm text-default">{{ describeApiError(stopped.error.kind).title }}</p>
            <p class="text-xs break-words text-muted" data-testid="submit-reason">{{ stopped.error.detail }}</p>
          </template>
          <template v-else-if="failure">
            <p class="text-sm font-medium text-highlighted">todo にできませんでした</p>
            <div v-for="cause in failure.causes" :key="cause.item.id" class="flex flex-col gap-1" data-testid="submit-cause">
              <p class="text-sm text-highlighted">原因: 「{{ titleOf(cause.item) }}」</p>
              <p class="text-sm text-default"><code class="rounded bg-elevated px-1 text-xs">{{ cause.check.code }}</code> {{ describeSubmitCode(cause.check.code) }}</p>
              <p v-if="cause.check.missing.length" class="text-sm text-default">足りない項目: {{ describeMissing(cause.check.missing).join("、") }}</p>
              <p v-if="cause.check.message" class="text-xs break-words text-muted">{{ cause.check.message }}</p>
            </div>
            <ul v-if="failure.effects.length" class="flex flex-col gap-1 text-sm text-default" data-testid="submit-effects">
              <li v-for="effect in failure.effects" :key="effect.item.id" :data-testid="`submit-effect-${effect.effect}`">「{{ titleOf(effect.item) }}」: {{ describeEffect(effect.effect) }}</li>
            </ul>
          </template>
          <p v-if="!failure || failure.nothingChanged" class="text-xs text-muted">この間も、何も変わっていません（全て draft のままです）。直してから「もう一度確かめる」を押すと、最新の内容で最初から確かめます。</p>
          <p v-else class="text-sm font-medium text-error" data-testid="submit-stuck">次の件は openspec-mcp 側で open のまま戻せませんでした: {{ failure.stuck.map((i) => `「${titleOf(i)}」`).join("") }}。binone の側は全て draft のままです。openspec-mcp の側を確かめてください。</p>
          <div class="flex flex-wrap gap-2">
            <UButton size="sm" data-testid="submit-retry" @click="choose('retry')">もう一度確かめる</UButton>
            <UButton v-if="editTarget" size="sm" color="neutral" variant="outline" data-testid="submit-open-editor" @click="openEditor(editTarget)">登録の画面を開く（この画面は閉じます）</UButton>
            <UButton size="sm" color="error" variant="soft" data-testid="submit-abort" @click="choose('abort')">中断</UButton>
          </div>
        </div>

        <div v-if="phase === 'done' && outcome" class="flex flex-col gap-2" data-testid="submit-result">
          <p v-if="outcome.status === 'submitted'" class="text-sm text-highlighted" data-testid="submit-success">{{ count }} 件を todo にしました。</p>
          <template v-else-if="errorText">
            <p class="text-sm font-medium text-highlighted" data-testid="submit-failure">
              {{ outcome.status === "unknown" ? "受け付けられたか分かりません" : "todo にできませんでした" }}
            </p>
            <p class="text-sm text-default">{{ describeApiError(errorText.kind).title }}</p>
            <p class="text-xs break-words text-muted" data-testid="submit-reason">{{ errorText.detail }}</p>
            <p v-if="outcome.status === 'unknown'" class="text-xs text-muted">自動では送り直しません。盤面を読み直しているので、draft か todo かを見て確かめてください。</p>
            <p v-else class="text-xs text-muted">盤面を読み直しています。変わっていない物は draft のままです。</p>
          </template>
        </div>
      </div>
    </template>
    <template #footer>
      <div class="flex w-full justify-end">
        <UButton color="neutral" variant="outline" size="sm" :disabled="!canClose" data-testid="submit-close" @click="open = false">{{ phase === "done" ? "閉じる" : "閉じて中断" }}</UButton>
      </div>
    </template>
  </UModal>
</template>
