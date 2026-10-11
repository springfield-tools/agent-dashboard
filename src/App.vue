<template>
  <div ref="popoverEl" class="popover">
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
      <template v-if="orgSections.length">
        <details
          v-for="orgSection in orgSections"
          :key="orgSection.org"
          class="org-section"
          :open="!isOrgCollapsed(orgSection.org)"
          @toggle="onOrgToggle(orgSection.org, $event)"
        >
          <summary class="org-header">
            <span class="org-chevron" aria-hidden="true">›</span>
            <span class="org-name">{{ orgSection.org }}</span>
            <span v-if="orgSection.trayTitle" class="org-counts">{{ orgSection.trayTitle }}</span>
          </summary>
          <p v-if="orgSection.authBanner" class="banner banner-warn org-auth-banner">
            {{ orgSection.authBanner }}
          </p>
          <section
            v-for="group in orgSection.groups"
            :key="`${orgSection.org}-${group.id}`"
            class="section"
          >
            <h2 class="section-title">{{ group.title }} · {{ group.items.length }}</h2>
            <div
              v-for="row in group.items"
              :key="`${row.org}-${row.repo}-${row.number}`"
              class="row"
              @contextmenu.prevent="onRowContextMenu($event, group.id, row)"
            >
              <div class="row-surface">
                <button type="button" class="row-button" @click="openRow(row.url)">
                  <span class="row-top">
                    <span class="row-top-meta">
                      <span class="row-repo">{{ row.repo }}</span>
                      <span class="row-number">#{{ row.number }}</span>
                      <span :class="kindChipClass(row.kind, row.draft)">
                        {{ row.draft ? 'Draft' : row.kind === 'pr' ? 'PR' : 'Issue' }}
                      </span>
                    </span>
                  </span>
                  <span class="row-title">{{ row.title }}</span>
                  <span class="row-meta">
                    <span class="row-author">@{{ row.author }}</span>
                    <span class="row-time">{{ formatRelativeTime(row.updatedAt) }}</span>
                  </span>
                  <span v-if="row.chips.length" class="row-chips">
                    <span
                      v-for="chip in row.chips"
                      :key="`${chip.label}-${chip.variant}`"
                      :class="chipClass(chip.variant)"
                    >
                      {{ chip.label }}
                    </span>
                  </span>
                </button>
                <button
                  v-if="!configStatus?.fixtureMode && rowMenuEntries(group.id).length"
                  type="button"
                  class="row-more"
                  :aria-label="`Actions for ${row.repo} #${row.number}`"
                  aria-haspopup="menu"
                  :aria-expanded="isContextMenuForRow(group.id, row)"
                  @click.stop="openContextMenu($event, group.id, row)"
                >
                  ⋯
                </button>
              </div>
            </div>
          </section>
          <p v-if="!orgSection.groups.length && !orgSection.authBanner" class="empty org-empty">
            No open items in this org.
          </p>
        </details>
      </template>
      <template v-else>
        <section v-for="group in snapshot?.groups ?? []" :key="group.id" class="section">
          <h2 class="section-title">{{ group.title }} · {{ group.items.length }}</h2>
          <div
            v-for="row in group.items"
            :key="`${row.org}-${row.repo}-${row.number}`"
            class="row"
            @contextmenu.prevent="onRowContextMenu($event, group.id, row)"
          >
            <div class="row-surface">
              <button type="button" class="row-button" @click="openRow(row.url)">
                <span class="row-top">
                  <span class="row-top-meta">
                    <span class="row-repo">{{ row.repo }}</span>
                    <span class="row-number">#{{ row.number }}</span>
                    <span :class="kindChipClass(row.kind, row.draft)">
                      {{ row.draft ? 'Draft' : row.kind === 'pr' ? 'PR' : 'Issue' }}
                    </span>
                  </span>
                </span>
                <span class="row-title">{{ row.title }}</span>
                <span class="row-meta">
                  <span class="row-author">@{{ row.author }}</span>
                  <span class="row-time">{{ formatRelativeTime(row.updatedAt) }}</span>
                </span>
                <span v-if="row.chips.length" class="row-chips">
                  <span
                    v-for="chip in row.chips"
                    :key="`${chip.label}-${chip.variant}`"
                    :class="chipClass(chip.variant)"
                  >
                    {{ chip.label }}
                  </span>
                </span>
              </button>
              <button
                v-if="!configStatus?.fixtureMode && rowMenuEntries(group.id).length"
                type="button"
                class="row-more"
                :aria-label="`Actions for ${row.repo} #${row.number}`"
                aria-haspopup="menu"
                :aria-expanded="isContextMenuForRow(group.id, row)"
                @click.stop="openContextMenu($event, group.id, row)"
              >
                ⋯
              </button>
            </div>
          </div>
        </section>
      </template>
      <p v-if="!orgSections.length && !snapshot?.groups.length" class="empty">No open items in this view.</p>
    </main>
    <div v-if="writeUi?.undoAvailable" class="undo-bar">
      <button type="button" class="btn-text" @click="onUndo">Undo last action</button>
      <span v-if="writeUi.undoMessage" class="undo-hint">{{ writeUi.undoMessage }}</span>
    </div>
    <div v-if="writeUi?.partialFailure" class="undo-bar">
      <button type="button" class="btn-text" @click="onRetryPartial">
        Retry labels
      </button>
      <span class="undo-hint">{{ writeUi.partialFailure.message }}</span>
    </div>

    <div
      v-if="contextMenu"
      class="context-menu-backdrop"
      aria-hidden="true"
      @click="closeContextMenu"
      @contextmenu.prevent="closeContextMenu"
    />
    <div
      v-if="contextMenu"
      ref="contextMenuEl"
      class="context-menu"
      role="menu"
      :style="contextMenuStyle"
      @keydown.escape.prevent="closeContextMenu"
    >
      <button
        v-for="(entry, index) in rowMenuEntries(contextMenu.groupId)"
        :key="entry.label"
        type="button"
        class="context-menu-item"
        role="menuitem"
        :tabindex="index === 0 ? 0 : -1"
        @click="pickContextMenuAction(entry)"
      >
        {{ entry.label }}
      </button>
    </div>

    <div
      v-if="actionSheetOpen"
      class="action-sheet-overlay"
      @click.self="closeActionSheet"
    >
      <div
        ref="actionSheetEl"
        class="action-sheet"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="actionSheetTitleId"
        @keydown.escape.prevent="closeActionSheet"
      >
        <h2 :id="actionSheetTitleId" class="settings-title">{{ actionSheetTitle }}</h2>
        <p v-if="actionTarget" class="action-target">
          {{ actionTarget.row.repo }}#{{ actionTarget.row.number }} · {{ actionTarget.row.title }}
        </p>
        <div v-if="actionPreview" class="action-diff">
          <span class="action-diff-label">Labels</span>
          <span class="row-chips action-diff-chips">
            <span
              v-for="name in actionPreview.labelsToRemove"
              :key="`rm-${name}`"
              class="chip chip-label-remove"
            >
              − {{ name }}
            </span>
            <span
              v-for="name in actionPreview.labelsToAdd"
              :key="`add-${name}`"
              class="chip chip-label-add"
            >
              + {{ name }}
            </span>
          </span>
        </div>
        <pre v-if="actionPreview" class="action-comment">{{ actionPreview.commentBody }}</pre>
        <div v-if="actionSheetMode === 'testingPassed'" class="testing-passed-choice">
          <label class="control-label">
            <input
              v-model="testingPassedChoice"
              type="radio"
              value="testingPassedReady"
              @change="onTestingPassedChoiceChange"
            />
            Ready to merge
          </label>
          <label class="control-label">
            <input
              v-model="testingPassedChoice"
              type="radio"
              value="testingPassedHandBack"
              @change="onTestingPassedChoiceChange"
            />
            Hand back to agent
          </label>
        </div>
        <label v-if="actionNeedsNote" class="settings-label">
          Answer
          <textarea
            v-model="actionNote"
            class="settings-input action-note"
            rows="3"
            @input="onActionNoteInput"
          />
        </label>
        <label v-else-if="showOptionalNote" class="settings-label">
          Optional note
          <textarea
            v-model="actionNote"
            class="settings-input action-note"
            rows="2"
            @input="onActionNoteInput"
          />
        </label>
        <p v-if="actionError" class="banner banner-error">{{ actionError }}</p>
        <div class="action-sheet-footer">
          <template v-if="actionSheetMode === 'testingPassed'">
            <button
              type="button"
              class="btn-primary"
              :disabled="actionBusy"
              @click="confirmAction(testingPassedChoice)"
            >
              Confirm
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
    </div>

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
        <p class="settings-hint">
          Each org can use <code>credential = "keychain"</code> (per-org Keychain item) or
          <code>credential = "gh"</code> on that <code>[[orgs]]</code> row. Fine-grained PATs need
          Metadata and Commit statuses (Read); row actions need Issues and Pull requests
          <strong>Read and write</strong>. Tokens stay in Keychain only — never in config or
          snapshot.
        </p>
        <p v-if="missingDefaultOrgs.length" class="banner banner-warn">
          Config lists {{ listedOrgNames }}. An existing config.toml is not upgraded in place.
          Quit the app and append this to
          <code>{{ configStatus?.configPath }}</code>, then relaunch:
        </p>
        <pre v-if="missingDefaultOrgs.length" class="action-comment">{{ missingOrgsToml }}</pre>
        <div
          v-for="entry in configStatus?.orgAuth ?? []"
          :key="entry.name"
          class="org-token-block"
        >
            <h3 class="settings-subtitle">
              {{ entry.name }}
              <span class="org-auth-pill" :class="{ ready: entry.ready }">
                {{ entry.credentialSource }} · {{ entry.ready ? 'ready' : 'needs token' }}
              </span>
            </h3>
            <template v-if="entry.credentialSource === 'keychain'">
              <label class="settings-label">
                GitHub token
                <input
                  v-model="tokenInputs[entry.name]"
                  type="password"
                  class="settings-input"
                  autocomplete="off"
                  :placeholder="`PAT for ${entry.name}`"
                />
              </label>
              <div class="settings-actions">
                <button type="button" class="btn-primary" @click="saveToken(entry.name)">
                  Save token
                </button>
                <button
                  v-if="entry.ready"
                  type="button"
                  class="btn-text"
                  @click="clearToken(entry.name)"
                >
                  Remove token
                </button>
              </div>
            </template>
          <p v-else class="settings-hint">
            Uses <code>gh auth token</code> for this org (shared CLI session).
          </p>
        </div>
        <p v-if="tokenMessage" class="settings-message">{{ tokenMessage }}</p>
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
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

