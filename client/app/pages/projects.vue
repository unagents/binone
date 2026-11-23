<script setup lang="ts">
import type { ApiError } from "~/utils/api/errors";
import { toApiError } from "~/utils/api/errors";
import { validateProjectName } from "~/utils/settings";

const api = useApi();
const { data, error, loading, reload } = useScreenData(() => api.projects());

const name = ref("");
const nameError = ref<string>();
const creating = ref(false);
const createError = ref<ApiError | null>(null);

async function create() {
  if (creating.value) return;
  nameError.value = validateProjectName(name.value, data.value ?? []) ?? undefined;
  if (nameError.value) return;
  creating.value = true;
  createError.value = null;
  try {
    await api.createProject(name.value.trim());
    name.value = "";
    await reload();
  } catch (e) {
    createError.value = toApiError(e);
  } finally {
    creating.value = false;
  }
}
</script>

<template>
  <ScreenFrame title="プロジェクト" description="依頼が属するプロジェクトの登録と一覧。">
    <template #actions>
      <UButton color="neutral" variant="outline" size="sm" to="/settings">設定へ</UButton>
      <UButton color="neutral" variant="outline" size="sm" :loading="loading" @click="reload">読み込み直す</UButton>
    </template>

    <UCard>
      <form class="flex items-start gap-2" data-testid="project-form" @submit.prevent="create">
        <UFormField class="min-w-0 flex-1" :error="nameError">
          <UInput v-model="name" class="w-full" placeholder="新しいプロジェクトの名前" autocomplete="off" data-testid="project-name" />
        </UFormField>
        <UButton type="submit" :loading="creating">登録</UButton>
      </form>
    </UCard>
    <ApiErrorAlert v-if="createError" :error="createError" />

    <p v-if="loading && !data" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
    <ApiErrorAlert v-if="error" :error="error" />
    <ul v-if="data" class="divide-y divide-default rounded-lg border border-default" data-testid="projects">
      <li v-for="project in data" :key="project" class="px-4 py-2 text-sm text-highlighted" data-testid="project">{{ project }}</li>
      <li v-if="data.length === 0" class="px-4 py-3 text-sm text-muted">プロジェクトはまだありません。</li>
    </ul>

    <UnsupportedNotice what="プロジェクトの名前変更と削除" missing="プロジェクトを変更・削除するサーバーの操作（今は登録と一覧だけ）" />
  </ScreenFrame>
</template>
