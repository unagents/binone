<script setup lang="ts">
// The common frame of a screen: a title row (with room for actions) and the content below it.
// `wide` lets the content use the whole width (the swimlane); otherwise it is centred and capped.
defineProps<{ title: string; description?: string; wide?: boolean }>();
</script>

<template>
  <section class="mx-auto flex w-full flex-col gap-4 px-6 py-5" :class="wide ? '' : 'max-w-5xl'" :data-screen="title">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="min-w-0">
        <h1 class="text-base font-semibold text-highlighted">{{ title }}</h1>
        <p v-if="description" class="text-xs text-muted">{{ description }}</p>
      </div>
      <div class="flex items-center gap-2">
        <slot name="actions" />
      </div>
    </div>
    <slot />
  </section>
</template>
