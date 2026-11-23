<script setup lang="ts">
import { badgeText } from "~/utils/update";
// Every screen but the connection screen needs a working connection: until there is one, the
// connection screen is shown in place of the page (so a start with no saved address, or a failed
// one, lands on it, and so does a page opened later when the connection was changed).
const route = useRoute();
const { connected, address, restore } = useConnection();
const { check, checkAtStartup } = useUpdate();

onMounted(() => {
  void restore();
  // One check for a newer version after startup; it does not wait for, or disturb, the connection.
  checkAtStartup();
});

const updateBadge = computed(() => badgeText(check.value));

const nav = [
  { label: "依頼の一覧", to: "/requests", match: "/requests" },
  { label: "スイムレーン", to: "/swimlane", match: "/swimlane" },
  { label: "通知", to: "/notifications", match: "/notifications" },
  { label: "設定", to: "/settings", match: "/settings" },
];

function isActive(match: string) {
  return route.path === match || route.path.startsWith(`${match}/`) || (match === "/settings" && route.path === "/projects");
}

// The connection screen page is always allowed; any other page needs the connection.
const showPage = computed(() => connected.value !== null || route.path === "/connect");
</script>

<template>
  <div class="flex h-screen min-w-[640px] flex-col bg-default text-default">
    <header class="flex shrink-0 items-center justify-between gap-4 border-b border-default px-6 py-3" data-testid="app-header">
      <div class="flex min-w-0 items-center gap-6">
        <span class="shrink-0 text-sm font-semibold text-highlighted">binone</span>
        <nav v-if="connected" class="flex min-w-0 items-center gap-1 overflow-x-auto" aria-label="画面" data-testid="app-nav">
          <NuxtLink
            v-for="item in nav"
            :key="item.to"
            :to="item.to"
            class="shrink-0 rounded-md px-3 py-1.5 text-sm whitespace-nowrap transition-colors"
            :class="isActive(item.match) ? 'bg-elevated text-highlighted' : 'text-muted hover:bg-elevated/60 hover:text-default'"
            :aria-current="isActive(item.match) ? 'page' : undefined"
          >
            {{ item.label }}
          </NuxtLink>
        </nav>
      </div>
      <div class="flex min-w-0 shrink items-center gap-3">
      <NuxtLink
        v-if="updateBadge"
        :to="{ path: '/settings', hash: '#version' }"
        class="shrink-0 rounded-md border border-primary/40 bg-primary/10 px-2 py-1 text-xs text-primary transition-colors hover:bg-primary/20"
        data-testid="update-badge"
        title="設定の画面の版の欄を開く"
      >
        {{ updateBadge }}
      </NuxtLink>
      <NuxtLink
        to="/connect"
        class="flex min-w-0 shrink items-center gap-2 rounded-md border border-default px-3 py-1.5 text-xs text-muted transition-colors hover:bg-elevated hover:text-default"
        data-testid="connection-link"
        title="接続の画面を開く"
      >
        <span class="size-2 shrink-0 rounded-full" :class="connected ? 'bg-success' : 'bg-muted'" />
        <template v-if="connected">
          <span class="truncate font-mono" data-testid="header-address">{{ address(connected.host, connected.port) }}</span>
          <span class="shrink-0 font-mono" data-testid="header-version">v{{ connected.version }}</span>
        </template>
        <span v-else class="shrink-0">未接続</span>
      </NuxtLink>
      </div>
    </header>

    <div class="min-h-0 flex-1 overflow-auto">
      <slot v-if="showPage" />
      <ConnectionScreen v-else />
    </div>
    <NotificationBand v-if="connected" />
  </div>
</template>
