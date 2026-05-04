<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type BlocklistEntry,
    type BlocklistCategory
  } from '$lib/api';

  const CATEGORIES: BlocklistCategory[] = [
    'Drainer',
    'Phishing',
    'Scam',
    'Sanctioned',
    'UserReported'
  ];

  let entries: BlocklistEntry[] = [];
  let error = '';
  let busy = false;

  // New-entry form state.
  let formOpen = false;
  let newAddr = '';
  let newCategory: BlocklistCategory = 'Phishing';
  let newSource = 'user';
  let newNote = '';

  // Bulk-import textarea.
  let importOpen = false;
  let importText = '';

  async function load() {
    error = '';
    try {
      entries = await api.blocklistList();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function resetForm() {
    newAddr = '';
    newCategory = 'Phishing';
    newSource = 'user';
    newNote = '';
  }

  async function add() {
    if (!newAddr.trim()) return;
    busy = true;
    error = '';
    try {
      const entry: BlocklistEntry = {
        address: newAddr.trim(),
        category: newCategory,
        source: newSource.trim() || 'user',
        note: newNote.trim() ? newNote.trim() : null,
        added_at: Math.floor(Date.now() / 1000)
      };
      await api.blocklistAdd(entry);
      resetForm();
      formOpen = false;
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function remove(addr: string) {
    busy = true;
    error = '';
    try {
      await api.blocklistRemove(addr);
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function importJson() {
    if (!importText.trim()) return;
    busy = true;
    error = '';
    try {
      await api.blocklistImportJson(importText);
      importText = '';
      importOpen = false;
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function tone(c: BlocklistCategory): string {
    switch (c) {
      case 'Drainer':
      case 'Sanctioned':
        return 'text-rose-400';
      case 'Phishing':
      case 'Scam':
        return 'text-amber-400';
      case 'UserReported':
        return 'text-fg';
    }
  }

  onMount(load);
</script>

<Card title="Blocklist" subtitle="Addresses Atlas will refuse to send to. Stored locally; never synced.">
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2">
      <Button variant="primary" on:click={() => (formOpen = !formOpen)} disabled={busy}>
        {formOpen ? 'Cancel' : 'Add address'}
      </Button>
      <Button variant="secondary" on:click={() => (importOpen = !importOpen)} disabled={busy}>
        {importOpen ? 'Cancel import' : 'Import JSON'}
      </Button>
      <span class="text-fg-subtle text-xs ml-auto">{entries.length} entries</span>
    </div>

    {#if formOpen}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2">
        <input
          type="text"
          bind:value={newAddr}
          placeholder="0x… or normalised address"
          class="w-full h-9 px-3 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-xs"
        />
        <div class="grid grid-cols-2 gap-2">
          <label class="text-xs text-fg-muted block">
            Category
            <select
              bind:value={newCategory}
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
            >
              {#each CATEGORIES as c}
                <option value={c}>{c}</option>
              {/each}
            </select>
          </label>
          <label class="text-xs text-fg-muted block">
            Source
            <input
              type="text"
              bind:value={newSource}
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
            />
          </label>
        </div>
        <input
          type="text"
          bind:value={newNote}
          placeholder="Optional note (why is this listed?)"
          class="w-full h-9 px-3 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
        />
        <Button variant="primary" on:click={add} disabled={busy || !newAddr.trim()}>
          Add to blocklist
        </Button>
      </div>
    {/if}

    {#if importOpen}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2">
        <p class="text-xs text-fg-muted">
          Paste a JSON array of <span class="font-mono">BlocklistEntry</span> objects.
        </p>
        <textarea
          bind:value={importText}
          rows="4"
          class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-xs"
        ></textarea>
        <Button variant="primary" on:click={importJson} disabled={busy || !importText.trim()}>
          Import
        </Button>
      </div>
    {/if}

    {#if entries.length === 0}
      <p class="text-fg-subtle text-xs">No addresses blocked.</p>
    {:else}
      <ul class="border border-border-subtle rounded-lg divide-y divide-border-subtle max-h-64 overflow-y-auto">
        {#each entries as e}
          <li class="px-3 py-2 flex items-start gap-3">
            <div class="flex-1 min-w-0">
              <p class="font-mono text-xs break-all">{e.address}</p>
              <p class="text-[10px] {tone(e.category)} mt-0.5">
                {e.category} &middot; <span class="text-fg-subtle">{e.source}</span>
              </p>
              {#if e.note}
                <p class="text-xs text-fg-muted mt-0.5">{e.note}</p>
              {/if}
            </div>
            <button
              type="button"
              on:click={() => remove(e.address)}
              disabled={busy}
              class="text-xs text-rose-400 hover:text-rose-300 shrink-0"
            >
              Remove
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</Card>