import { chipClass, kindChipClass } from './chipVariants';
import { formatRelativeTime } from './formatRelative';
import {
  computeContextMenuPosition,
  estimateContextMenuHeight,
  rowMenuEntries,
} from './rowActionMenu';
import {
  DEFAULT_POLL_INTERVAL_SECONDS,
  POLL_INTERVAL_PRESETS,
  type ConfigStatus,
  type DashboardOrgSection,
  type DashboardRow,
  type DashboardSnapshot,
  type RowActionKind,
  type RowActionPreview,
  type UiState,
  type WriteUiState,
} from './types/dashboard';

const snapshot = ref<DashboardSnapshot | null>(null);
const configStatus = ref<ConfigStatus | null>(null);
const writeUi = ref<WriteUiState | null>(null);
const settingsOpen = ref(false);
const tokenInputs = ref<Record<string, string>>({});
const tokenMessage = ref('');
const uiState = ref<UiState>({ orgCollapsed: {} });
const actionBanner = ref<string | null>(null);
const actionSheetOpen = ref(false);
const actionSheetMode = ref<RowActionKind | 'testingPassed'>('testingPassedReady');
const actionSheetTitle = ref('');
const actionTarget = ref<{ groupId: string; row: DashboardRow } | null>(null);
const actionPreview = ref<RowActionPreview | null>(null);
const actionNote = ref('');
const actionError = ref('');
const actionBusy = ref(false);
const popoverEl = ref<HTMLElement | null>(null);
const contextMenuEl = ref<HTMLElement | null>(null);
const actionSheetEl = ref<HTMLElement | null>(null);
const contextMenu = ref<{
  groupId: string;
  row: DashboardRow;
  left: number;
  top: number;
} | null>(null);
const contextMenuStyle = computed(() => {
  if (!contextMenu.value) {
    return {};
  }
  return {
    left: `${contextMenu.value.left}px`,
    top: `${contextMenu.value.top}px`,
  };
});
const actionSheetTitleId = 'action-sheet-title';
const testingPassedChoice = ref<RowActionKind>('testingPassedReady');
let previewDebounce: ReturnType<typeof setTimeout> | undefined;
let unlistenUpdated: (() => void) | undefined;
let unlistenSettings: (() => void) | undefined;

