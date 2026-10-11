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
        <template v-if="snapshot?.stale"> · CI may be stale</template>
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
      <p v-else-if="actionBanner" class="banner banner-warn">{{ actionBanner }}</p>
      <p v-else-if="configStatus?.banner" class="banner banner-warn">{{ configStatus.banner }}</p>
      <p v-if="writeUi?.toast" class="banner banner-info">{{ writeUi.toast }}</p>
      <div v-if="!configStatus?.fixtureMode" class="controls">
        <label class="control-label">
          Poll
          <select
            class="control-select"
            :value="configStatus?.pollIntervalSeconds ?? DEFAULT_POLL_INTERVAL_SECONDS"
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
        <div
          v-for="row in group.items"
          :key="`${row.repo}-${row.number}`"
          class="row row-wrap"
        >
          <button type="button" class="row row-button row-main" @click="openRow(row.url)">
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
          <button
            v-if="!configStatus?.fixtureMode && rowMenuEntries(group.id).length"
            type="button"
            class="row-menu-btn"
            title="Actions"
            @click.stop="openRowPicker(group.id, row)"
          >
            ⋯
          </button>
        </div>
      </section>
      <p v-if="!snapshot?.groups.length" class="empty">No open items in this view.</p>
    </main>
    <div v-if="writeUi?.undoAvailable" class="undo-bar">
      <button type="button" class="btn-text" @click="onUndo">Undo last action</button>
      <span v-if="writeUi.undoMessage" class="undo-hint">{{ writeUi.undoMessage }}</span>
    </div>
    <div v-if="writeUi?.partialFailure" class="undo-bar">
      <button type="button" class="btn-text" @click="onRetryPartial">
        Retry {{ writeUi.partialFailure.kind === 'retryLabels' ? 'labels' : 'comment' }}
      </button>
      <span class="undo-hint">{{ writeUi.partialFailure.message }}</span>
    </div>

    <dialog v-if="rowPicker" class="settings-sheet" open @click.self="rowPicker = null">
      <div class="settings-panel" @click.stop>
        <h2 class="settings-title">Row actions</h2>
        <p class="action-target">
          {{ rowPicker.row.repo }}#{{ rowPicker.row.number }}
        </p>
        <button
          v-for="entry in rowMenuEntries(rowPicker.groupId)"
          :key="entry.label"
          type="button"
          class="btn-text row-picker-btn"
          @click="startAction(rowPicker.groupId, rowPicker.row, entry)"
        >
          {{ entry.label }}
        </button>
        <button type="button" class="btn-text" @click="rowPicker = null">Cancel</button>
      </div>
    </dialog>

    <dialog v-if="actionSheetOpen" class="settings-sheet" open @click.self="closeActionSheet">
      <div class="settings-panel action-panel" @click.stop>
        <h2 class="settings-title">{{ actionSheetTitle }}</h2>
        <p v-if="actionTarget" class="action-target">
          {{ actionTarget.row.repo }}#{{ actionTarget.row.number }} · {{ actionTarget.row.title }}
        </p>
        <p v-if="actionPreview" class="action-diff">
          <strong>Labels</strong>
          <span v-if="actionPreview.labelsToRemove.length">
            − {{ actionPreview.labelsToRemove.join(', ') }}
          </span>
          <span v-if="actionPreview.labelsToAdd.length">
            + {{ actionPreview.labelsToAdd.join(', ') }}
          </span>
        </p>
        <pre v-if="actionPreview" class="action-comment">{{ actionPreview.commentBody }}</pre>
        <label v-if="actionNeedsNote" class="settings-label">
          Note
          <textarea v-model="actionNote" class="settings-input action-note" rows="3" />
        </label>
        <label v-else-if="showOptionalNote" class="settings-label">
          Optional note
          <textarea v-model="actionNote" class="settings-input action-note" rows="2" />
        </label>
        <p v-if="actionError" class="banner banner-error">{{ actionError }}</p>
        <div class="settings-actions action-buttons">
          <template v-if="actionSheetMode === 'testingPassed'">
            <button
              type="button"
              class="btn-primary"
              :disabled="actionBusy"
              @click="confirmAction('testingPassedReady')"
            >
              Ready to merge
            </button>
            <button
              type="button"
              class="btn-text"
              :disabled="actionBusy"
              @click="confirmAction('testingPassedHandBack')"
            >
              Hand back to agent
            </button>
          </template>
          <template v-else>
            <button
              type="button"
              class="btn-primary"
              :disabled="actionBusy"
              @click="confirmAction(actionSheetMode as RowActionKind)"
            >
              Confirm
            </button>
          </template>
          <button type="button" class="btn-text" :disabled="actionBusy" @click="closeActionSheet">
            Cancel
          </button>
        </div>
      </div>
    </dialog>

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
      <span v-if="graphqlBudgetLabel" class="footer-budget">{{ graphqlBudgetLabel }}</span>
      <span v-if="configStatus" class="footer-path">{{ configStatus.configPath }}</span>
    </footer>

    <dialog v-if="settingsOpen" class="settings-sheet" open @click.self="settingsOpen = false">
      <div class="settings-panel" @click.stop>
        <h2 class="settings-title">Settings</h2>
        <p v-if="configStatus?.credentialSource === 'gh'" class="settings-hint">
          <code>credential = "gh"</code> in config: each poll runs
          <code>gh auth token</code> (searches PATH plus Homebrew paths). The token stays in memory
          only for this session and is never written to config, snapshot, or logs. Run
          <code>gh auth login</code> if you see an auth banner. Check names in CI rollup may work
          better than a fine-grained PAT.
        </p>
        <template v-else>
          <p class="settings-hint">
            Fine-grained PAT for resource owner springfield-ham-radio: Metadata and Commit
            statuses (Read). Row actions need Issues and Pull requests <strong>Read and write</strong>.
            Contents stays off. No Checks permission on fine-grained tokens. Org
            may need to allow or approve the token. Stored in Keychain only — never in config or
            snapshot.
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
          </div>
        </template>
        <div class="settings-actions">
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
  DEFAULT_POLL_INTERVAL_SECONDS,
  POLL_INTERVAL_PRESETS,
  type ConfigStatus,
  type DashboardRow,
  type DashboardSnapshot,
  type RowActionKind,
  type RowActionPreview,
  type WriteUiState,
} from './types/dashboard';

