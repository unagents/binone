<script setup lang="ts">
import { describeApiError, pointsToConnection, type ApiError } from "~/utils/api/errors";

// A failed call, in the connection screen's way: what happened, what to check, and the detail.
const props = defineProps<{ error: ApiError }>();
const { connected, address } = useConnection();

const message = computed(() => {
  const shown = connected.value ? address(connected.value.host, connected.value.port) : "";
  return describeApiError(props.error.kind, shown);
});
</script>

<template>
  <UAlert color="error" variant="subtle" :title="message.title" data-testid="api-error" :data-kind="error.kind">
    <template #description>
      <p>{{ message.description }}</p>
      <p v-if="message.hint" class="mt-1">{{ message.hint }}</p>
      <p v-if="error.detail" class="mt-1 font-mono text-xs opacity-80">{{ error.detail }}</p>
      <NuxtLink v-if="pointsToConnection(error.kind)" to="/connect" class="mt-2 inline-block underline">接続の画面を開く</NuxtLink>
    </template>
  </UAlert>
</template>
