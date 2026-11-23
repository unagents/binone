<script setup lang="ts">
import { toApiError, type ApiError } from "~/utils/api/errors";
import type { MissingSource, Request, ServerInfo } from "~/types/binone";
import {
  addTag,
  baseFromRequest,
  describeBodyUnreadable,
  emptyForm,
  formFromRequest,
  formatDateTime,
  saveOnce,
  sameForm,
  submitProblems,
  describeSaveTarget,
  type DraftBase,
  type DraftForm,
} from "~/utils/requests";
import { requestRoute } from "~/utils/routes";
import { validateProjectName } from "~/utils/settings";

// One screen for a new request and for a draft being edited (`/requests/new?id=<id>`). The version
// read is kept and sent back on every save; a request that moved on is a conflict the user settles.
const route = useRoute();
const router = useRouter();
const api = useApi();

const form = reactive<DraftForm>(emptyForm());
const base = ref<DraftBase | null>(null);
const lane = ref("draft");
const savedForm = ref<DraftForm>(emptyForm());
const savedAt = ref<number | null>(null);
const loadedId = computed(() => (typeof route.query.id === "string" && route.query.id !== "" ? route.query.id : null));

const projects = ref<string[]>([]);
const kinds = ref<string[]>([]);
const loading = ref(true);
const loadError = ref<ApiError | null>(null);
const optionsError = ref<ApiError | null>(null);
const actionError = ref<ApiError | null>(null);
const conflict = ref(false);
const busy = ref<"save" | "submit" | "reload" | "overwrite" | null>(null);
const problems = ref<string[]>([]);
const tagText = ref("");
// The body is written as text; the preview shows it formatted (raw HTML, images and links are not shown as such).
const previewing = ref(false);

const dirty = computed(() => !sameForm(form, savedForm.value));
const editable = computed(() => lane.value === "draft");
const stateText = computed(() => {
  if (savedAt.value === null) return base.value ? "保存済み" : "未保存";
  return `${dirty.value ? "変更あり ・ " : ""}保存済み ${formatDateTime(savedAt.value)}`;
});

function apply(request: Request) {
  Object.assign(form, formFromRequest(request));
  savedForm.value = formFromRequest(request);
  base.value = baseFromRequest(request);
  lane.value = request.lane;
}

// Where a save goes (the server says), shown small at the edge. Not knowing it is not an error here.
const info = ref<ServerInfo | null>(null);
const storeText = computed(() => describeSaveTarget(info.value));
const bodyUnreadable = computed(() => base.value?.bodyUnreadable === true);
// Why the body cannot be read: asked of the status record, only when it cannot. Not getting it is not an error here.
const bodySources = ref<MissingSource[]>([]);
const bodyUnreadableText = computed(() => describeBodyUnreadable(bodySources.value));

async function loadOptions() {
  optionsError.value = null;
  try {
    info.value = await api.serverInfo();
  } catch {
    info.value = null;
  }
  try {
    projects.value = await api.projects();
  } catch (e) {
    optionsError.value = toApiError(e);
  }
  try {
    kinds.value = (await api.settings()).settings.kinds;
  } catch (e) {
    optionsError.value ??= toApiError(e);
  }
}

async function loadRequest(id: string) {
  loadError.value = null;
  try {
    const request = await api.request(id);
    apply(request);
    bodySources.value = [];
    if (bodyUnreadable.value) {
      try {
        bodySources.value = (await api.requestDetail(id)).missing_sources;
      } catch {
        bodySources.value = [];
      }
    }
  } catch (e) {
    loadError.value = toApiError(e);
  }
}

async function loadAll(id: string | null) {
  loading.value = true;
  // Another request (or none): what the form held is dropped, not carried over.
  Object.assign(form, emptyForm());
  savedForm.value = emptyForm();
  base.value = null;
  bodySources.value = [];
  lane.value = "draft";
  savedAt.value = null;
  actionError.value = null;
  conflict.value = false;
  problems.value = [];
  await Promise.all([loadOptions(), id ? loadRequest(id) : Promise.resolve()]);
  loading.value = false;
}

onMounted(() => void loadAll(loadedId.value));

// `?id=` changed to another request, or was cleared. Saving a new request sets `?id=` to the id
// just saved (`base` already has it), which is not a change of request and must not reload.
watch(loadedId, (id) => {
  if (id !== (base.value?.id ?? null)) void loadAll(id);
});

const projectItems = computed(() => withCurrent(projects.value, form.project));
const kindItems = computed(() => withCurrent(kinds.value, form.kind));
function withCurrent(list: string[], current: string) {
  return (current !== "" && !list.includes(current) ? [...list, current] : list).map((v) => ({ label: v, value: v }));
}

function fail(e: unknown) {
  const error = toApiError(e);
  conflict.value = error.kind === "conflict";
  actionError.value = conflict.value ? null : error;
}

