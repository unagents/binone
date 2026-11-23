<script setup lang="ts">
import { describeConnectError, isValidIpv4, parsePort, submitsOnKeydown } from "~/utils/connection";

const { connected, failure, defaultPort, restoring, restore, connect, change, address } = useConnection();

const host = ref("");
const port = ref("");
const portOpen = ref(false);
const hostError = ref<string>();
const portError = ref<string>();
const connecting = ref(false);

function fillForm(savedHost: string, savedPort: number | null) {
  host.value = savedHost;
  port.value = savedPort === null ? "" : String(savedPort);
  portOpen.value = savedPort !== null;
}

onMounted(async () => {
  await restore();
  if (failure.value) fillForm(failure.value.host, failure.value.port);
});

const failureMessage = computed(() => {
  if (!failure.value) return null;
  const { host: failedHost, port: failedPort, error } = failure.value;
  return { ...describeConnectError(error.kind, address(failedHost, failedPort)), detail: error.detail };
});

async function onSubmit() {
  // One attempt at a time, however it was started (the button, or Return in a field).
  if (connecting.value) return;
  hostError.value = undefined;
  portError.value = undefined;

  const ip = host.value.trim();
  if (!isValidIpv4(ip)) {
    hostError.value = "IP アドレスを 192.168.0.10 の形で入れてください";
  }
  // A closed port field means "default port", whatever was typed before closing it.
  const parsed = parsePort(portOpen.value ? port.value : "");
  if (!parsed.ok) {
    portError.value = "port は 1 から 65535 の数字で入れてください";
  }
  if (hostError.value || !parsed.ok) return;

  connecting.value = true;
  await connect(ip, parsed.port);
  connecting.value = false;
}

/**
 * Return in the IP or the port field does what the button does. The key's own action is always
 * suppressed, so the form is not submitted a second time by the webview.
 */
function onFieldKeydown(event: KeyboardEvent) {
  if (event.key !== "Enter") return;
  event.preventDefault();
  if (submitsOnKeydown(event, connecting.value)) void onSubmit();
}

function onChange() {
  if (connected.value) fillForm(connected.value.host, connected.value.port);
  change();
}
</script>

<template>
  <main class="flex min-h-[70vh] items-center justify-center p-6" data-testid="connection-screen">
    <UCard class="w-full max-w-md">
      <template #header>
        <div class="flex items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold text-highlighted">binone</h1>
            <p class="text-sm text-muted">サーバーへの接続</p>
          </div>
          <UBadge
            v-if="!restoring"
            data-testid="connection-status"
            :color="connected ? 'success' : 'neutral'"
            variant="subtle"
            size="lg"
          >
            {{ connected ? "接続済み" : "未接続" }}
          </UBadge>
        </div>
      </template>

      <p v-if="restoring" class="text-sm text-muted" data-testid="restoring">保存した接続先を確かめています…</p>

      <dl v-else-if="connected" class="space-y-3 text-sm" data-testid="connected">
        <div class="flex justify-between gap-4">
          <dt class="text-muted">接続先</dt>
          <dd class="font-mono text-default">{{ address(connected.host, connected.port) }}</dd>
        </div>
        <div class="flex justify-between gap-4">
          <dt class="text-muted">サーバーの版</dt>
          <dd class="font-mono text-default" data-testid="server-version">{{ connected.version }}</dd>
        </div>
      </dl>

      <form v-else class="space-y-4" data-testid="connection-form" @submit.prevent="onSubmit">
        <UAlert
          v-if="failureMessage"
          data-testid="connection-error"
          color="error"
          variant="subtle"
          :title="failureMessage.title"
        >
          <template #description>
            <p>{{ failureMessage.description }}</p>
            <p v-if="failureMessage.hint" class="mt-1" data-testid="connection-error-hint">{{ failureMessage.hint }}</p>
            <p class="mt-1 font-mono text-xs opacity-80">{{ failureMessage.detail }}</p>
          </template>
        </UAlert>

        <UFormField label="サーバーの IP" name="host" :error="hostError" required>
          <UInput
            v-model="host"
            class="w-full"
            placeholder="192.168.0.10"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            autofocus
            @keydown="onFieldKeydown"
          />
        </UFormField>

        <UCollapsible v-model:open="portOpen">
          <UButton
            type="button"
            color="neutral"
            variant="link"
            size="sm"
            class="px-0"
            trailing-icon="i-lucide-chevron-down"
            :ui="{ trailingIcon: portOpen ? 'rotate-180 transition-transform' : 'transition-transform' }"
          >
            サーバーの port を変えている場合
          </UButton>
          <template #content>
            <UFormField
              class="pt-3"
              label="port"
              name="port"
              :error="portError"
              :help="defaultPort === null ? undefined : `空のままなら既定の ${defaultPort} を使います`"
            >
              <UInput
                v-model="port"
                class="w-full"
                inputmode="numeric"
                :placeholder="defaultPort === null ? '' : String(defaultPort)"
                autocomplete="off"
                @keydown="onFieldKeydown"
              />
            </UFormField>
          </template>
        </UCollapsible>

        <UButton type="submit" block :loading="connecting">接続</UButton>
      </form>

      <template v-if="connected" #footer>
        <div class="flex items-center justify-between gap-3">
          <UButton color="neutral" variant="outline" size="sm" @click="onChange">接続先を変える</UButton>
          <UButton size="sm" to="/requests">依頼の一覧へ</UButton>
        </div>
      </template>
    </UCard>
  </main>
</template>
