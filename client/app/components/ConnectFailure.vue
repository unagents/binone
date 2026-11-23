<script setup lang="ts">
import { describeConnectError } from "~/utils/connection";
import type { Failure } from "~/composables/useConnection";

// A failed connection attempt: what happened, what to check, and the technical detail.
const props = defineProps<{ failure: Failure }>();
const { address } = useConnection();

const message = computed(() => {
  const { host, port, error } = props.failure;
  return { ...describeConnectError(error.kind, address(host, port)), detail: error.detail };
});
</script>

<template>
  <UAlert data-testid="connection-error" :data-kind="failure.error.kind" color="error" variant="subtle" :title="message.title">
    <template #description>
      <p>{{ message.description }}</p>
      <p v-if="message.hint" class="mt-1" data-testid="connection-error-hint">{{ message.hint }}</p>
      <p class="mt-1 font-mono text-xs opacity-80">{{ message.detail }}</p>
    </template>
  </UAlert>
</template>
