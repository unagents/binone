<script setup lang="ts">
import { connectionView } from "~/utils/connection";

const { connected, failure, restoring, hasKey, reacquiring, restore, connect, changeAddress, forget, address } = useConnection();

onMounted(() => {
  void restore();
});

const view = computed(() =>
  connectionView({
    restoring: restoring.value,
    connected: connected.value !== null,
    hasKey: hasKey.value,
    reacquiring: reacquiring.value,
    failureKind: failure.value?.error.kind ?? null,
  }),
);

// What the form starts with: the address that failed, or (first time) nothing.
const initialHost = computed(() => failure.value?.host ?? "");
const initialPort = computed(() => failure.value?.port ?? null);

const confirmingReset = ref(false);
const resetError = ref<string>();
async function onReset() {
  const error = await forget();
  resetError.value = error ? `${error.detail}` : undefined;
  confirmingReset.value = false;
}
</script>

<template>
  <main class="flex min-h-[70vh] items-center justify-center p-6" data-testid="connection-screen" :data-view="view">
    <UCard class="w-full max-w-md">
      <template #header>
        <div class="flex items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold text-highlighted">binone</h1>
            <p class="text-sm text-muted">{{ view === "acquire" ? "サーバーへの初回の接続" : "サーバーへの接続" }}</p>
          </div>
          <UBadge
            v-if="view !== 'restoring'"
            data-testid="connection-status"
            :color="view === 'connected' ? 'success' : 'neutral'"
            variant="subtle"
            size="lg"
          >
            {{ view === "connected" ? "接続済み" : "未接続" }}
          </UBadge>
        </div>
      </template>

      <p v-if="view === 'restoring'" class="text-sm text-muted" data-testid="restoring">保存した接続先を確かめています…</p>

      <dl v-else-if="view === 'connected' && connected" class="space-y-3 text-sm" data-testid="connected">
        <div class="flex justify-between gap-4">
          <dt class="text-muted">接続先</dt>
          <dd class="font-mono text-default">{{ address(connected.host, connected.port) }}</dd>
        </div>
        <div class="flex justify-between gap-4">
          <dt class="text-muted">サーバーの版</dt>
          <dd class="font-mono text-default" data-testid="server-version">{{ connected.version }}</dd>
        </div>
      </dl>

      <!-- No key is held (or it is being fetched again): the only place a key is fetched. -->
      <div v-else-if="view === 'acquire'" class="space-y-4" data-testid="acquire-form">
        <p class="text-sm text-muted">
          {{ hasKey ? "サーバーと同じ network から、鍵を取り直します。" : "サーバーの IP を入れると、このアプリが接続に使う鍵を受け取ります。" }}
        </p>
        <ConnectFailure v-if="failure" :failure="failure" />
        <AddressForm :key="view" :initial-host="initialHost" :initial-port="initialPort" submit-label="接続" :run="connect" />
        <UButton v-if="hasKey" color="neutral" variant="ghost" size="sm" data-testid="cancel-reacquire" @click="reacquiring = false">戻る</UButton>
      </div>

      <!-- A key is held and the connection did not work: the address can be changed. -->
      <div v-else class="space-y-4" :data-testid="view === 'key_invalid' ? 'key-invalid' : 'connection-failed'">
        <ConnectFailure v-if="failure" :failure="failure" />
        <UButton v-if="view === 'key_invalid'" data-testid="reacquire-key" @click="reacquiring = true">鍵を取り直す</UButton>
        <p class="text-sm text-muted">{{ view === "key_invalid" ? "別の address に接続することもできます。" : "address を直して、もう一度接続できます。" }}</p>
        <AddressForm :key="view" :initial-host="initialHost" :initial-port="initialPort" submit-label="接続" :run="changeAddress" />
      </div>

      <template v-if="view === 'connected'" #footer>
        <div class="flex items-center justify-between gap-3">
          <UButton color="neutral" variant="outline" size="sm" to="/settings">接続先は設定で変える</UButton>
          <UButton size="sm" to="/requests">依頼の一覧へ</UButton>
        </div>
      </template>
      <template v-else-if="view === 'key_invalid' || view === 'failed'" #footer>
        <div class="space-y-1">
          <div v-if="confirmingReset" class="flex items-center gap-2 text-xs" data-testid="reset-confirm">
            <span class="text-muted">保存した address と鍵を消して、初回の画面に戻ります。</span>
            <UButton size="xs" color="error" variant="soft" data-testid="reset-yes" @click="onReset">やり直す</UButton>
            <UButton size="xs" color="neutral" variant="ghost" @click="confirmingReset = false">やめる</UButton>
          </div>
          <UButton v-else size="xs" color="neutral" variant="link" data-testid="reset-start" @click="confirmingReset = true">接続をやり直す</UButton>
          <p v-if="resetError" class="text-xs text-error">{{ resetError }}</p>
        </div>
      </template>
    </UCard>
  </main>
</template>