async function reload(): Promise<void> {
  try {
    snapshot.value = await invoke<DashboardSnapshot>('dashboard_snapshot');
    configStatus.value = await invoke<ConfigStatus>('config_status');
    writeUi.value = await invoke<WriteUiState>('write_ui_state_cmd');
    uiState.value = await invoke<UiState>('ui_state_cmd');
  } catch {
    snapshot.value = null;
  }
}

const DEFAULT_ORG_NAMES = [
  'springfield-ham-radio',
  'springfield-tools',
  'springfield-usa',
] as const;

const listedOrgNames = computed(() =>
  (configStatus.value?.orgs ?? []).map((org) => org.name).join(', ') || 'no orgs',
);

const missingDefaultOrgs = computed(() => {
  const present = new Set((configStatus.value?.orgs ?? []).map((org) => org.name));
  return DEFAULT_ORG_NAMES.filter((name) => !present.has(name));
});

const missingOrgsToml = computed(() =>
  missingDefaultOrgs.value
    .map((name) => `[[orgs]]\nname = "${name}"\ninclude = []\nexclude = []`)
    .join('\n\n'),
);

const orgSections = computed((): DashboardOrgSection[] => {
  const sections = snapshot.value?.orgSections;
  if (sections && sections.length > 0) {
    return sections;
  }
  return [];
});

