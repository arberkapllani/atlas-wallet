<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type Eip712Report,
    type Eip712Category,
    type SignatureRisk
  } from '$lib/api';

  let json = '';
  let report: Eip712Report | null = null;
  let error = '';
  let busy = false;

  async function classify() {
    if (!json.trim()) return;
    busy = true;
    error = '';
    report = null;
    try {
      report = await api.eip712Classify(json);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function clear() {
    json = '';
    report = null;
    error = '';
  }

  function categoryLabel(c: Eip712Category): string {
    switch (c) {
      case 'Permit':
        return 'ERC-2612 Permit';
      case 'Permit2':
        return 'Uniswap Permit2';
      case 'SeaportOrder':
        return 'OpenSea Seaport order';
      case 'SafeTx':
        return 'Gnosis Safe transaction';
      case 'Generic':
        return 'Generic typed-data';
    }
  }

  function tone(r: SignatureRisk): string {
    switch (r) {
      case 'Critical':
        return 'text-rose-400';
      case 'High':
        return 'text-amber-400';
      case 'Medium':
        return 'text-yellow-400';
      case 'Low':
        return 'text-emerald-400';
    }
  }

  function borderTone(r: SignatureRisk): string {
    switch (r) {
      case 'Critical':
        return 'border-rose-500/40 bg-rose-500/5';
      case 'High':
        return 'border-amber-500/40 bg-amber-500/5';
      case 'Medium':
        return 'border-yellow-500/40 bg-yellow-500/5';
      case 'Low':
        return 'border-emerald-500/40 bg-emerald-500/5';
    }
  }
</script>

<Card
  title="Typed-data inspector"
  subtitle="Paste an EIP-712 typed-data JSON payload (eth_signTypedData) to classify the request and surface signature risk before approving."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <textarea
      bind:value={json}
      rows="6"
      placeholder={'{"types": …, "domain": …, "primaryType": …, "message": …}'}
      class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
    ></textarea>

    <div class="flex items-center gap-2">
      <Button variant="primary" on:click={classify} disabled={busy || !json.trim()}>
        Classify
      </Button>
      {#if report || json}
        <Button variant="secondary" on:click={clear} disabled={busy}>Clear</Button>
      {/if}
    </div>

    {#if report}
      <div class="rounded-lg border {borderTone(report.risk)} p-3 space-y-2">
        <div class="flex items-baseline justify-between gap-2">
          <p class="text-xs font-medium">{categoryLabel(report.category)}</p>
          <p class="text-[10px] font-medium {tone(report.risk)}">{report.risk}</p>
        </div>
        <p class="text-[11px] text-fg-muted">
          primaryType: <span class="font-mono">{report.primary_type}</span>
        </p>
        <div class="text-[11px] text-fg-muted space-y-0.5">
          {#if report.domain.name}
            <p>name: <span class="font-mono">{report.domain.name}</span></p>
          {/if}
          {#if report.domain.version}
            <p>version: <span class="font-mono">{report.domain.version}</span></p>
          {/if}
          {#if report.domain.chain_id !== null && report.domain.chain_id !== undefined}
            <p>chainId: <span class="font-mono">{report.domain.chain_id}</span></p>
          {/if}
          {#if report.domain.verifying_contract}
            <p>
              verifyingContract:
              <span class="font-mono break-all">{report.domain.verifying_contract}</span>
            </p>
          {/if}
        </div>
        {#if report.findings.length > 0}
          <ul class="text-[11px] list-disc list-inside text-fg-muted space-y-0.5">
            {#each report.findings as f}
              <li>{f}</li>
            {/each}
          </ul>
        {:else}
          <p class="text-[11px] text-fg-subtle">No specific findings — generic signature.</p>
        {/if}
      </div>
    {/if}
  </div>
</Card>
