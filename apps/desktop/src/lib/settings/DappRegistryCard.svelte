<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type DappAssessment, type DappEntry } from '$lib/api';

  let url = '';
  let result: DappAssessment | null = null;
  let curated: DappEntry[] = [];
  let error = '';
  let busy = false;

  async function assess() {
    if (!url.trim()) return;
    busy = true;
    error = '';
    result = null;
    try {
      result = await api.dappAssessOrigin(url.trim());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function loadCurated() {
    try {
      curated = await api.dappListCurated();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function riskTone(r: string): string {
    if (r === 'verified') return 'text-emerald-400';
    if (r === 'blocked') return 'text-rose-400';
    return 'text-amber-400';
  }

  onMount(loadCurated);
</script>

<Card
  title="dApp origin assessor"
  subtitle="Strict origin (scheme + host + port) check against the curated dApp registry. HTTPS-only except for localhost. Pure offline lookup, no network."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2">
      <input
        type="text"
        bind:value={url}
        placeholder="https://app.uniswap.org"
        class="flex-1 h-9 px-3 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-xs"
      />
      <Button variant="primary" on:click={assess} disabled={busy || !url.trim()}>
        {busy ? 'Checking…' : 'Assess'}
      </Button>
    </div>

    {#if result}
      <div class="rounded-lg border border-border-subtle p-3 space-y-1.5 text-xs">
        <p>
          <span class="text-fg-muted">Origin:</span>
          <span class="font-mono break-all">{result.origin}</span>
        </p>
        <p>
          <span class="text-fg-muted">Risk:</span>
          <span class="font-medium uppercase {riskTone(result.risk)}">{result.risk}</span>
        </p>
        {#if result.entry}
          <p>
            <span class="text-fg-muted">Curated:</span>
            <span class="font-medium">{result.entry.name}</span>
            <span class="text-fg-subtle">({result.entry.category})</span>
          </p>
          {#if result.entry.tags.length > 0}
            <p class="text-fg-subtle">tags: {result.entry.tags.join(', ')}</p>
          {/if}
        {/if}
        {#if result.warnings.length > 0}
          <ul class="list-disc list-inside text-amber-400">
            {#each result.warnings as w}
              <li>{w}</li>
            {/each}
          </ul>
        {/if}
        <p class="text-[10px] text-fg-subtle font-mono">fingerprint: {result.fingerprint}</p>
      </div>
    {/if}

    {#if curated.length > 0}
      <details class="text-xs">
        <summary class="cursor-pointer text-fg-muted">
          Curated registry ({curated.length} entries)
        </summary>
        <ul class="mt-2 border border-border-subtle rounded-md divide-y divide-border-subtle max-h-48 overflow-y-auto">
          {#each curated as e}
            <li class="px-3 py-1.5 flex items-center gap-2">
              <span class="font-medium">{e.name}</span>
              <span class="text-fg-subtle">[{e.category}]</span>
              <span class="font-mono text-[10px] text-fg-muted ml-auto break-all">{e.origin}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>
</Card>
