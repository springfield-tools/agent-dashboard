<template>
  <div class="popover">
    <header class="header">
      <span class="header-title">Agent Dashboard</span>
      <span class="header-meta">
        Fixture · updated {{ fetchedLabel }}
        <template v-if="configStatus"> · every {{ configStatus.pollIntervalSeconds }}s</template>
      </span>
      <p v-if="configStatus?.configError" class="banner banner-error">
        {{ configStatus.configError }}
      </p>
    </header>
    <main class="sections">
      <section v-for="group in snapshot?.groups ?? []" :key="group.id" class="section">
        <h2 class="section-title">{{ group.title }} · {{ group.items.length }}</h2>
        <button
          v-for="row in group.items"
          :key="`${row.repo}-${row.number}`"
          type="button"
          class="row row-button"
          @click="openRow(row.url)"
        >
          <span class="row-top">
            <span class="row-repo">{{ row.repo }}</span>
            <span class="row-number">#{{ row.number }}</span>
            <span v-if="row.draft" class="chip chip-kind">Draft</span>
            <span v-else class="chip chip-kind">{{ row.kind === 'pr' ? 'PR' : 'Issue' }}</span>
          </span>
          <span class="row-title">{{ row.title }}</span>
          <span class="row-meta">
            <span class="row-author">@{{ row.author }}</span>
            <span class="row-time">{{ formatRelativeTime(row.updatedAt) }}</span>
          </span>
          <span v-if="row.chips.length" class="row-chips">
            <span v-for="chip in row.chips" :key="chip.label" class="chip">{{ chip.label }}</span>
          </span>
        </button>
      </section>
      <p v-if="!snapshot?.groups.length" class="empty">No open items in this view.</p>
    </main>
    <footer class="footer">
      <button
        v-if="snapshot?.bots"
        type="button"
        class="bot-summary"
        @click="openRow(snapshot.bots.searchUrl)"
      >
        Dependabot · {{ snapshot.bots.green }} green · {{ snapshot.bots.noCi }} no CI
      </button>
      <span class="footer-note">
        Phase 2 · fixture data · edit labels or filters.bots in config and restart
      </span>
      <span v-if="configStatus" class="footer-path">{{ configStatus.configPath }}</span>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { openUrl } from '@tauri-apps/plugin-opener';
import { invoke } from '@tauri-apps/api/core';
import { computed, onMounted, ref } from 'vue';

import { formatRelativeTime } from './formatRelative';
import type { ConfigStatus, DashboardSnapshot } from './types/dashboard';

const snapshot = ref<DashboardSnapshot | null>(null);
const configStatus = ref<ConfigStatus | null>(null);

onMounted(async () => {
  try {
    snapshot.value = await invoke<DashboardSnapshot>('dashboard_snapshot');
    configStatus.value = await invoke<ConfigStatus>('config_status');
  } catch {
    snapshot.value = null;
  }
});

const fetchedLabel = computed(() => {
  if (!snapshot.value?.fetchedAt) {
    return '—';
  }

  return formatRelativeTime(snapshot.value.fetchedAt);
});

async function openRow(url: string): Promise<void> {
  await openUrl(url);
}
</script>