const snapshot = ref<DashboardSnapshot | null>(null);
const configStatus = ref<ConfigStatus | null>(null);
const writeUi = ref<WriteUiState | null>(null);
const settingsOpen = ref(false);
const tokenInput = ref('');
const tokenMessage = ref('');
const actionBanner = ref<string | null>(null);
const actionSheetOpen = ref(false);
const actionSheetMode = ref<RowActionKind | 'testingPassed'>('testingPassedReady');
const actionSheetTitle = ref('');
const actionTarget = ref<{ groupId: string; row: DashboardRow } | null>(null);
const actionPreview = ref<RowActionPreview | null>(null);
const actionNote = ref('');
const actionError = ref('');
const actionBusy = ref(false);
const rowPicker = ref<{ groupId: string; row: DashboardRow } | null>(null);
let unlistenUpdated: (() => void) | undefined;
let unlistenSettings: (() => void) | undefined;

async function reload(): Promise<void> {
  try {
    snapshot.value = await invoke<DashboardSnapshot>('dashboard_snapshot');
    configStatus.value = await invoke<ConfigStatus>('config_status');
    writeUi.value = await invoke<WriteUiState>('write_ui_state_cmd');
  } catch {
    snapshot.value = null;
  }
}

function rowMenuEntries(groupId: string): { mode: RowActionKind | 'testingPassed'; label: string }[] {
  switch (groupId) {
    case 'needs-testing':
      return [
        { mode: 'testingPassed', label: 'Testing passed…' },
        { mode: 'sendBackToAgent', label: 'Send back to agent' },
      ];
    case 'ci-failing':
      return [{ mode: 'sendBackToAgent', label: 'Send back to agent' }];
    case 'needs-input':
      return [{ mode: 'answer', label: 'Answer' }];
    case 'not-started':
    case 'agent-working':
      return [{ mode: 'approveReadyToMerge', label: 'Approve / ready to merge' }];
    default:
      return [];
  }
}

