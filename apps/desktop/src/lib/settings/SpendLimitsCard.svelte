<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type SpendPolicy,
    type SpendState
  } from '$lib/api';

  let policy: SpendPolicy = {
    daily_usd_limit: 0,
    per_tx_usd_limit: 0,
    hard_block_on_daily: false
  };
  let state: SpendState = { window_started_at: 0, spent_in_window_usd: 0 };

  // Editable mirrors of the policy. Stored as strings so an empty input
  // is "disabled" (0) without forcing the user to type a literal zero.
  let dailyInput = '';
  let perTxInput = '';
  let hardBlock = false;

  let saving = false;
  let error = '';
  let savedAt: number | null = null;

  function parseUsd(s: string): number | null {
    const t = s.trim();
    if (t === '') return 0;
    if (!/^\d+$/.test(t)) return null;
    const n = parseInt(t, 10);
    return Number.isFinite(n) && n >= 0 ? n : null;
  }

  function fmtTs(unix: number): string {
    if (unix === 0) return 'never';
    const d = new Date(unix * 1000);
    return d.toLocaleString();
  }

  async function load() {
    error = '';
    try {
      policy = await api.spendGetPolicy();
      state = await api.spendGetState();
      dailyInput = policy.daily_usd_limit === 0 ? '' : String(policy.daily_usd_limit);
      perTxInput = policy.per_tx_usd_limit === 0 ? '' : String(policy.per_tx_usd_limit);
      hardBlock = policy.hard_block_on_daily;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function save() {
    saving = true;
    error = '';
    savedAt = null;
    try {
      const daily = parseUsd(dailyInput);
      const perTx = parseUsd(perTxInput);
      if (daily === null) {
        error = 'Daily limit must be a non-negative whole number of USD.';
        return;
      }
      if (perTx === null) {
        error = 'Per-transaction limit must be a non-negative whole number of USD.';
        return;
      }
      const next: SpendPolicy = {
        daily_usd_limit: daily,
        per_tx_usd_limit: perTx,
        hard_block_on_daily: hardBlock
      };
      await api.spendSetPolicy(next);
      policy = next;
      savedAt = Date.now();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  async function reset() {
    saving = true;
    error = '';
    savedAt = null;
    try {
      await api.spendReset();
      state = await api.spendGetState();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  onMount(load);

  $: dirty =
    parseUsd(dailyInput) !== policy.daily_usd_limit ||
    parseUsd(perTxInput) !== policy.per_tx_usd_limit ||
    hardBlock !== policy.hard_block_on_daily;
</script>

<Card title="Spend limits" subtitle="USD caps applied locally before any transaction is signed.">
  <div class="space-y-4 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <label class="block">
        <span class="text-fg-muted text-xs block mb-1">Daily limit (USD)</span>
        <input
          type="text"
          inputmode="numeric"
          bind:value={dailyInput}
          placeholder="0 = disabled"
          class="w-full h-10 px-3 rounded-lg bg-bg-elevated border border-border-subtle text-fg font-mono focus:outline-none focus:ring-2 focus:ring-accent/40"
        />
      </label>
      <label class="block">
        <span class="text-fg-muted text-xs block mb-1">Per-transaction limit (USD)</span>
        <input
          type="text"
          inputmode="numeric"
          bind:value={perTxInput}
          placeholder="0 = disabled"
          class="w-full h-10 px-3 rounded-lg bg-bg-elevated border border-border-subtle text-fg font-mono focus:outline-none focus:ring-2 focus:ring-accent/40"
        />
      </label>
    </div>

    <label class="flex items-start gap-2">
      <input type="checkbox" bind:checked={hardBlock} class="mt-1" />
      <span class="text-fg-muted text-xs">
        Hard-block daily cap. When unchecked, exceeding the daily limit only
        requires an extra confirmation; per-transaction limit is always a hard block.
      </span>
    </label>

    <div class="flex items-center gap-2">
      <Button variant="primary" on:click={save} disabled={saving || !dirty}>
        {saving ? 'Saving…' : 'Save policy'}
      </Button>
      {#if savedAt}
        <span class="text-fg-subtle text-xs">Saved.</span>
      {/if}
    </div>

    <div class="rounded-lg border border-border-subtle p-3 space-y-1">
      <p class="font-medium text-xs text-fg-muted">Current 24-hour window</p>
      <p class="font-mono text-xs">
        spent: {state.spent_in_window_usd} USD &middot; window opened: {fmtTs(state.window_started_at)}
      </p>
      <Button variant="secondary" on:click={reset} disabled={saving}>
        Reset window
      </Button>
    </div>
  </div>
</Card>
