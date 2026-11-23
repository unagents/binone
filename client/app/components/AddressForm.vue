<script setup lang="ts">
import { isValidIpv4, parsePort, submitsOnKeydown } from "~/utils/connection";

// The server's address (IP and, when it is not the default, port) with a button. What the button
// does is up to the screen that uses it (fetch a key, or only connect with the key that is held).
const props = defineProps<{
  initialHost?: string;
  initialPort?: number | null;
  submitLabel: string;
  /** Runs with the checked address. The form is busy until it returns. */
  run: (host: string, port: number | null) => Promise<unknown>;
}>();

const { defaultPort } = useConnection();

const host = ref(props.initialHost ?? "");
const port = ref(props.initialPort == null ? "" : String(props.initialPort));
const portOpen = ref(props.initialPort != null);
const hostError = ref<string>();
const portError = ref<string>();
const busy = ref(false);

async function onSubmit() {
  // One attempt at a time, however it was started (the button, or Return in a field).
  if (busy.value) return;
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

  busy.value = true;
  try {
    await props.run(ip, parsed.port);
  } finally {
    busy.value = false;
  }
}

/**
 * Return in the IP or the port field does what the button does. The key's own action is always
 * suppressed, so the form is not submitted a second time by the webview.
 */
function onFieldKeydown(event: KeyboardEvent) {
  if (event.key !== "Enter") return;
  event.preventDefault();
  if (submitsOnKeydown(event, busy.value)) void onSubmit();
}
</script>

<template>
  <form class="space-y-4" data-testid="address-form" @submit.prevent="onSubmit">
    <UFormField label="サーバーの IP" name="host" :error="hostError" required>
      <UInput
        v-model="host"
        class="w-full"
        placeholder="192.168.0.10"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        data-testid="address-host"
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

    <UButton type="submit" block :loading="busy" data-testid="address-submit">{{ submitLabel }}</UButton>
  </form>
</template>
