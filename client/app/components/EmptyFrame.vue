<script setup lang="ts">
// The placeholder of a screen that is filled in later. The screens' own work replaces it.
defineProps<{ name: string }>();
</script>

<template>
  <div class="rounded-lg border border-dashed border-default p-8 text-center text-sm text-muted" :data-testid="`frame-${name}`">
    <slot>この画面はこれから作ります。</slot>
  </div>
</template>
