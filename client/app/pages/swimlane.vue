<script setup lang="ts">
import type { BatchItem } from "~/utils/swimlaneSubmit";
import type { RequestSummary } from "~/types/binone";
import {
  DEFAULT_VIEW,
  EMPTY_FILTER,
  buildBoard,
  cardRoute,
  filterChoices,
  isFiltering,
  normalizeHidden,
  parseView,
  type SwimlaneFilter,
  type SwimlaneGroupBy,
  type SwimlaneOrder,
} from "~/utils/swimlane";
import { canDrop, grabbedIds, toggleSelected } from "~/utils/swimlaneSubmit";

const api = useApi();
const router = useRouter();
// The lanes come from the settings; the cards are read whole and filtered here (the rules live
// in utils/swimlane.ts), so hiding a lane or changing a filter never asks the server again.
const { data, error, loading, reload } = useScreenData(async () => ({ lanes: (await api.settings()).settings.lanes, items: await api.requests({}) }));

const VIEW_KEY = "binone.swimlane.view";
const view = reactive({ ...DEFAULT_VIEW, hidden: [] as string[] });
const filter = reactive<SwimlaneFilter>({ ...EMPTY_FILTER, kinds: [], tags: [] });
const selected = ref<string[]>([]);
const batch = ref<BatchItem[] | null>(null);
const dragging = ref<string[]>([]);
const overLane = ref<string | null>(null);

onMounted(() => {
  try {
    Object.assign(view, parseView(localStorage.getItem(VIEW_KEY)));
  } catch {
    /* storage is unavailable: keep the defaults */
  }
});
watch(view, () => {
  try {
    localStorage.setItem(VIEW_KEY, JSON.stringify(view));
  } catch {
    /* not kept; the view still works */
  }
});

const lanes = computed(() => data.value?.lanes ?? []);
const items = computed(() => data.value?.items ?? []);
const board = computed(() => buildBoard(items.value, lanes.value, { filter, hidden: view.hidden, order: view.order, groupBy: view.groupBy }));
const hidden = computed(() => normalizeHidden(lanes.value, view.hidden));
const projectChoices = computed(() => ["", ...filterChoices(items.value, "project")].map((v) => ({ label: v === "" ? "すべて" : v, value: v })));
const kindChoices = computed(() => filterChoices(items.value, "kind"));
const tagChoices = computed(() => filterChoices(items.value, "tags"));
const screenDrafts = computed(() => board.value.columns.filter((c) => c.lane === "draft").flatMap((c) => c.groups.flatMap((g) => g.items.map((i) => i.id))));
const selectedDrafts = computed(() => selected.value.filter((id) => screenDrafts.value.includes(id)));

const orders: { label: string; value: SwimlaneOrder }[] = [
  { label: "更新日時（新しい順）", value: "UPDATED_DESC" },
  { label: "更新日時（古い順）", value: "UPDATED_ASC" },
  { label: "プロジェクト名（昇順）", value: "PROJECT_ASC" },
  { label: "プロジェクト名（降順）", value: "PROJECT_DESC" },
];
const groups: { label: string; value: SwimlaneGroupBy }[] = [
  { label: "グループなし", value: "none" },
  { label: "プロジェクト別", value: "project" },
  { label: "種別別", value: "kind" },
];

function toggleLane(name: string) {
  view.hidden = hidden.value.includes(name) ? hidden.value.filter((n) => n !== name) : [...hidden.value, name];
}
function toggleIn(list: string[], value: string) {
  const index = list.indexOf(value);
  if (index >= 0) list.splice(index, 1);
  else list.push(value);
}
function clearFilter() {
  filter.project = null;
  filter.kinds = [];
  filter.tags = [];
}

