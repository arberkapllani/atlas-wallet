<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type Contact,
    type ContactAddress,
    type ContactChain
  } from '$lib/api';

  let contacts: Contact[] = [];
  let query = '';
  let loading = false;
  let error = '';

  // Edit-form state. `editing` is null when adding a new contact, otherwise
  // it's the id of the contact being edited.
  let editing: string | null = null;
  let formOpen = false;
  let name = '';
  let note = '';
  let addresses: ContactAddress[] = [];
  let saving = false;

  // Pending row for adding a new address inside the form.
  const CHAINS: ContactChain[] = ['Bitcoin', 'Ethereum'];
  let newAddrChain: ContactChain = 'Ethereum';
  let newAddrValue = '';

  async function load(q = query) {
    loading = true;
    error = '';
    try {
      contacts = q.trim() ? await api.contactsSearch(q) : await api.contactsList();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  function resetForm() {
    editing = null;
    formOpen = false;
    name = '';
    note = '';
    addresses = [];
    newAddrChain = 'Ethereum';
    newAddrValue = '';
  }

  function startAdd() {
    resetForm();
    formOpen = true;
  }

  function startEdit(c: Contact) {
    editing = c.id;
    name = c.name;
    note = c.note;
    addresses = c.addresses.map((a) => ({ ...a }));
    newAddrChain = 'Ethereum';
    newAddrValue = '';
    formOpen = true;
  }

  function addRow() {
    const v = newAddrValue.trim();
    if (!v) return;
    addresses = [...addresses, { chain: newAddrChain, address: v }];
    newAddrValue = '';
  }

  function removeRow(i: number) {
    addresses = addresses.filter((_, idx) => idx !== i);
  }

  async function save() {
    saving = true;
    error = '';
    try {
      if (editing === null) {
        await api.contactsAdd(name.trim(), note.trim(), addresses);
      } else {
        await api.contactsUpdate(editing, name.trim(), note.trim(), addresses);
      }
      resetForm();
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  async function remove(id: string) {
    error = '';
    try {
      await api.contactsRemove(id);
      if (editing === id) resetForm();
      await load();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearchInput() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => load(query), 150);
  }

  onMount(load);
</script>

<Card title="Contacts" subtitle="Address book stored locally as encrypted JSON.">
  <div class="space-y-4 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2">
      <input
        type="text"
        bind:value={query}
        on:input={onSearchInput}
        placeholder="Search by name or note…"
        class="flex-1 h-10 px-3 rounded-lg bg-bg-elevated border border-border-subtle text-fg focus:outline-none focus:ring-2 focus:ring-accent/40"
      />
      <Button variant="secondary" on:click={startAdd} disabled={formOpen}>Add contact</Button>
    </div>

    {#if loading}
      <p class="text-fg-subtle text-xs">Loading…</p>
    {:else if contacts.length === 0}
      <p class="text-fg-subtle text-xs">
        {query.trim() ? 'No contacts match.' : 'No contacts yet.'}
      </p>
    {:else}
      <ul class="divide-y divide-border-subtle border border-border-subtle rounded-lg">
        {#each contacts as c (c.id)}
          <li class="p-3 flex items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <p class="font-medium text-fg truncate">{c.name}</p>
              {#if c.note}
                <p class="text-fg-muted text-xs mt-0.5 truncate">{c.note}</p>
              {/if}
              <ul class="mt-1.5 space-y-0.5">
                {#each c.addresses as a}
                  <li class="font-mono text-xs text-fg-subtle truncate">
                    <span class="text-fg-muted mr-2">{a.chain}</span>{a.address}
                  </li>
                {/each}
              </ul>
            </div>
            <div class="flex flex-col gap-1 shrink-0">
              <Button variant="secondary" on:click={() => startEdit(c)}>Edit</Button>
              <Button variant="secondary" on:click={() => remove(c.id)}>Remove</Button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    {#if formOpen}
      <div class="border border-border-subtle rounded-lg p-4 space-y-3 bg-bg-elevated">
        <p class="font-medium">{editing === null ? 'New contact' : 'Edit contact'}</p>

        <Input label="Name" bind:value={name} placeholder="Alice" />
        <Input
          label="Note"
          bind:value={note}
          multiline
          rows={2}
          placeholder="Optional. Short description."
        />

        <div>
          <p class="text-fg-muted mb-2">Addresses</p>
          {#if addresses.length > 0}
            <ul class="space-y-1.5 mb-2">
              {#each addresses as a, i}
                <li class="flex items-center gap-2">
                  <span class="text-xs text-fg-muted w-20 shrink-0">{a.chain}</span>
                  <span class="font-mono text-xs text-fg truncate flex-1">{a.address}</span>
                  <button
                    type="button"
                    class="text-xs text-rose-400 hover:underline"
                    on:click={() => removeRow(i)}
                  >
                    remove
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="flex items-center gap-2">
            <select
              bind:value={newAddrChain}
              class="h-9 px-2 rounded-md bg-bg-subtle border border-border-subtle text-fg text-xs"
            >
              {#each CHAINS as ch}
                <option value={ch}>{ch}</option>
              {/each}
            </select>
            <input
              type="text"
              bind:value={newAddrValue}
              placeholder="0x… or bc1…"
              class="flex-1 h-9 px-2 rounded-md bg-bg-subtle border border-border-subtle text-fg text-xs font-mono"
            />
            <Button variant="secondary" on:click={addRow}>Add</Button>
          </div>
        </div>

        <div class="flex items-center gap-2 pt-1">
          <Button
            variant="primary"
            on:click={save}
            disabled={saving || name.trim() === '' || addresses.length === 0}
          >
            {saving ? 'Saving…' : 'Save'}
          </Button>
          <Button variant="secondary" on:click={resetForm} disabled={saving}>Cancel</Button>
        </div>
      </div>
    {/if}
  </div>
</Card>
