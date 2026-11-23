<script setup lang="ts">
// A request body as formatted Markdown. The HTML comes from `renderMarkdown` only, which never
// lets raw HTML, an image, or a link through (see utils/markdown.ts).
import { renderMarkdown } from "~/utils/markdown";

const props = defineProps<{ source: string }>();
const html = computed(() => renderMarkdown(props.source));
</script>

<template>
  <!-- eslint-disable-next-line vue/no-v-html -->
  <div class="md" v-html="html" />
</template>

<style scoped>
.md { font-size: 0.875rem; line-height: 1.7; overflow-wrap: anywhere; }
.md :deep(> * + *) { margin-top: 0.75rem; }
.md :deep(h1), .md :deep(h2), .md :deep(h3), .md :deep(h4) { font-weight: 600; color: var(--ui-text-highlighted); }
.md :deep(h1) { font-size: 1.25rem; }
.md :deep(h2) { font-size: 1.125rem; }
.md :deep(ul) { list-style: disc; padding-left: 1.25rem; }
.md :deep(ol) { list-style: decimal; padding-left: 1.25rem; }
.md :deep(blockquote) { border-left: 3px solid var(--ui-border); padding-left: 0.75rem; color: var(--ui-text-muted); }
.md :deep(code) { font-family: ui-monospace, monospace; font-size: 0.8125rem; background: var(--ui-bg-elevated); border-radius: 0.25rem; padding: 0.05rem 0.3rem; }
.md :deep(pre) { background: var(--ui-bg-elevated); border-radius: 0.5rem; padding: 0.75rem; overflow-x: auto; white-space: pre; }
.md :deep(pre code) { background: transparent; padding: 0; }
.md :deep(table) { border-collapse: collapse; }
.md :deep(th), .md :deep(td) { border: 1px solid var(--ui-border); padding: 0.25rem 0.5rem; }
.md :deep(hr) { border-color: var(--ui-border); }
.md :deep(.md-link) { color: var(--ui-primary); text-decoration: underline; text-decoration-style: dotted; cursor: default; }
.md :deep(.md-image) { color: var(--ui-text-muted); }
.md :deep(.md-left) { text-align: left; }
.md :deep(.md-center) { text-align: center; }
.md :deep(.md-right) { text-align: right; }
</style>
