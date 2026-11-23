<script setup lang="ts">
import { unsupportedHeadline } from "~/utils/unsupported";

// A part of a screen that does not work yet. It says so instead of faking it. By default the
// server side does not exist; `headline` says otherwise where the lack is somewhere else.
defineProps<{ what: string; missing?: string; headline?: string }>();
</script>

<template>
  <div class="rounded-lg border border-default bg-muted p-4 text-sm" data-testid="unsupported">
    <p class="font-medium text-highlighted">{{ unsupportedHeadline(what, headline) }}</p>
    <p v-if="missing" class="mt-1 text-xs text-muted">足りないもの: {{ missing }}</p>
  </div>
</template>
