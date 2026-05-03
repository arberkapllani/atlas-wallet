<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, type ProfileSummary } from '$lib/api';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';

  let profiles: ProfileSummary[] = [];
  let active: ProfileSummary | null = null;
  let busy = '';
  let error = '';

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
      await refresh();
      // Force a full reload so all stores re-fetch under the new profile.
      window.location.assign('/portfolio');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = '';
    }
  }

  onMount(refresh);
</script>

<div class="p-8 max-w-3xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Wallets</h1>
    <p class="text-fg-muted text-sm mt-1">
      Switch between your wallets. Each wallet has its own seed phrase and addresses.
    </p>
  </header>

  {#if error}
    <Card><p class="text-sm text-danger">{error}</p></Card>
  {/if}

  <Card title="Your wallets">
    <div class="-mx-6 -mb-6">
      {#each profiles as p (p.id)}
        <div
          class="flex items-center justify-between px-6 py-4 border-t border-border-subtle"
        >
          <div class="flex items-center gap-3">
            <div
              class="h-10 w-10 rounded-xl bg-brand-gradient flex items-center justify-center text-white font-bold"
            >
              {p.name.charAt(0).toUpperCase()}
            </div>
            <div>
              <div class="font-semibold flex items-center gap-2">
                {p.name}
                {#if active?.id === p.id}
                  <span class="text-[10px] uppercase tracking-wider text-success">Active</span>
                {/if}
              </div>
              <div class="text-xs text-fg-muted">
                {p.kind === 'hot' ? 'Hot wallet' : 'Watch-only'} · Created{' '}
                {new Date(p.created_at).toLocaleDateString()}
              </div>
            </div>
          </div>
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
        </div>
      {/each}
      {#if profiles.length === 0}
        <div class="px-6 py-8 text-sm text-fg-muted text-center">
          No wallets yet.
        </div>
      {/if}
    </div>
  </Card>

  <Card>
    <p class="text-sm text-fg-muted">
      Adding a new wallet creates a fresh seed phrase. To add one, lock Atlas
      and choose <strong class="text-fg">Generate phrase</strong> on the unlock
      screen — your existing wallet stays intact.
    </p>
  </Card>
</div>
