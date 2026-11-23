<script setup lang="ts">
import type { ApiError } from "~/utils/api/errors";
import { toApiError } from "~/utils/api/errors";
import type { LaneDef, SettingsView } from "~/types/binone";
import { MANUAL_COMMAND, finishedText, manualReasonText, statusText } from "~/utils/update";
import { canMoveLane, isStandardLane, laneLabel, moveItem, validateKind, validateLaneName } from "~/utils/settings";

const { check, checking, outcome, updating, actionError, runCheck, update, restart, openReleasePage } = useUpdate();
const finished = computed(() => (outcome.value?.state === "finished" ? outcome.value : null));
const updated = computed(() => finished.value?.ok === true);
const canUpdate = computed(() => check.value?.state === "available" && !updated.value);

const api = useApi();
const { data, error, loading, reload } = useScreenData(() => api.settings());

// The working copy of the two lists. The rest of the settings is sent back as it was read.
const lanes = ref<LaneDef[]>([]);
const kinds = ref<string[]>([]);
watch(
  data,
  (view) => {
    if (!view) return;
    lanes.value = view.settings.lanes.map((lane) => ({ ...lane }));
    kinds.value = [...view.settings.kinds];
  },
  { immediate: true },
);

const dirty = computed(() => {
  const view = data.value;
  if (!view) return false;
  return JSON.stringify([lanes.value, kinds.value]) !== JSON.stringify([view.settings.lanes, view.settings.kinds]);
});

const newLane = ref("");
const newLaneLabel = ref("");
const newLaneError = ref<string>();
function addLane() {
  newLaneError.value = validateLaneName(newLane.value.trim(), lanes.value) ?? undefined;
  if (newLaneError.value) return;
  const label = newLaneLabel.value.trim();
  lanes.value = [...lanes.value, { name: newLane.value.trim(), label: label === "" ? null : label }];
  newLane.value = "";
  newLaneLabel.value = "";
}

const newKind = ref("");
const newKindError = ref<string>();
function addKind() {
  newKindError.value = validateKind(newKind.value.trim(), kinds.value) ?? undefined;
  if (newKindError.value) return;
  kinds.value = [...kinds.value, newKind.value.trim()];
  newKind.value = "";
}

