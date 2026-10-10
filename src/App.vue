<template>
  <div class="popover">
    <header class="header">
      <span class="header-title">Agent Dashboard</span>
      <span class="header-meta">Phase 1 · placeholder data</span>
    </header>
    <main class="sections">
      <section v-for="group in groups" :key="group.id" class="section">
        <h2 class="section-title">{{ group.title }} · 1</h2>
        <button type="button" class="row" @click="openPlan">
          <span class="row-repo">springfield-tools</span>
          <span class="row-number">#0</span>
          <span class="row-title">{{ group.placeholderTitle }}</span>
        </button>
      </section>
    </main>
    <footer class="footer">
      <a class="footer-link" href="#" @click.prevent="openPlan">Implementation plan</a>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';

import { PLACEHOLDER_GROUPS, type PlaceholderGroup } from './groups';

const groups: PlaceholderGroup[] = [...PLACEHOLDER_GROUPS];

const PLAN_URL = 'https://github.com/springfield-tools/agent-dashboard/blob/main/docs/PLAN.md';

async function openPlan(): Promise<void> {
  try {
    await openUrl(PLAN_URL);
  } catch {
    await invoke('open_plan_url');
  }
}
</script>