function isOrgCollapsed(org: string): boolean {
  return uiState.value.orgCollapsed[org] === true;
}

function onOrgToggle(org: string, event: Event): void {
  const details = event.target as HTMLDetailsElement;
  const collapsed = !details.open;
  uiState.value = {
    ...uiState.value,
    orgCollapsed: { ...uiState.value.orgCollapsed, [org]: collapsed },
  };
  void invoke('set_org_section_collapsed', { org, collapsed });
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
  try {
    actionPreview.value = await invoke<RowActionPreview>('preview_row_action_cmd', {
      groupId,
      org: row.org,
      repo: row.repo,
      number: row.number,
      action,
      note: actionNote.value,
    });
    actionError.value = '';
  } catch (e) {
    actionError.value = String(e);
  }
}

function onActionNoteInput(): void {
  if (previewDebounce) {
    clearTimeout(previewDebounce);
  }
  previewDebounce = setTimeout(() => {
    void refreshPreviewForCurrentSheet();
  }, 200);
}

async function refreshPreviewForCurrentSheet(): Promise<void> {
  if (actionSheetMode.value === 'testingPassed') {
    await refreshPreview(testingPassedChoice.value);
    return;
  }
  if (actionSheetMode.value !== 'testingPassed') {
    await refreshPreview(actionSheetMode.value as RowActionKind);
  }
}

function onTestingPassedChoiceChange(): void {
  void refreshPreview(testingPassedChoice.value);
}

function isContextMenuForRow(groupId: string, row: DashboardRow): boolean {
  if (!contextMenu.value) {
    return false;
  }
  return (
    contextMenu.value.groupId === groupId &&
    contextMenu.value.row.org === row.org &&
    contextMenu.value.row.repo === row.repo &&
    contextMenu.value.row.number === row.number
  );
}

function closeContextMenu(): void {
  contextMenu.value = null;
}

function openContextMenu(event: MouseEvent, groupId: string, row: DashboardRow): void {
  const entries = rowMenuEntries(groupId);
  if (!entries.length || configStatus.value?.fixtureMode) {
    return;
  }
  const popover = popoverEl.value;
  if (!popover) {
    return;
  }
  const rect = popover.getBoundingClientRect();
  const menuHeight = estimateContextMenuHeight(entries.length);
  const pos = computeContextMenuPosition(
    rect,
    event.clientX,
    event.clientY,
    { width: 200, height: menuHeight },
  );
  contextMenu.value = { groupId, row, left: pos.left, top: pos.top };
  void nextTick(() => {
    const first = contextMenuEl.value?.querySelector<HTMLButtonElement>('[role="menuitem"]');
    first?.focus();
  });
}