const actionNeedsNote = computed(
  () => actionSheetMode.value === 'answer' || actionPreview.value?.noteRequired,
);
const showOptionalNote = computed(() => {
  if (!actionPreview.value?.wakesAgent || actionSheetMode.value === 'answer') {
    return false;
  }
  if (actionSheetMode.value === 'testingPassed') {
    return true;
  }
  return (
    actionSheetMode.value !== 'testingPassedReady' &&
    actionSheetMode.value !== 'approveReadyToMerge'
  );
});

async function refreshPreview(action: RowActionKind): Promise<void> {
  if (!actionTarget.value) {
    return;
  }
  const { groupId, row } = actionTarget.value;
  actionPreview.value = await invoke<RowActionPreview>('preview_row_action_cmd', {
    groupId,
    org: row.org,
    repo: row.repo,
    number: row.number,
    action,
    note: actionNote.value,
  });
}

function openRowPicker(groupId: string, row: DashboardRow): void {
  rowPicker.value = { groupId, row };
}

async function startAction(
  groupId: string,
  row: DashboardRow,
  entry: { mode: RowActionKind | 'testingPassed'; label: string },
): Promise<void> {
  rowPicker.value = null;
  actionError.value = '';
  actionBanner.value = null;
  actionTarget.value = { groupId, row };
  if (entry.mode === 'testingPassed') {
    actionSheetMode.value = 'testingPassed';
    actionSheetTitle.value = 'Testing passed';
    actionNote.value = '';
    await refreshPreview('testingPassedReady');
  } else {
    actionSheetMode.value = entry.mode;
    actionSheetTitle.value = entry.label;
    actionNote.value = '';
    await refreshPreview(entry.mode);
  }
  actionSheetOpen.value = true;
}

function closeActionSheet(): void {
  actionSheetOpen.value = false;
  actionTarget.value = null;
  actionPreview.value = null;
  actionError.value = '';
}

async function confirmAction(action: RowActionKind): Promise<void> {
  if (!actionTarget.value) {
    return;
  }
  actionBusy.value = true;
  actionError.value = '';
  actionBanner.value = null;
  const { groupId, row } = actionTarget.value;
  try {
    await refreshPreview(action);
    await invoke('confirm_row_action', {
      groupId,
      org: row.org,
      repo: row.repo,
      number: row.number,
      action,
      note: actionNote.value,
    });
    closeActionSheet();
    await reload();
  } catch (e) {
    actionError.value = String(e);
    actionBanner.value = String(e);
    await reload();
  } finally {
    actionBusy.value = false;
  }
}

async function onUndo(): Promise<void> {
  try {
    await invoke('undo_row_action_cmd');
    await reload();
  } catch (e) {
    actionBanner.value = String(e);
  }
}

async function onRetryPartial(): Promise<void> {
  try {
    await invoke('retry_partial_write_cmd');
    actionBanner.value = null;
    await reload();
  } catch (e) {
    actionBanner.value = String(e);
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

const graphqlBudgetLabel = computed(() => {
  const cost = snapshot.value?.graphqlPollCost;
  const remaining = snapshot.value?.graphqlRemaining;
  if (cost == null && remaining == null) {
    return '';
  }
  const pts = cost != null ? `${cost} pts/poll` : '— pts/poll';
  const left = remaining != null ? `${remaining.toLocaleString()} left` : '— left';
  return `${pts} · ${left}`;
});

const intervalLabel = computed(() => {
  const secs = configStatus.value?.pollIntervalSeconds ?? DEFAULT_POLL_INTERVAL_SECONDS;
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