function formatTime(ms: number) {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function press(item: RequestSummary, event: MouseEvent) {
  if (event.shiftKey && item.lane === "draft") {
    selected.value = toggleSelected(selected.value, item.id);
    return;
  }
  void router.push(cardRoute(item));
}

function dragStart(item: RequestSummary, event: DragEvent) {
  dragging.value = grabbedIds(selectedDrafts.value, item.id, screenDrafts.value);
  event.dataTransfer?.setData("text/plain", dragging.value.join(","));
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}
function dragEnd() {
  dragging.value = [];
  overLane.value = null;
}
const dragLanes = computed(() => dragging.value.map(() => "draft"));
function dragOver(lane: string, event: DragEvent) {
  if (!canDrop(dragLanes.value, lane)) return;
  event.preventDefault();
  overLane.value = lane;
}
function drop(lane: string) {
  const ids = dragging.value;
  dragEnd();
  if (canDrop(ids.map(() => "draft"), lane)) startBatch(ids);
}

function startBatch(ids: string[]) {
  const byId = new Map(items.value.map((item) => [item.id, item]));
  const list = ids.flatMap((id) => {
    const item = byId.get(id);
    return item ? [{ id, title: item.title }] : [];
  });
  if (list.length > 0) batch.value = list;
}
async function batchClosed(changed: boolean) {
  batch.value = null;
  selected.value = [];
  if (changed) await reload();
}
</script>

<template>
  <ScreenFrame title="スイムレーン" description="レーン別の依頼。手で動かせるのは draft から todo だけです。" wide>
    <template #actions>
      <UPopover>
        <UButton color="neutral" variant="outline" size="sm" icon="i-lucide-columns-3" data-testid="lanes-button">
          表示レーン<span v-if="hidden.length" class="ml-1 rounded-full bg-primary px-1.5 text-[10px] text-inverted">{{ hidden.length }}</span>
        </UButton>
        <template #content>
          <div class="flex min-w-44 flex-col p-1" data-testid="lanes-menu">
            <label v-for="(lane, index) in lanes" :key="lane.name" class="flex items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-elevated" :class="index === 0 ? 'opacity-50' : 'cursor-pointer'">
              <input type="checkbox" :disabled="index === 0" :checked="index === 0 || !hidden.includes(lane.name)" :data-testid="`lane-toggle-${lane.name}`" @change="toggleLane(lane.name)" >
              {{ lane.label || lane.name }}
            </label>
          </div>
        </template>
      </UPopover>
      <UButton color="neutral" variant="outline" size="sm" :loading="loading" @click="reload">読み込み直す</UButton>
    </template>

    <div class="flex flex-wrap items-center gap-x-4 gap-y-3" data-testid="swimlane-toolbar">
      <label class="flex items-center gap-2 text-sm text-muted">
        プロジェクト
        <USelect :model-value="filter.project ?? ''" :items="projectChoices" size="sm" class="w-40" data-testid="filter-project" @update:model-value="(v: string) => (filter.project = v === '' ? null : v)" />
      </label>
      <UPopover>
        <UButton color="neutral" variant="outline" size="sm" data-testid="filter-kinds-button">種別<span v-if="filter.kinds.length" class="ml-1 rounded-full bg-primary px-1.5 text-[10px] text-inverted">{{ filter.kinds.length }}</span></UButton>
        <template #content>
          <div class="flex min-w-40 flex-col p-1">
            <label v-for="kind in kindChoices" :key="kind" class="flex cursor-pointer items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-elevated">
              <input type="checkbox" :checked="filter.kinds.includes(kind)" :data-testid="`filter-kind-${kind}`" @change="toggleIn(filter.kinds, kind)" >{{ kind }}
            </label>
            <p v-if="kindChoices.length === 0" class="px-3 py-2 text-xs text-muted">種別のある依頼がありません。</p>
          </div>
        </template>
      </UPopover>
      <UPopover>
        <UButton color="neutral" variant="outline" size="sm" data-testid="filter-tags-button">タグ<span v-if="filter.tags.length" class="ml-1 rounded-full bg-primary px-1.5 text-[10px] text-inverted">{{ filter.tags.length }}</span></UButton>
        <template #content>
          <div class="flex min-w-40 flex-col p-1">
            <label v-for="tag in tagChoices" :key="tag" class="flex cursor-pointer items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-elevated">
              <input type="checkbox" :checked="filter.tags.includes(tag)" :data-testid="`filter-tag-${tag}`" @change="toggleIn(filter.tags, tag)" >{{ tag }}
            </label>
            <p v-if="tagChoices.length === 0" class="px-3 py-2 text-xs text-muted">タグのある依頼がありません。</p>
          </div>
        </template>
      </UPopover>
      <UButton v-if="isFiltering(filter)" color="neutral" variant="ghost" size="sm" data-testid="filter-clear" @click="clearFilter">絞り込みを解除</UButton>
      <div class="flex-1" />
      <USelect v-model="view.groupBy" :items="groups" size="sm" class="w-36" aria-label="グループ表示" data-testid="group-by" />
      <USelect v-model="view.order" :items="orders" size="sm" class="w-52" aria-label="並べ替え" data-testid="order-by" />
    </div>

    <div v-if="selectedDrafts.length > 0" class="flex items-center gap-3 rounded-lg border border-primary/40 bg-primary/10 px-3 py-2 text-sm" data-testid="selection-bar">
      <span class="text-highlighted">draft を {{ selectedDrafts.length }} 件選んでいます</span>
      <UButton size="xs" data-testid="selection-submit" @click="startBatch(selectedDrafts)">todo にする</UButton>
      <UButton size="xs" color="neutral" variant="ghost" @click="selected = []">選択を解除</UButton>
    </div>
    <p v-else class="text-xs text-muted">draft のカードは Shift を押しながら押すと複数選べます。カードを todo のレーンへドラッグして todo にします。todo を隠している時は、選んだ draft の「todo にする」ボタンで todo 化します。</p>

    <p v-if="loading && !data" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
    <ApiErrorAlert v-if="error" :error="error" />
    <p v-if="board.orphans.length" class="text-xs text-warning" data-testid="orphans">設定にないレーンの依頼が {{ board.orphans.length }} 件あり、表示していません。</p>

    <div v-if="data" class="flex min-h-[420px] gap-5 overflow-x-auto pb-2" data-testid="board">
      <section
        v-for="column in board.columns"
        :key="column.lane"
        class="flex max-h-[calc(100vh-18rem)] min-h-40 w-72 shrink-0 flex-col rounded-xl border bg-default/60"
        :class="overLane === column.lane ? 'border-primary' : 'border-default'"
        :data-lane="column.lane"
        @dragover="dragOver(column.lane, $event)"
        @dragleave="overLane = null"
        @drop.prevent="drop(column.lane)"
      >
        <header class="flex items-center gap-2 border-b border-default px-4 py-3">
          <h2 class="min-w-0 truncate text-sm font-semibold text-highlighted">{{ column.label }}</h2>
          <span class="ml-auto flex size-5 items-center justify-center rounded-md bg-elevated text-xs text-muted" data-testid="lane-count">{{ column.count }}</span>
          <UButton v-if="column.lane !== 'draft'" color="neutral" variant="ghost" size="xs" icon="i-lucide-eye-off" :aria-label="`${column.label}を隠す`" @click="toggleLane(column.lane)" />
        </header>
        <div v-if="column.lane === 'draft'" class="px-3 pt-3">
          <UButton block color="neutral" variant="outline" size="xs" icon="i-lucide-plus" class="border-dashed" aria-label="新しい依頼を登録" data-testid="add-request" to="/requests/new" />
        </div>
        <div class="flex min-h-24 flex-1 flex-col gap-2 overflow-y-auto p-3">
          <div v-for="group in column.groups" :key="group.key" class="flex flex-col gap-2">
            <p v-if="view.groupBy !== 'none'" class="px-0.5 text-xs font-medium tracking-wide text-muted" data-testid="group-head">{{ group.label }}</p>
            <article
              v-for="item in group.items"
              :key="item.id"
              class="cursor-pointer rounded-xl border bg-elevated/80 p-3.5 transition-colors select-none hover:bg-elevated"
              :class="selected.includes(item.id) ? 'border-primary' : 'border-default/60'"
              :draggable="item.lane === 'draft'"
              :data-card="item.id"
              tabindex="0"
              @click="press(item, $event)"
              @keydown.enter="router.push(cardRoute(item))"
              @dragstart="dragStart(item, $event)"
              @dragend="dragEnd"
            >
              <p class="line-clamp-2 text-sm font-medium text-highlighted">{{ item.title || "（題名なし）" }}</p>
              <p class="mt-2 flex items-center gap-1 truncate text-xs text-muted"><UIcon name="i-lucide-folder" class="size-3" />{{ item.project || "（プロジェクト未設定）" }}<span v-if="item.kind"> ・ {{ item.kind }}</span></p>
              <p v-if="item.currentStep" class="mt-1 flex items-center gap-1 truncate text-xs text-muted"><UIcon name="i-lucide-footprints" class="size-3" />{{ item.currentStep }}</p>
              <div v-if="item.tags.length" class="mt-2 flex flex-wrap gap-1"><span v-for="tag in item.tags" :key="tag" class="max-w-36 truncate rounded-full border border-primary/25 bg-primary/10 px-2 text-xs text-primary">{{ tag }}</span></div>
              <p class="mt-2 flex items-center gap-1 text-xs text-muted"><UIcon name="i-lucide-clock" class="size-3" />{{ formatTime(item.updatedMs) }}<span v-if="item.lane !== column.lane" class="ml-auto truncate">（{{ item.lane }}）</span></p>
            </article>
          </div>
          <p v-if="column.count === 0" class="py-4 text-center text-xs text-muted">依頼はありません</p>
        </div>
      </section>
    </div>

    <UnsupportedNotice what="draft 以外の移動、レーンの追加" missing="レーンを動かすサーバーの操作（draft→todo の submitRequest だけある）。レーンの追加は設定の画面" />
    <UnsupportedNotice what="エージェントによる点検と質疑" missing="draft の問題をエージェントが調べ、モーダルで質疑する仕組み（サーバー・エージェントの接続）。今はサーバーの検証だけ行います" />

    <SwimlaneSubmitModal v-if="batch" :items="batch" @close="batchClosed" />
  </ScreenFrame>
</template>
