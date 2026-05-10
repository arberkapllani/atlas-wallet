<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type SwapFeeConfig } from '$lib/api';

  let cfg: SwapFeeConfig | null = null;
  let feePct = '0.25'; // displayed in percent for humans
  let affiliate = '';
  let flashbots = false;

  let saving = false;
  let error: string | null = null;
  let savedAt: number | null = null;

  function bpsToPct(bps: number): string {
    return (bps / 100).toFixed(2);
  }

  function pctToBps(pct: string): number | null {
    const trimmed = pct.trim();
    if (trimmed === '' || !/^\d*(?:\.\d+)?$/.test(trimmed)) return null;
    const n = Math.round(parseFloat(trimmed) * 100);
    if (!Number.isFinite(n) || n < 0) return null;
    return Math.min(n, 100); // backend caps at 100 bps = 1 %
  }

  async function load() {
    try {
      cfg = await api.getSwapFeeConfig();
      feePct = bpsToPct(cfg.fee_bps);
      affiliate = cfg.thorchain_affiliate ?? '';
      flashbots = await api.flashbotsProtectEnabled();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function save() {
    saving = true;
    error = null;
    savedAt = null;
    try {
      const bps = pctToBps(feePct);
      if (bps === null) {
        error = 'Fee must be a non-negative percentage (max 1 %).';
        return;
      }
      const newBps = await api.setSwapFeeBps(bps);
      const newAff = await api.setThorchainAffiliate(
        affiliate.trim() === '' ? null : affiliate.trim()
      );
      cfg = { fee_bps: newBps, thorchain_affiliate: newAff };
      feePct = bpsToPct(newBps);
      affiliate = newAff ?? '';
      savedAt = Date.now();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  async function toggleFlashbots() {
    saving = true;
    error = null;
    savedAt = null;
    try {
      flashbots = await api.setFlashbotsProtect(!flashbots);
      savedAt = Date.now();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  onMount(load);
</script>

<Card title="Swap fee &amp; MEV protection">
  <div class="space-y-5 text-sm">
    <p class="text-fg-muted">
      Atlas takes a small fee on every swap to fund development. Tune it here, set a THORChain
      affiliate name for cross-chain swaps, and toggle Flashbots Protect to route Ethereum-mainnet
      transactions through a private mempool.
    </p>

    <Input
      label="Atlas swap fee"
      bind:value={feePct}
      placeholder="0.25"
      hint="Percent (0&ndash;1&nbsp;%). Default 0.25&nbsp;%. Applies to Jupiter; cross-chain uses THORChain's affiliate-bps mirror."
    />

    <Input
      label="THORChain affiliate (THORName)"
      bind:value={affiliate}
      placeholder="atlas"
      hint="Optional. Earn affiliate fees on THORChain swaps when set."
    />

    <div class="flex gap-2">
      <Button on:click={save} disabled={saving}>
        {saving ? 'Saving…' : 'Save'}
      </Button>
    </div>

    <div class="border-t border-border-subtle pt-4 space-y-3">
      <div class="flex items-start justify-between gap-4">
        <div class="flex-1">
          <div class="font-medium">Flashbots Protect</div>
          <p class="text-xs text-fg-muted mt-1">
            Routes Ethereum-mainnet transactions through
            <span class="font-mono">rpc.flashbots.net</span> instead of the public mempool, blocking sandwich
            and front-run attacks on swaps and large sends.
          </p>
        </div>
        <button
          type="button"
          on:click={toggleFlashbots}
          disabled={saving}
          class="shrink-0 px-3 py-1.5 rounded-lg text-xs font-medium border transition
                 {flashbots
            ? 'bg-success/15 border-success/40 text-success'
            : 'bg-bg-elevated border-border text-fg-muted hover:border-accent'}"
          aria-pressed={flashbots}
        >
          {flashbots ? 'Enabled' : 'Disabled'}
        </button>
      </div>
    </div>

    {#if error}
      <p class="text-danger text-xs">{error}</p>
    {/if}
    {#if savedAt}
      <p class="text-success text-xs">Saved.</p>
    {/if}
  </div>
</Card>
