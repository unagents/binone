<script setup lang="ts">
import {
  canEditDraft,
  currentStep,
  describeMissingSource,
  formatDateTime,
  historySourceName,
  missingArtifactSources,
  newestFirst,
  stepStatusText,
} from "~/utils/requests";
import { requestRoute } from "~/utils/routes";

const route = useRoute();
const api = useApi();
const id = computed(() => String(route.params.id));

const { data, error, loading, reload } = useScreenData(async () => {
  const [detail, lanes] = await Promise.all([api.requestDetail(id.value), api.settings().then((v) => v.settings.lanes).catch(() => null)]);
  return { detail, lanes };
});

const detail = computed(() => data.value?.detail ?? null);
const history = computed(() => newestFirst(detail.value?.history ?? []));
const steps = computed(() => [...(detail.value?.steps ?? [])].sort((a, b) => a.position - b.position));
const current = computed(() => currentStep(detail.value?.steps ?? []));
const missing = computed(() => (detail.value?.missing_sources ?? []).map(describeMissingSource));
const artifactMissing = computed(() => missingArtifactSources(detail.value?.missing_sources ?? []).map(describeMissingSource));
const editable = computed(() => (detail.value ? canEditDraft(detail.value) : false));
</script>

<template>
  <div class="flex h-full min-h-0">
    <div class="min-w-0 flex-1 overflow-y-auto">
      <ScreenFrame :title="detail?.request.title || '依頼の状況'" description="依頼の中身、レーン、protocol の手順、履歴。" wide>
        <template #actions>
          <UButton color="neutral" variant="outline" size="sm" to="/requests">一覧へ</UButton>
          <UButton v-if="editable" size="sm" :to="requestRoute({ id, lane: detail?.lane ?? '' })" data-testid="edit-draft">登録の画面で編集</UButton>
          <UButton color="neutral" variant="outline" size="sm" :loading="loading" data-testid="reload" @click="reload">読み込み直す</UButton>
        </template>

        <p v-if="loading && !detail" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
        <ApiErrorAlert v-if="error" :error="error" />

        <template v-if="detail">
          <section class="rounded-lg border border-default p-4" data-testid="request-fields">
            <div class="mb-3 flex flex-wrap items-center gap-2">
              <RequestsLaneBadge :lane="detail.lane" :lanes="data?.lanes" />
              <span class="text-xs text-muted">版 {{ detail.request.version }}</span>
            </div>
            <dl class="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-x-4 gap-y-3 text-sm">
              <div><dt class="text-xs text-muted">プロジェクト</dt><dd class="text-default">{{ detail.request.project || "—" }}</dd></div>
              <div><dt class="text-xs text-muted">種別</dt><dd class="text-default">{{ detail.request.kind || "—" }}</dd></div>
              <div><dt class="text-xs text-muted">protocol</dt><dd class="text-default">{{ detail.request.protocol ?? "未設定" }}</dd></div>
              <div>
                <dt class="text-xs text-muted">タグ</dt>
                <dd class="flex flex-wrap gap-1"><UBadge v-for="t in detail.request.tags" :key="t" variant="subtle" size="sm">{{ t }}</UBadge><span v-if="!detail.request.tags.length">—</span></dd>
              </div>
              <div><dt class="text-xs text-muted">登録</dt><dd class="text-default">{{ formatDateTime(detail.request.created_ms) }}</dd></div>
              <div><dt class="text-xs text-muted">更新</dt><dd class="text-default">{{ formatDateTime(detail.request.updated_ms) }}</dd></div>
            </dl>
            <div class="mt-4 border-t border-default pt-3 text-default" data-testid="request-body">
              <MarkdownView v-if="detail.request.body" :source="detail.request.body" />
              <p v-else class="text-sm text-muted">（本文なし）</p>
            </div>
          </section>

          <UAlert
            v-if="missing.length"
            color="warning"
            variant="subtle"
            title="読み出せていない情報があります"
            data-testid="missing-sources"
          >
            <template #description>
              <p>空なのではなく、取得できていません。</p>
              <ul class="mt-1 space-y-1 text-xs">
                <li v-for="m in missing" :key="m.name" data-testid="missing-source"><b>{{ m.name }}</b>: {{ m.reason }}<span v-if="m.detail" class="ml-1 font-mono opacity-80">{{ m.detail }}</span></li>
              </ul>
            </template>
          </UAlert>

          <section class="rounded-lg border border-default p-4" data-testid="steps">
            <h2 class="mb-2 text-sm font-semibold text-highlighted">protocol の手順</h2>
            <p v-if="current" class="mb-2 text-xs text-muted" data-testid="current-step">今の手順: <b class="text-default">{{ current.key }}</b></p>
            <ol v-if="steps.length" class="space-y-1 text-sm">
              <li v-for="s in steps" :key="s.key" class="flex items-center gap-2" :data-status="s.status" data-testid="step">
                <span class="size-2 rounded-full" :class="s.status === 'done' ? 'bg-success' : s.status === 'active' ? 'bg-primary' : 'bg-muted'" />
                <span :class="s.status === 'active' ? 'text-highlighted' : 'text-default'">{{ s.key }}</span>
                <span class="text-xs text-muted">{{ stepStatusText(s.status) }}</span>
              </li>
            </ol>
            <p v-else class="text-sm text-muted" data-testid="steps-empty">
              {{ detail.request.protocol ? "サーバーが返した手順はありません。" : "protocol が決まっていないため、手順はありません。" }}
              <template v-if="missing.length">（読み出せていない情報があります）</template>
            </p>
          </section>

          <section class="rounded-lg border border-default p-4" data-testid="history">
            <h2 class="mb-2 text-sm font-semibold text-highlighted">履歴（新しい順）</h2>
            <ul v-if="history.length" class="divide-y divide-default text-sm">
              <li v-for="(h, i) in history" :key="i" class="flex items-start gap-3 py-2" data-testid="history-entry">
                <span class="w-36 shrink-0 font-mono text-xs text-muted">{{ formatDateTime(h.at_ms) }}</span>
                <UBadge variant="outline" size="sm" color="neutral">{{ historySourceName(h) }}</UBadge>
                <span class="min-w-0 flex-1 text-default">{{ h.summary }}</span>
              </li>
            </ul>
            <p v-else class="text-sm text-muted" data-testid="history-empty">
              {{ missing.length ? "履歴を取り出せていません（上の読み出せていない情報を見てください）。" : "履歴はまだありません。" }}
            </p>
          </section>

          <UnsupportedNotice
            what="承認・修正の指示・質問への回答"
            missing="この画面からの操作（承認と修正の指示は decideApproval、質問への回答は answerQuestion。画面は未実装）"
          />

          <section class="rounded-lg border border-default p-4" data-testid="artifacts">
            <h2 class="mb-2 text-sm font-semibold text-highlighted">成果物</h2>
            <div class="flex gap-3 overflow-x-auto">
              <p v-if="artifactMissing.length" class="text-sm text-warning" data-testid="artifacts-missing">
                成果物の情報源が取り出せていません（{{ artifactMissing.map((m) => `${m.name}: ${m.reason}`).join("、") }}）
              </p>
              <p v-else class="text-sm text-muted" data-testid="artifacts-empty">成果物はまだありません。</p>
            </div>
          </section>
        </template>
      </ScreenFrame>
    </div>
    <RequestsAgentPane title="binone エージェント" />
  </div>
</template>