function onRowContextMenu(event: MouseEvent, groupId: string, row: DashboardRow): void {
  openContextMenu(event, groupId, row);
}

function pickContextMenuAction(entry: {
  mode: RowActionKind | 'testingPassed';
  label: string;
}): void {
  if (!contextMenu.value) {
    return;
  }
  const { groupId, row } = contextMenu.value;
  closeContextMenu();
  void startAction(groupId, row, entry);
}

async function startAction(
  groupId: string,
  row: DashboardRow,
  entry: { mode: RowActionKind | 'testingPassed'; label: string },
): Promise<void> {
  actionError.value = '';
  actionBanner.value = null;
  actionTarget.value = { groupId, row };
  if (entry.mode === 'testingPassed') {
    actionSheetMode.value = 'testingPassed';
    actionSheetTitle.value = 'Testing passed';
    actionNote.value = '';
    testingPassedChoice.value = 'testingPassedReady';
    actionSheetOpen.value = true;
    await refreshPreview('testingPassedReady');
  } else if (entry.mode === 'answer') {
    actionSheetMode.value = entry.mode;
    actionSheetTitle.value = entry.label;
    actionNote.value = '';
    actionSheetOpen.value = true;
    await refreshPreview('answer');
  } else {
    actionSheetMode.value = entry.mode;
    actionSheetTitle.value = entry.label;
    actionNote.value = '';
    actionSheetOpen.value = true;
    await refreshPreview(entry.mode);
  }
}

function closeActionSheet(): void {
  actionSheetOpen.value = false;
  actionTarget.value = null;
  actionPreview.value = null;
  actionError.value = '';
}

function onDocumentKeydown(event: KeyboardEvent): void {
  if (contextMenu.value && event.key === 'Escape') {
    event.preventDefault();
    closeContextMenu();
    return;
  }
  if (!actionSheetOpen.value || actionBusy.value) {
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    closeActionSheet();
    return;
  }
  if (event.key === 'Enter' && event.target instanceof HTMLElement) {
    if (event.target.tagName === 'TEXTAREA') {
      return;
    }
    event.preventDefault();
    if (actionSheetMode.value === 'testingPassed') {
      void confirmAction(testingPassedChoice.value);
    } else {
      void confirmAction(actionSheetMode.value as RowActionKind);
    }
  }
}

watch(actionSheetOpen, (open) => {
  if (open) {
    void nextTick(() => {
      const primary = actionSheetEl.value?.querySelector<HTMLButtonElement>('.btn-primary');
      primary?.focus();
    });
  }
});

async function confirmAction(action: RowActionKind): Promise<void> {
  if (!actionTarget.value) {
    return;
  }
  actionBusy.value = true;
  actionError.value = '';
  actionBanner.value = null;
  const { groupId, row } = actionTarget.value;
  try {
    if (action === 'answer' && !actionNote.value.trim()) {
      actionError.value = 'Answer text is required.';
      return;
    }
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
  document.addEventListener('keydown', onDocumentKeydown);
  await reload();
  unlistenUpdated = await listen('dashboard-updated', () => {
    void reload();
  });
  unlistenSettings = await listen('open-settings', () => {
    settingsOpen.value = true;
  });
});

onUnmounted(() => {
  document.removeEventListener('keydown', onDocumentKeydown);
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
  const budgets = snapshot.value?.graphqlBudgets;
  if (budgets && budgets.length > 0) {
    return budgets
      .map(
        (b) =>
          `${b.credentialKey}: ${b.pollCost} pts/poll · ${b.remaining.toLocaleString()} left`,
      )
      .join(' · ');
  }
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

async function saveToken(org: string): Promise<void> {
  tokenMessage.value = '';
  try {
    const token = tokenInputs.value[org] ?? '';
    await invoke('store_github_token', { org, token });
    tokenInputs.value = { ...tokenInputs.value, [org]: '' };
    tokenMessage.value = `Token saved for ${org}.`;
    await invoke('refresh_now');
    await reload();
  } catch (e) {
    tokenMessage.value = String(e);
  }
}

async function clearToken(org: string): Promise<void> {
  await invoke('clear_github_token', { org });
  tokenInputs.value = { ...tokenInputs.value, [org]: '' };
  tokenMessage.value = `Token removed for ${org}.`;
  await reload();
}
</script>
