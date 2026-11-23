<script setup lang="ts">
import type { LaneDef } from "~/types/binone";
import { laneColor, laneText } from "~/utils/requests";

const props = defineProps<{ lane: string; lanes?: LaneDef[] | null }>();
const text = computed(() => laneText(props.lane, props.lanes));
</script>

<template>
  <UBadge :color="laneColor(lane)" variant="subtle" size="sm" data-testid="lane-badge" :data-lane="lane">{{ text }}</UBadge>
</template>