/** Saves the draft; returns the saved request, or null when it failed (the reason is on screen). */
async function save(versionOverride?: number): Promise<Request | null> {
  actionError.value = null;
  conflict.value = false;
  try {
    // Sent once. A failure, an unknown outcome included, is shown and the input stays as typed.
    const done = await saveOnce(api.saveDraft, form, base.value, savedForm.value, versionOverride);
    Object.assign(form, done.form);
    savedForm.value = done.saved;
    base.value = done.base;
    lane.value = done.request.lane;
    savedAt.value = Date.now();
    if (loadedId.value !== done.request.id) await router.replace({ path: "/requests/new", query: { id: done.request.id } });
    return done.request;
  } catch (e) {
    fail(e);
    return null;
  }
}

async function onSave() {
  if (busy.value) return;
  busy.value = "save";
  await save();
  busy.value = null;
}

async function onSubmit() {
  if (busy.value) return;
  problems.value = submitProblems(form, base.value);
  if (problems.value.length) return;
  busy.value = "submit";
  const saved = !base.value || dirty.value ? await save() : { id: base.value.id, version: base.value.version };
  if (saved) {
    try {
      await navigateTo(requestRoute(await api.submitRequest(saved.id, saved.version)));
    } catch (e) {
      fail(e);
    }
  }
  busy.value = null;
}

/** Take the server's version in place of the edits. */
async function onReread() {
  if (!base.value || busy.value) return;
  busy.value = "reload";
  actionError.value = null;
  conflict.value = false;
  try {
    apply(await api.request(base.value.id));
  } catch (e) {
    fail(e);
  }
  busy.value = null;
}

/** Save the edits over what the server has now: the newest version is read first, then sent. */
async function onOverwrite() {
  if (!base.value || busy.value) return;
  busy.value = "overwrite";
  try {
    const latest = await api.request(base.value.id);
    // What this screen knows of the body stays as it was: the answer may say "unreadable" for a body it holds.
    base.value = { id: latest.id, version: latest.version, change: latest.change, bodyUnreadable: base.value.bodyUnreadable };
    if (latest.lane !== "draft") {
      lane.value = latest.lane;
      conflict.value = false;
      actionError.value = { kind: "conflict", detail: "依頼はもう draft ではありません（上書きできません）" };
    } else {
      await save();
    }
  } catch (e) {
    fail(e);
  }
  busy.value = null;
}

