<script setup lang="ts">
// A part of a screen whose server side does not exist yet. It says so instead of faking it.
defineProps<{ what: string; missing?: string }>();
</script>

<template>
  <div class="rounded-lg border border-default bg-muted p-4 text-sm" data-testid="unsupported">
    <p class="font-medium text-highlighted">{{ what }}: サーバーが未対応です</p>
    <p v-if="missing" class="mt-1 text-xs text-muted">足りないもの: {{ missing }}</p>
  </div>
</template>
