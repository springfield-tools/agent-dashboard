<template>
  <div class="popover">
    <header class="header">
      <div class="header-row">
        <span class="header-title">Agent Dashboard</span>
        <div class="header-actions">
          <button type="button" class="btn-icon" title="Refresh" @click="onRefresh">↻</button>
          <button type="button" class="btn-icon" title="Settings" @click="settingsOpen = true">
            ⚙
          </button>
        </div>
      </div>
      <span class="header-meta">
        <template v-if="configStatus?.fixtureMode">Fixture ·</template>
        <template v-else-if="configStatus?.pollingPaused">Paused ·</template>
        updated {{ fetchedLabel }}
        <template v-if="configStatus"> · every {{ intervalLabel }}</template>
      </span>
      <p v-if="configStatus?.configError" class="banner banner-error">
        {{ configStatus.configError }}
      </p>
      <p v-else-if="configStatus?.authError" class="banner banner-error">
        {{ configStatus.authError }}
      </p>
      <p v-else-if="configStatus?.offline" class="banner banner-warn">
        Offline · last updated {{ fetchedLabel }}
      </p>
      <p v-else-if="configStatus?.pollingPaused" class="banner banner-info">
        {{ configStatus.pauseTooltip }}
      </p>
      <p v-else-if="configStatus?.banner" class="banner banner-warn">{{ configStatus.banner }}</p>
      <div v-if="!configStatus?.fixtureMode" class="controls">
        <label class="control-label">
          Poll
          <select
            class="control-select"
            :value="configStatus?.pollIntervalSeconds ?? 30"
            @change="onIntervalChange"
          >
            <option v-for="preset in POLL_INTERVAL_PRESETS" :key="preset.seconds" :value="preset.seconds">
              {{ preset.label }}
            </option>
          </select>
        </label>
        <button
          v-if="configStatus?.pollingPaused"
          type="button"
          class="btn-text"
          @click="onResume"
        >
          Resume
        </button>
        <template v-else>
          <button type="button" class="btn-text" @click="onPause('manual')">Pause</button>
          <button type="button" class="btn-text" @click="onPause('hour')">1h</button>
          <button type="button" class="btn-text" @click="onPause('tomorrow')">Until midnight</button>
        </template>
      </div>
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
        Dependabot · {{ snapshot.bots.green }} green · {{ snapshot.bots.noCi }} no CI<template
          v-if="snapshot.bots.pending > 0"
        >
          · {{ snapshot.bots.pending }} pending
        </template><template v-if="snapshot.bots.failing > 0">
          · {{ snapshot.bots.failing }} failing
        </template>
      </button>
      <span v-if="configStatus" class="footer-path">{{ configStatus.configPath }}</span>
    </footer>

    <dialog v-if="settingsOpen" class="settings-sheet" open @click.self="settingsOpen = false">
      <div class="settings-panel" @click.stop>
        <h2 class="settings-title">Settings</h2>
        <p class="settings-hint">
          Fine-grained PAT for resource owner springfield-ham-radio: Metadata, Issues, Pull
          requests, and Commit statuses (Read). No Checks permission exists on fine-grained tokens.
          Org may need to allow or approve the token. Stored in Keychain only — never in config or
          snapshot. CI rollup state works; individual check names may be missing unless you use
          <code>credential = "gh"</code> in config.
        </p>
        <label class="settings-label">
          GitHub token
          <input
            v-model="tokenInput"
            type="password"
            class="settings-input"
            autocomplete="off"
            placeholder="Paste fine-grained PAT once"
          />
        </label>
        <p v-if="tokenMessage" class="settings-message">{{ tokenMessage }}</p>
        <div class="settings-actions">
          <button type="button" class="btn-primary" @click="saveToken">Save token</button>
          <button
            v-if="configStatus?.hasGithubToken"
            type="button"
            class="btn-text"
            @click="clearToken"
          >
            Remove token
          </button>
          <button type="button" class="btn-text" @click="settingsOpen = false">Close</button>
        </div>
      </div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
import { openUrl } from '@tauri-apps/plugin-opener';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { computed, onMounted, onUnmounted, ref } from 'vue';

import { formatRelativeTime } from './formatRelative';
import {
  POLL_INTERVAL_PRESETS,
  type ConfigStatus,
  type DashboardSnapshot,
} from './types/dashboard';

const snapshot = ref<DashboardSnapshot | null>(null);
const configStatus = ref<ConfigStatus | null>(null);
const settingsOpen = ref(false);
const tokenInput = ref('');
const tokenMessage = ref('');
let unlistenUpdated: (() => void) | undefined;
let unlistenSettings: (() => void) | undefined;

async function reload(): Promise<void> {
  try {
    snapshot.value = await invoke<DashboardSnapshot>('dashboard_snapshot');
    configStatus.value = await invoke<ConfigStatus>('config_status');
  } catch {
    snapshot.value = null;
  }
}

onMounted(async () => {
  await reload();
  unlistenUpdated = await listen('dashboard-updated', () => {
    void reload();
  });
  unlistenSettings = await listen('open-settings', () => {
    settingsOpen.value = true;
  });
});

onUnmounted(() => {
  unlistenUpdated?.();
  unlistenSettings?.();
});

const fetchedLabel = computed(() => {
  if (!snapshot.value?.fetchedAt) {
    return '—';
  }
  return formatRelativeTime(snapshot.value.fetchedAt);
});

const intervalLabel = computed(() => {
  const secs = configStatus.value?.pollIntervalSeconds ?? 30;
  const preset = POLL_INTERVAL_PRESETS.find((p) => p.seconds === secs);
  return preset?.label ?? `${secs}s`;
});

async function openRow(url: string): Promise<void> {
  await openUrl(url);
}

async function onRefresh(): Promise<void> {
  await invoke('refresh_now');
  await reload();
}

async function onIntervalChange(event: Event): Promise<void> {
  const select = event.target as HTMLSelectElement;
  const seconds = Number(select.value);
  try {
    await invoke('set_poll_interval', { seconds });
    tokenMessage.value = '';
    await reload();
  } catch (e) {
    tokenMessage.value = String(e);
    await reload();
  }
}

async function onPause(mode: string): Promise<void> {
  await invoke('pause_polling', { mode });
  await reload();
}

async function onResume(): Promise<void> {
  await invoke('resume_polling');
  await reload();
}

async function saveToken(): Promise<void> {
  tokenMessage.value = '';
  try {
    await invoke('store_github_token', { token: tokenInput.value });
    tokenInput.value = '';
    tokenMessage.value = 'Token saved to Keychain.';
    await invoke('refresh_now');
    await reload();
  } catch (e) {
    tokenMessage.value = String(e);
  }
}

async function clearToken(): Promise<void> {
  await invoke('clear_github_token');
  tokenInput.value = '';
  tokenMessage.value = 'Token removed.';
  await reload();
}
</script>