function onTagKeydown(event: KeyboardEvent) {
  if (event.key !== "Enter" || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  form.tags = addTag(form.tags, tagText.value);
  tagText.value = "";
}

// A project made from the choices.
const newProject = ref("");
const projectOpen = ref(false);
const projectError = ref<string>();
const projectActionError = ref<ApiError | null>(null);
const creating = ref(false);
async function createProject() {
  if (creating.value) return;
  projectError.value = validateProjectName(newProject.value, projects.value) ?? undefined;
  if (projectError.value) return;
  creating.value = true;
  projectActionError.value = null;
  try {
    const name = await api.createProject(newProject.value.trim());
    projects.value = await api.projects();
    form.project = name;
    newProject.value = "";
    projectOpen.value = false;
  } catch (e) {
    projectActionError.value = toApiError(e);
  } finally {
    creating.value = false;
  }
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <div class="min-w-0 flex-1 overflow-y-auto">
      <ScreenFrame :title="base ? '依頼の編集' : '依頼の登録'" description="作業指示を Markdown で書きます。「途中保存」と「todo 化」は別の操作です。" wide>
        <template #actions>
          <span class="text-xs text-muted" data-testid="save-state">{{ stateText }}</span>
          <UButton color="neutral" variant="outline" size="sm" :disabled="!editable || !!loadError" :loading="busy === 'save'" data-testid="save" @click="onSave">途中保存</UButton>
          <UButton size="sm" :disabled="!editable || !!loadError" :loading="busy === 'submit'" data-testid="submit" @click="onSubmit">todo 化</UButton>
        </template>

        <p v-if="storeText" class="text-xs text-muted" data-testid="save-target">{{ storeText }}</p>
        <p v-if="loading" class="text-sm text-muted" data-testid="loading">読み込んでいます…</p>
        <ApiErrorAlert v-if="loadError" :error="loadError" />
        <ApiErrorAlert v-if="optionsError" :error="optionsError" />

        <template v-if="!loading && !loadError">
          <UAlert v-if="!editable" color="warning" variant="subtle" title="この依頼は draft ではないので編集できません" data-testid="not-draft">
            <template #description>
              <NuxtLink :to="requestRoute({ id: base?.id ?? '', lane: lane })" class="underline">依頼の状況を開く</NuxtLink>
            </template>
          </UAlert>

          <UAlert v-if="conflict" color="warning" variant="subtle" title="他の操作と重なりました" data-testid="conflict">
            <template #description>
              <p>読み込んだ後に、この依頼が別の所で更新されました。サーバーの内容を読み直すと、ここでの変更は失われます。上書きすると、ここでの内容がサーバーの内容に替わります。</p>
              <div class="mt-2 flex gap-2">
                <UButton size="xs" color="neutral" variant="outline" :loading="busy === 'reload'" data-testid="conflict-reload" @click="onReread">読み直す</UButton>
                <UButton size="xs" color="warning" :loading="busy === 'overwrite'" data-testid="conflict-overwrite" @click="onOverwrite">上書き</UButton>
              </div>
            </template>
          </UAlert>
          <ApiErrorAlert v-if="actionError" :error="actionError" keeps-input />
          <UAlert v-if="problems.length" color="warning" variant="subtle" title="todo 化の前に確かめてください" data-testid="problems">
            <template #description>
              <ul class="list-disc pl-5"><li v-for="p in problems" :key="p">{{ p }}</li></ul>
            </template>
          </UAlert>

          <fieldset :disabled="!editable" class="flex min-w-0 flex-col gap-4">
            <UInput v-model="form.title" size="xl" class="w-full" placeholder="タイトル" aria-label="タイトル" data-testid="title" />

            <div class="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-x-4 gap-y-3 border-b border-default pb-4">
              <UFormField label="プロジェクト" :error="projectError">
                <USelect v-model="form.project" :items="projectItems" placeholder="選んでください" class="w-full" data-testid="project" />
                <UButton type="button" variant="link" size="xs" class="px-0" data-testid="project-new-toggle" @click="projectOpen = !projectOpen">新しいプロジェクトを作る</UButton>
                <form v-if="projectOpen" class="mt-1 flex gap-1" data-testid="project-new" @submit.prevent="createProject">
                  <UInput v-model="newProject" size="sm" class="min-w-0 flex-1" placeholder="プロジェクトの名前" autocomplete="off" data-testid="project-new-name" />
                  <UButton type="submit" size="sm" :loading="creating">作る</UButton>
                </form>
                <ApiErrorAlert v-if="projectActionError" class="mt-1" :error="projectActionError" />
              </UFormField>
              <UFormField label="種別">
                <USelect v-model="form.kind" :items="kindItems" placeholder="選んでください" class="w-full" data-testid="kind" />
              </UFormField>
              <UFormField label="protocol" help="選択肢をサーバーが返さないため、名前を直接入れます">
                <UInput v-model="form.protocol" class="w-full" placeholder="例: protocol-a" autocomplete="off" data-testid="protocol" />
              </UFormField>
              <UFormField label="タグ">
                <div class="flex min-h-8 flex-wrap items-center gap-1 rounded-md border border-default px-2 py-1">
                  <UBadge v-for="t in form.tags" :key="t" variant="subtle" size="sm" class="gap-1" data-testid="tag">
                    {{ t }}
                    <button type="button" :aria-label="`${t} を外す`" class="opacity-70 hover:opacity-100" @click="form.tags = form.tags.filter((x) => x !== t)">×</button>
                  </UBadge>
                  <input v-model="tagText" class="min-w-16 flex-1 bg-transparent text-sm outline-none" placeholder="追加…" aria-label="タグを追加" data-testid="tag-input" @keydown="onTagKeydown">
                </div>
              </UFormField>
            </div>

            <UAlert v-if="bodyUnreadable" color="warning" variant="subtle" :title="bodyUnreadableText" data-testid="body-unreadable">
              <template #description>
                <p>本文は空ではなく、読めていません。題名と管理項目は編集して保存できます。保存しても本文は送らないので、今の本文は書き換わりません。</p>
              </template>
            </UAlert>
            <template v-else>
              <div class="flex items-center gap-2">
                <UButton type="button" size="xs" :variant="previewing ? 'outline' : 'solid'" color="neutral" data-testid="body-edit" @click="previewing = false">書く</UButton>
                <UButton type="button" size="xs" :variant="previewing ? 'solid' : 'outline'" color="neutral" data-testid="body-preview-toggle" @click="previewing = true">プレビュー</UButton>
              </div>
              <div v-if="previewing" class="min-h-48 rounded-md border border-default p-3" data-testid="body-preview">
                <MarkdownView v-if="form.body.trim() !== ''" :source="form.body" />
                <p v-else class="text-sm text-muted">（本文なし）</p>
              </div>
              <UTextarea v-else v-model="form.body" :rows="14" autoresize class="w-full" placeholder="作業指示を Markdown で書く" aria-label="作業指示（Markdown）" data-testid="body" />
            </template>
          </fieldset>
        </template>
      </ScreenFrame>
    </div>
    <RequestsAgentPane title="依頼作成支援エージェント" />
  </div>
</template>
