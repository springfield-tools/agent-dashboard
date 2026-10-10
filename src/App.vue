<template>
  <div class="popover">
    <header class="header">
      <span class="header-title">Agent Dashboard</span>
      <span class="header-meta">Phase 1 · {{ sectionCount }} sections · placeholder data</span>
    </header>
    <main class="sections">
      <section v-for="group in groups" :key="group.id" class="section">
        <h2 class="section-title">{{ group.title }} · 1</h2>
        <div class="row" aria-disabled="true">
          <span class="row-repo">springfield-tools</span>
          <span class="row-number">#0</span>
          <span class="row-title">{{ group.placeholderTitle }}</span>
        </div>
      </section>
    </main>
    <footer class="footer">
      <span class="footer-note">Placeholder rows are not links yet. See README and docs/PLAN.md in the repo.</span>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { computed, onMounted, ref } from 'vue';

import { PLACEHOLDER_GROUPS, type PlaceholderGroup } from './groups';

const groups: PlaceholderGroup[] = [...PLACEHOLDER_GROUPS];
const rustGroupCount = ref<number | null>(null);

onMounted(async () => {
  try {
    rustGroupCount.value = await invoke<number>('placeholder_group_count');
  } catch {
    rustGroupCount.value = null;
  }
});

const sectionCount = computed(() => rustGroupCount.value ?? groups.length);
</script>
