<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, type ProfileSummary } from '$lib/api';
  import { wallet } from '$lib/stores/wallet';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';

  let profiles: ProfileSummary[] = [];
  let active: ProfileSummary | null = null;
  let busy = '';
  let error = '';
  let editing: string | null = null;
  let editName = '';
  let confirmDelete: ProfileSummary | null = null;

  // Add-wallet modal state.
  let addOpen = false;
  let addPassword = '';
  let addPasswordConfirm = '';
  let addWordCount: 12 | 24 = 12;
  let addBusy = false;
  let addError = '';
  let newPhrase: string | null = null;

  async function refresh() {
    try {
      const [list, act] = await Promise.all([api.listProfiles(), api.activeProfile()]);
      profiles = list;
      active = act;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function switchTo(id: string) {
    if (busy) return;
    busy = id;
    error = '';
    try {
      await api.switchProfile(id);
      // Force a full reload so all stores re-fetch under the new profile.
      window.location.assign('/portfolio');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = '';
    }
  }

  function startRename(p: ProfileSummary) {
    editing = p.id;
    editName = p.name;
  }

  async function saveRename(p: ProfileSummary) {
    const trimmed = editName.trim();
    if (!trimmed || trimmed === p.name) {
      editing = null;
      return;
    }
    busy = p.id;
    error = '';
    try {
      await api.renameProfile(p.id, trimmed);
      await refresh();
      editing = null;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = '';
    }
  }

  async function doDelete(p: ProfileSummary) {
    busy = p.id;
    error = '';
    try {
      await api.deleteProfile(p.id);
      confirmDelete = null;
      await refresh();
      // If we deleted the active wallet, the backend cleared the in-memory
      // mnemonic too — bounce to unlock so the user can pick another.
      if (active?.id === p.id) {
        window.location.assign('/unlock');
      }
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = '';
    }
  }

  function openAdd() {
    addOpen = true;
    addPassword = '';
    addPasswordConfirm = '';
    addWordCount = 12;
    addError = '';
    newPhrase = null;
  }

  async function submitAdd() {
    addError = '';
    if (addPassword.length < 8) {
      addError = 'Password must be at least 8 characters.';
      return;
    }
    if (addPassword !== addPasswordConfirm) {
      addError = 'Passwords do not match.';
      return;
    }
    addBusy = true;
    try {
      const phrase = await wallet.createNew(addPassword, addWordCount);
      newPhrase = phrase;
      await refresh();
    } catch (e) {
      addError = errorMessage(e);
    } finally {
      addBusy = false;
    }
  }

  function closeAdd() {
    addOpen = false;
    newPhrase = null;
  }

  onMount(refresh);
</script>

<div class="p-8 max-w-3xl space-y-6">
  <header class="flex items-baseline justify-between">
    <div>
      <h1 class="text-2xl font-bold">Wallets</h1>
      <p class="text-fg-muted text-sm mt-1">
        Switch between your wallets, rename them, or add a new one.
      </p>
    </div>
    <Button on:click={openAdd}>+ Add wallet</Button>
  </header>

  {#if error}
    <Card><p class="text-sm text-danger">{error}</p></Card>
  {/if}

  <Card title="Your wallets">
    <div class="-mx-6 -mb-6">
      {#each profiles as p (p.id)}
        <div class="px-6 py-4 border-t border-border-subtle">
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div
                class="h-10 w-10 rounded-xl bg-brand-gradient flex items-center justify-center text-white font-bold flex-none"
              >
                {p.name.charAt(0).toUpperCase()}
              </div>
              <div class="min-w-0">
                {#if editing === p.id}
                  <div class="flex gap-2 items-center">
                    <input
                      bind:value={editName}
                      class="bg-bg-elevated border border-border rounded-lg px-2 py-1 text-sm w-48"
                    />
                    <Button size="sm" loading={busy === p.id} on:click={() => saveRename(p)}>
                      Save
                    </Button>
                    <Button size="sm" variant="ghost" on:click={() => (editing = null)}>
                      Cancel
                    </Button>
                  </div>
                {:else}
                  <div class="font-semibold flex items-center gap-2">
                    {p.name}
                    {#if active?.id === p.id}
                      <span class="text-[10px] uppercase tracking-wider text-success">Active</span>
                    {/if}
                  </div>
                  <div class="text-xs text-fg-muted">
                    {p.kind === 'hot' ? 'Hot wallet' : 'Watch-only'} · Created
                    {new Date(p.created_at).toLocaleDateString()}
                  </div>
                {/if}
              </div>
            </div>

            {#if editing !== p.id}
              <div class="flex items-center gap-1.5 flex-none">
                {#if active?.id !== p.id}
                  <Button
                    variant="secondary"
                    size="sm"
                    loading={busy === p.id}
                    on:click={() => switchTo(p.id)}
                  >
                    Switch
                  </Button>
                {/if}
                <Button variant="ghost" size="sm" on:click={() => startRename(p)}>
                  Rename
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  on:click={() => (confirmDelete = p)}
                >
                  Delete
                </Button>
              </div>
            {/if}
          </div>
        </div>
      {/each}
      {#if profiles.length === 0}
        <div class="px-6 py-8 text-sm text-fg-muted text-center">
          No wallets yet.
        </div>
      {/if}
    </div>
  </Card>
</div>

<!-- Delete confirmation -->
{#if confirmDelete}
  <div class="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
    <div class="bg-bg-subtle border border-border-subtle rounded-2xl shadow-card w-full max-w-md p-6 space-y-4">
      <h3 class="font-semibold text-lg">Delete this wallet?</h3>
      <p class="text-sm text-fg-muted">
        This permanently removes <strong class="text-fg">{confirmDelete.name}</strong>
        and its encrypted vault from disk.
        <strong class="text-warning">If you have not backed up the seed phrase,
        the funds will be lost forever.</strong>
      </p>
      <div class="flex justify-end gap-2 pt-2">
        <Button variant="ghost" on:click={() => (confirmDelete = null)}>Cancel</Button>
        <Button
          variant="danger"
          loading={busy === confirmDelete.id}
          on:click={() => doDelete(confirmDelete!)}
        >
          Yes, delete
        </Button>
      </div>
    </div>
  </div>
{/if}

<!-- Add-wallet modal -->
{#if addOpen}
  <div class="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
    <div class="bg-bg-subtle border border-border-subtle rounded-2xl shadow-card w-full max-w-md p-6 space-y-4">
      <div class="flex items-center justify-between">
        <h3 class="font-semibold text-lg">Add a new wallet</h3>
        <button class="text-fg-muted hover:text-fg text-xl leading-none px-2" on:click={closeAdd}>×</button>
      </div>

      {#if newPhrase}
        <div class="space-y-3">
          <p class="text-sm text-fg-muted">
            <strong class="text-warning">Write this phrase down now.</strong>
            It is the only way to recover this wallet. Atlas will never show
            it again.
          </p>
          <div class="grid grid-cols-3 gap-2 font-mono text-sm bg-bg-elevated rounded-xl p-4 border border-border">
            {#each newPhrase.split(/\s+/) as word, i}
              <div class="flex items-center gap-2">
                <span class="text-fg-subtle text-xs w-5 text-right">{i + 1}.</span>
                <span class="select-all">{word}</span>
              </div>
            {/each}
          </div>
          <Button fullWidth on:click={closeAdd}>I have saved it</Button>
        </div>
      {:else}
        <p class="text-sm text-fg-muted">
          Each wallet has its own seed phrase and vault password. The new
          wallet becomes active automatically.
        </p>

        <div class="space-y-3">
          <div>
            <span class="text-sm text-fg-muted block mb-2">Phrase length</span>
            <div class="grid grid-cols-2 gap-2">
              <button
                class="rounded-xl px-3 py-2 border text-sm transition {addWordCount === 12 ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (addWordCount = 12)}
              >12 words</button>
              <button
                class="rounded-xl px-3 py-2 border text-sm transition {addWordCount === 24 ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (addWordCount = 24)}
              >24 words</button>
            </div>
          </div>

          <Input label="New vault password" type="password" bind:value={addPassword} />
          <Input label="Confirm password" type="password" bind:value={addPasswordConfirm} />

          {#if addError}
            <p class="text-sm text-danger">{addError}</p>
          {/if}
        </div>

        <div class="flex justify-end gap-2 pt-2">
          <Button variant="ghost" on:click={closeAdd}>Cancel</Button>
          <Button loading={addBusy} on:click={submitAdd}>Generate</Button>
        </div>
      {/if}
    </div>
  </div>
{/if}
