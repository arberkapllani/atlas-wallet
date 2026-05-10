<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api } from '$lib/api';

  /** Presets in minutes; 0 = disabled. */
  const PRESETS = [
    { value: 1, label: '1 minute' },
    { value: 5, label: '5 minutes' },
    { value: 15, label: '15 minutes' },
    { value: 30, label: '30 minutes' },
    { value: 60, label: '1 hour' },
    { value: 0, label: 'Never (disabled)' }
  ];

  let current = 5;
  let pending = 5;
  let saving = false;
  let error = '';

  onMount(async () => {
    try {
      current = await api.getAutoLockMinutes();
      pending = current;
    } catch (e) {
      error = `${e}`;
    }
  });

  async function save() {
    saving = true;
    error = '';
    try {
      const persisted = await api.setAutoLockMinutes(pending);
      current = persisted;
      pending = persisted;
    } catch (e) {
      error = `${e}`;
    } finally {
      saving = false;
    }
  }

  $: dirty = pending !== current;
</script>

<Card title="Auto-lock">
  <div class="space-y-3 text-sm">
    <p class="text-fg-muted">
      Atlas locks the in-memory seed after this period of inactivity. The page reverts to the unlock
      screen and your password is required again.
    </p>

    <label class="block">
      <span class="sr-only">Auto-lock timeout</span>
      <select
        bind:value={pending}
        class="w-full h-10 px-3 rounded-lg bg-bg-elevated border border-border-subtle text-fg focus:outline-none focus:ring-2 focus:ring-accent/40"
      >
        {#each PRESETS as p}
          <option value={p.value}>{p.label}</option>
        {/each}
      </select>
    </label>

    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2">
      <Button variant="primary" disabled={!dirty || saving} on:click={save}>
        {saving ? 'Saving…' : 'Save'}
      </Button>
      <span class="text-xs text-fg-subtle">
        Currently: {current === 0 ? 'disabled' : `${current} minute${current === 1 ? '' : 's'}`}
      </span>
    </div>
  </div>
</Card>