const saving = ref(false);
const saveError = ref<ApiError | null>(null);
const saved = ref(false);
async function save() {
  const view = data.value;
  if (!view || saving.value) return;
  saving.value = true;
  saveError.value = null;
  saved.value = false;
  try {
    const next: SettingsView = await api.updateSettings({ ...view.settings, lanes: lanes.value, kinds: kinds.value });
    data.value = next;
    saved.value = true;
  } catch (e) {
    saveError.value = toApiError(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <ScreenFrame title="設定" description="レーンと種別の並びと追加。接続先の変更は、上の接続の表示から。">
    <template #actions>
      <UButton color="neutral" variant="outline" size="sm" to="/projects">プロジェクト</UButton>
      <UButton color="neutral" variant="outline" size="sm" :loading="loading" @click="reload">読み込み直す</UButton>
    </template>

    <UCard id="version" class="scroll-mt-4" data-testid="version-card">
      <template #header>
        <div class="flex items-center justify-between gap-3">
          <h2 class="text-sm font-medium text-highlighted">アプリの版</h2>
          <UButton size="xs" color="neutral" variant="outline" :loading="checking" data-testid="check-update" @click="runCheck">確認</UButton>
        </div>
      </template>
      <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm" data-testid="version-info">
        <dt class="text-muted">今の版</dt><dd class="font-mono" data-testid="version-current">{{ check?.current || "—" }}</dd>
        <dt class="text-muted">最新の版</dt><dd class="font-mono" data-testid="version-latest">{{ check && check.state !== "unknown" ? check.latest : "—" }}</dd>
        <dt class="text-muted">状態</dt>
        <dd data-testid="version-status" :data-state="check?.state ?? 'none'" :class="check?.state === 'available' ? 'text-primary' : check?.state === 'unknown' ? 'text-warning' : ''">{{ statusText(check, checking) }}</dd>
      </dl>

      <div v-if="canUpdate" class="mt-3 flex items-center gap-3">
        <UButton size="sm" :loading="updating" :disabled="updating" data-testid="run-update" @click="update">更新</UButton>
        <span v-if="updating" class="text-xs text-muted" data-testid="update-progress">更新しています（brew update、brew upgrade）。そのままお待ちください。</span>
      </div>

      <div v-if="outcome?.state === 'manual'" class="mt-3 space-y-2 text-sm" data-testid="update-manual">
        <p class="text-muted">{{ manualReasonText(outcome.reason) }}</p>
        <div class="flex flex-wrap items-center gap-3">
          <UButton size="sm" color="neutral" variant="outline" data-testid="open-release" @click="openReleasePage">リリースのページを開く</UButton>
          <span class="text-xs text-muted">または、Homebrew で入れた Mac で次を実行:</span>
          <code class="rounded-md bg-muted px-2 py-1 font-mono text-xs" data-testid="manual-command">{{ MANUAL_COMMAND }}</code>
        </div>
      </div>
      <p v-else-if="outcome?.state === 'busy'" class="mt-3 text-xs text-muted">別の更新が動いています。</p>

      <div v-if="finished" class="mt-3 space-y-2" data-testid="update-result" :data-ok="finished.ok">
        <p class="text-sm" :class="finished.ok ? 'text-success' : 'text-error'">{{ finishedText(finished) }}</p>
        <pre v-if="finished.output" class="max-h-48 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap" data-testid="update-output">{{ finished.output }}</pre>
        <UButton v-if="finished.ok" size="sm" data-testid="restart-app" @click="restart">再起動</UButton>
      </div>
      <p v-if="actionError" class="mt-2 text-xs text-error" data-testid="update-action-error">{{ actionError }}</p>
    </UCard>

    <p v-if="loading && !data" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
    <ApiErrorAlert v-if="error" :error="error" />

    <template v-if="data">
      <div class="grid gap-4 md:grid-cols-2">
        <UCard data-testid="lanes-card">
          <template #header><h2 class="text-sm font-medium text-highlighted">レーン（状態）</h2></template>
          <ol class="space-y-1" data-testid="lanes">
            <li v-for="(lane, index) in lanes" :key="lane.name" class="flex items-center gap-2 rounded-md bg-muted px-3 py-1.5 text-sm" data-testid="lane">
              <span class="min-w-0 flex-1 truncate"><span class="text-highlighted">{{ laneLabel(lane) }}</span> <span class="font-mono text-xs text-muted">{{ lane.name }}</span></span>
              <span v-if="!isStandardLane(lane.name)" class="text-xs text-muted">追加</span>
              <UButton size="xs" color="neutral" variant="ghost" :disabled="!canMoveLane(lanes, index, -1)" aria-label="上へ" @click="lanes = moveItem(lanes, index, -1)">↑</UButton>
              <UButton size="xs" color="neutral" variant="ghost" :disabled="!canMoveLane(lanes, index, 1)" aria-label="下へ" @click="lanes = moveItem(lanes, index, 1)">↓</UButton>
            </li>
          </ol>
          <p class="mt-2 text-xs text-muted">draft は先頭に固定です。標準のレーンは消せません。</p>
          <form class="mt-3 flex items-start gap-2" @submit.prevent="addLane">
            <UFormField class="min-w-0 flex-1" :error="newLaneError"><UInput v-model="newLane" class="w-full" placeholder="名前（例: review）" autocomplete="off" /></UFormField>
            <UInput v-model="newLaneLabel" class="min-w-0 flex-1" placeholder="表示名（任意）" autocomplete="off" />
            <UButton type="submit" size="sm" color="neutral" variant="outline">追加</UButton>
          </form>
        </UCard>

        <UCard data-testid="kinds-card">
          <template #header><h2 class="text-sm font-medium text-highlighted">種別</h2></template>
          <ol class="space-y-1" data-testid="kinds">
            <li v-for="(kind, index) in kinds" :key="kind" class="flex items-center gap-2 rounded-md bg-muted px-3 py-1.5 text-sm" data-testid="kind">
              <span class="min-w-0 flex-1 truncate text-highlighted">{{ kind }}</span>
              <UButton size="xs" color="neutral" variant="ghost" :disabled="index === 0" aria-label="上へ" @click="kinds = moveItem(kinds, index, -1)">↑</UButton>
              <UButton size="xs" color="neutral" variant="ghost" :disabled="index === kinds.length - 1" aria-label="下へ" @click="kinds = moveItem(kinds, index, 1)">↓</UButton>
            </li>
            <li v-if="kinds.length === 0" class="text-sm text-muted">種別はまだありません。</li>
          </ol>
          <form class="mt-3 flex items-start gap-2" @submit.prevent="addKind">
            <UFormField class="min-w-0 flex-1" :error="newKindError"><UInput v-model="newKind" class="w-full" placeholder="種別の名前" autocomplete="off" /></UFormField>
            <UButton type="submit" size="sm" color="neutral" variant="outline">追加</UButton>
          </form>
        </UCard>
      </div>

      <div class="flex items-center gap-3">
        <UButton :loading="saving" :disabled="!dirty" data-testid="save-settings" @click="save">保存</UButton>
        <span v-if="saved && !dirty" class="text-xs text-success" data-testid="saved">保存しました</span>
        <span v-else-if="dirty" class="text-xs text-muted">保存していない変更があります</span>
      </div>
      <ApiErrorAlert v-if="saveError" :error="saveError" />

      <UCard>
        <template #header><h2 class="text-sm font-medium text-highlighted">サーバーの設定（読み取りのみ）</h2></template>
        <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm" data-testid="server-settings">
          <dt class="text-muted">待ち受け port</dt><dd class="font-mono">{{ data.listen_port }}</dd>
          <dt class="text-muted">鍵を渡す IP 範囲</dt><dd class="font-mono">{{ data.allowed_ranges.join(", ") || "なし" }}</dd>
          <dt class="text-muted">許可する origin</dt><dd class="font-mono">{{ data.allowed_origins.join(", ") || "なし" }}</dd>
          <dt class="text-muted">定期処理の間隔</dt><dd class="font-mono">{{ data.settings.scheduler_interval_secs }} 秒</dd>
        </dl>
      </UCard>

      <UnsupportedNotice what="タグと protocol の一覧" missing="タグ・protocol を登録・取得するサーバーの操作（今は依頼ごとの文字列としてだけ持つ）" />
    </template>
  </ScreenFrame>
</template>
