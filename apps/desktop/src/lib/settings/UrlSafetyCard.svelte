<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type PhishingReport,
    type PhishingVerdict
  } from '$lib/api';

  let url = '';
  let report: PhishingReport | null = null;
  let error = '';
  let busy = false;

  async function check() {
    if (!url.trim()) return;
    busy = true;
    error = '';
    report = null;
    try {
      report = await api.phishingAnalyzeDefault(url.trim());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function clear() {
    url = '';
    report = null;
    error = '';
  }

  function tone(v: PhishingVerdict): string {
    switch (v) {
      case 'Safe':
        return 'text-emerald-400';
      case 'Suspicious':
        return 'text-amber-400';
      case 'Phish':
        return 'text-rose-400';
    }
  }

  function borderTone(v: PhishingVerdict): string {
    switch (v) {
      case 'Safe':
        return 'border-emerald-500/40 bg-emerald-500/5';
      case 'Suspicious':
        return 'border-amber-500/40 bg-amber-500/5';
      case 'Phish':
        return 'border-rose-500/40 bg-rose-500/5';
    }
  }
</script>

<Card
  title="URL safety check"
  subtitle="Scan a dApp / WalletConnect origin for typosquats, homoglyphs and Punycode tricks before connecting."
>
  <div class="space-y-3 text-sm">
    <div class="flex items-center gap-2">
      <input
        type="text"
        bind:value={url}
        placeholder="https://app.uniswap.org"
        class="flex-1 h-9 px-3 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-xs"
        on:keydown={(e) => {
          if (e.key === 'Enter') check();
        }}
      />
      <Button variant="primary" on:click={check} disabled={busy || !url.trim()}>Check</Button>
      {#if report || error}
        <Button variant="secondary" on:click={clear} disabled={busy}>Clear</Button>
      {/if}
    </div>

    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    {#if report}
      <div class="rounded-lg border {borderTone(report.verdict)} p-3 space-y-2">
        <div class="flex items-baseline justify-between gap-2">
          <p class="font-mono text-xs break-all">{report.host}</p>
          <p class="text-xs font-medium {tone(report.verdict)}">{report.verdict}</p>
        </div>
        {#if report.reasons.length > 0}
          <ul class="text-xs text-fg-muted list-disc list-inside space-y-0.5">
            {#each report.reasons as r}
              <li>{r}</li>
            {/each}
          </ul>
        {:else}
          <p class="text-xs text-fg-subtle">No red flags detected.</p>
        {/if}
        {#if report.closest_target && report.closest_distance !== null}
          <p class="text-[10px] text-fg-subtle">
            Closest brand: <span class="font-mono">{report.closest_target}</span>
            (edit distance {report.closest_distance})
          </p>
        {/if}
      </div>
    {/if}

    <p class="text-[10px] text-fg-subtle">
      Built-in target list covers major DEX, NFT, and wallet brands. Curated good origins are also
      checked separately by the dApp registry.
    </p>
  </div>
</Card>
