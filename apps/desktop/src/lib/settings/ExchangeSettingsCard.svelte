<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type ExchangeSettings } from '$lib/api';

  let cfg: ExchangeSettings | null = null;
  let apiKey = '';
  let baseUrl = '';
  let saving = false;
  let error: string | null = null;
  let savedAt: number | null = null;

  async function load() {
    try {
      cfg = await api.getExchangeSettings();
      // Never show the stored API key — only its set/unset state.
      apiKey = '';
      baseUrl = cfg.base_url ?? '';
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function save() {
    saving = true;
    error = null;
    try {
      const next = await api.setExchangeSettings({
        api_key: apiKey.trim() === '' ? null : apiKey,
        base_url: baseUrl.trim() === '' ? null : baseUrl
      });
      cfg = next;
      apiKey = '';
      baseUrl = next.base_url;
      savedAt = Date.now();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  async function clearKey() {
    apiKey = '';
    saving = true;
    error = null;
    try {
      cfg = await api.setExchangeSettings({ api_key: null, base_url: cfg?.base_url ?? null });
      savedAt = Date.now();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  onMount(load);
</script>

<Card title="Exchange (1inch)">
  <div class="space-y-4 text-sm">
    <p class="text-fg-muted">
      Atlas talks to the 1inch v6 aggregator for EVM swaps. Get a free API key at
      <span class="font-mono text-xs">portal.1inch.dev</span> — Atlas stores it locally, never on a server.
    </p>

    <div class="flex items-center justify-between">
      <span class="text-fg-muted">API key</span>
      <span class="text-xs">
        {#if cfg?.api_key_set}
          <span class="px-2 py-0.5 rounded-full bg-success/15 text-success">configured</span>
        {:else}
          <span class="px-2 py-0.5 rounded-full bg-warning/15 text-warning">not set</span>
        {/if}
      </span>
    </div>

    <Input
      label="New API key"
      type="password"
      bind:value={apiKey}
      placeholder={cfg?.api_key_set ? '•••••••• (leave blank to keep)' : 'paste your 1inch key'}
      hint="Bearer token. Stored only on this device."
    />

    <Input
      label="Base URL override"
      bind:value={baseUrl}
      placeholder="https://api.1inch.dev"
      hint="Leave blank for the default."
    />

    {#if error}
      <p class="text-danger text-xs">{error}</p>
    {/if}
    {#if savedAt}
      <p class="text-success text-xs">Saved.</p>
    {/if}

    <div class="flex gap-2">
      <Button on:click={save} disabled={saving}>
        {saving ? 'Saving…' : 'Save'}
      </Button>
      {#if cfg?.api_key_set}
        <Button variant="secondary" on:click={clearKey} disabled={saving}>Clear key</Button>
      {/if}
    </div>
  </div>
</Card>
