<script setup lang="ts">
import { formatDateTime, relativeTime } from "~/utils/requests";
import { requestRoute } from "~/utils/routes";
import { allProjectsValue, projectFromSelect, projectToSelect } from "~/utils/swimlane";
import type { RequestSummary } from "~/types/binone";

const api = useApi();
// `null` is "all projects"; no project name can be mistaken for it. The select needs a value of its
// own for that choice, which `allProjectsValue` keeps apart from every project name.
const project = ref<string | null>(null);

// The lanes' labels come from the settings; when they cannot be read the lane names are shown.
const { data, error, loading, reload } = useScreenData(async () => {
  const filter = project.value === null ? null : { project: project.value };
  const [rows, lanes, projects] = await Promise.all([
    api.requests({ filter }),
    api.settings().then((v) => v.settings.lanes).catch(() => null),
    api.projects().catch(() => [] as string[]),
  ]);
  return { rows, lanes, projects };
});

const projectNames = computed(() => (data.value?.projects ?? []).filter((p) => p !== ""));
const allValue = computed(() => allProjectsValue(projectNames.value));
const projectItems = computed(() => [
  { label: "すべてのプロジェクト", value: allValue.value },
  ...projectNames.value.map((p) => ({ label: p, value: p })),
]);
const selected = computed({
  get: () => projectToSelect(project.value, allValue.value),
  set: (value: string | null | undefined) => {
    project.value = projectFromSelect(value, allValue.value);
  },
});
watch(project, () => void reload());
const now = Date.now();

function open(row: RequestSummary) {
  void navigateTo(requestRoute(row));
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <div class="min-w-0 flex-1 overflow-y-auto">
      <ScreenFrame title="依頼の一覧" description="依頼とその状態（レーン）。選ぶと依頼の状況（draft は登録の画面）を開きます。" wide>
        <template #actions>
          <USelect v-model="selected" :items="projectItems" size="sm" class="w-48" aria-label="プロジェクトで絞る" data-testid="project-filter" />
          <UButton color="neutral" variant="outline" size="sm" :loading="loading" data-testid="reload" @click="reload">読み込み直す</UButton>
          <UButton size="sm" to="/requests/new" data-testid="new-request">依頼を登録</UButton>
        </template>

        <p v-if="loading && !data" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
        <ApiErrorAlert v-if="error" :error="error" />
        <div v-if="data" class="overflow-x-auto rounded-lg border border-default">
          <table class="w-full min-w-[520px] text-left text-sm" data-testid="requests">
            <thead class="border-b border-default text-xs text-muted">
              <tr>
                <th class="px-4 py-2 font-medium">依頼</th>
                <th class="px-3 py-2 font-medium">状態</th>
                <th class="px-3 py-2 font-medium">プロジェクト</th>
                <th class="px-3 py-2 font-medium">種別</th>
                <th class="px-3 py-2 font-medium">更新</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-default">
              <tr
                v-for="row in data.rows"
                :key="row.id"
                class="cursor-pointer hover:bg-elevated/60"
                tabindex="0"
                data-testid="request-row"
                @click="open(row)"
                @keydown.enter="open(row)"
              >
                <td class="max-w-xs px-4 py-2.5">
                  <span class="block truncate text-highlighted">{{ row.title || "（無題）" }}</span>
                  <span v-if="row.tags.length" class="block truncate text-xs text-muted">{{ row.tags.join(" ・ ") }}</span>
                </td>
                <td class="px-3 py-2.5 whitespace-nowrap"><RequestsLaneBadge :lane="row.lane" :lanes="data.lanes" /></td>
                <td class="px-3 py-2.5 whitespace-nowrap text-default">{{ row.project || "—" }}</td>
                <td class="px-3 py-2.5 whitespace-nowrap text-default">{{ row.kind || "—" }}</td>
                <td class="px-3 py-2.5 whitespace-nowrap text-xs text-muted" :title="formatDateTime(row.updatedMs)">{{ relativeTime(row.updatedMs, now) }}</td>
              </tr>
              <tr v-if="data.rows.length === 0">
                <td colspan="5" class="px-4 py-6 text-center text-muted" data-testid="requests-empty">依頼はまだありません。</td>
              </tr>
            </tbody>
          </table>
        </div>
      </ScreenFrame>
    </div>
    <RequestsAgentPane title="binone エージェント" />
  </div>
</template>
