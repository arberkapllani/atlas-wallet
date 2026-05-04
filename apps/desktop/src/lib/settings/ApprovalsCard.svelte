<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type Approval,
    type ApprovalConfig,
    type ApprovalRisk,
    type ApprovalSummary,
    type RiskLevel
  } from '$lib/api';

  let pasted = '';
  let rows: ApprovalRisk[] = [];
  let summaries: ApprovalSummary[] = [];
  let error = '';
  let busy = false;

  // Default config: defer to crate defaults via empty allow/block.
  const defaultConfig: ApprovalConfig = {
    blocklist: [],
    allowlist: [],
    unlimited_threshold:
      '1606938044258990275541962092341162602522202993782792835301376',
    stale_after_secs: 60 * 60 * 24 * 180
  };

  async function analyse(approvals: Approval[]) {
    busy = true;
    error = '';
    try {
      const now = Math.floor(Date.now() / 1000);
      rows = await api.approvalsAnalyze(approvals, defaultConfig, now);
      summaries = await api.approvalsSummarise(rows);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function analysePasted() {
    if (!pasted.trim()) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(pasted);
    } catch (e) {
      error = `Invalid JSON: ${(e as Error).message}`;
      return;
    }
    if (!Array.isArray(parsed)) {
      error = 'Expected a JSON array of Approval objects.';
      return;
    }
    await analyse(parsed as Approval[]);
  }

  async function loadDemo() {
    const now = Math.floor(Date.now() / 1000);
    const demo: Approval[] = [
      {
        chain_id: 'ethereum',
        token_address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
        token_symbol: 'USDC',
        token_decimals: 6,
        spender_address: '0x0000000000000000000000000000000000000bad',
        spender_label: null,
        allowance:
          '115792089237316195423570985008687907853269984665640564039457584007913129639935',
        kind: 'Erc20',
        first_seen: now - 86_400 * 30,
        last_used: null
      },
      {
        chain_id: 'ethereum',
        token_address: '0xdac17f958d2ee523a2206206994597c13d831ec7',
        token_symbol: 'USDT',
        token_decimals: 6,
        spender_address: '0x1111111254eeb25477b68fb85ed929f73a960582',
        spender_label: '1inch v5 Router',
        allowance: '500000000',
        kind: 'Erc20',
        first_seen: now - 86_400 * 5,
        last_used: now - 86_400 * 2
      },
      {
        chain_id: 'ethereum',
        token_address: '0x6b175474e89094c44da98b954eedeac495271d0f',
        token_symbol: 'DAI',
        token_decimals: 18,
        spender_address: '0xe592427a0aece92de3edee1f18e0157c05861564',
        spender_label: 'Uniswap V3 Router',
        allowance:
          '115792089237316195423570985008687907853269984665640564039457584007913129639935',
        kind: 'Erc20',
        first_seen: now - 86_400 * 400,
        last_used: now - 86_400 * 300
      },
      {
        chain_id: 'ethereum',
        token_address: '0xb01....bayc',
        token_symbol: 'BAYC',
        token_decimals: 0,
        spender_address: '0x00000000006c3852cbef3e08e8df289169ede581',
        spender_label: 'Seaport 1.5',
        allowance: 'unlimited',
        kind: 'ForAll',
        first_seen: now - 86_400 * 14,
        last_used: now - 86_400 * 1
      }
    ];
    await analyse(demo);
  }

  function clear() {
    pasted = '';
    rows = [];
    summaries = [];
    error = '';
  }

  function tone(l: RiskLevel): string {
    switch (l) {
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
</script>

<Card
  title="Token approval risks"
  subtitle="Score outstanding ERC-20 / ERC-721 / ERC-1155 allowances and prioritise revocations. On-chain fetching lands in a later phase — for now load demo data or paste an Approval[] JSON snapshot."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2 flex-wrap">
      <Button variant="primary" on:click={loadDemo} disabled={busy}>Load demo data</Button>
      <Button
        variant="secondary"
        on:click={analysePasted}
        disabled={busy || !pasted.trim()}
      >
        Analyse pasted JSON
      </Button>
      {#if rows.length > 0 || pasted}
        <Button variant="secondary" on:click={clear} disabled={busy}>Clear</Button>
      {/if}
    </div>

    <details class="text-xs">
      <summary class="cursor-pointer text-fg-muted">Paste Approval[] JSON</summary>
      <textarea
        bind:value={pasted}
        rows="4"
        placeholder={'[{"chain_id": "ethereum", "token_address": "0x…", …}]'}
        class="mt-2 w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
      ></textarea>
    </details>

    {#if summaries.length > 0}
      <div class="grid grid-cols-2 gap-2">
        {#each summaries as s}
          <div class="rounded-lg border border-border-subtle p-2">
            <p class="text-xs font-medium">{s.chain_id}</p>
            <p class="text-[10px] text-fg-muted mt-0.5">
              {s.total} approvals &middot; {s.unique_spenders} spenders
            </p>
            <p class="text-[10px] mt-0.5">
              <span class="text-rose-400">{s.critical} critical</span> &middot;
              <span class="text-amber-400">{s.high} high</span> &middot;
              <span class="text-yellow-400">{s.medium} med</span> &middot;
              <span class="text-emerald-400">{s.low} low</span>
            </p>
          </div>
        {/each}
      </div>
    {/if}

    {#if rows.length > 0}
      <ul class="border border-border-subtle rounded-lg divide-y divide-border-subtle max-h-80 overflow-y-auto">
        {#each rows as r}
          <li class="px-3 py-2 space-y-1">
            <div class="flex items-baseline justify-between gap-2">
              <p class="text-xs font-medium">
                {r.approval.token_symbol}
                <span class="text-fg-subtle text-[10px] ml-1">{r.approval.chain_id}</span>
              </p>
              <p class="text-[10px] font-medium {tone(r.level)}">{r.level}</p>
            </div>
            <p class="font-mono text-[10px] text-fg-muted break-all">
              spender: {r.approval.spender_label ?? r.approval.spender_address}
            </p>
            <p class="text-[10px] text-fg-subtle">
              allowance: <span class="font-mono">{r.approval.allowance}</span> &middot; {r.approval
                .kind}
            </p>
            {#if r.reasons.length > 0}
              <ul class="text-[10px] text-fg-muted list-disc list-inside">
                {#each r.reasons as why}
                  <li>{why}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-fg-subtle text-xs">No approvals analysed yet.</p>
    {/if}
  </div>
</Card>
