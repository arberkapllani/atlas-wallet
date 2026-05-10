<script lang="ts">
  import { onMount } from 'svelte';
  import { wallet } from '$lib/stores/wallet';
  import {
    api,
    parseAmountToBase,
    formatAmount,
    errorMessage,
    type FeeOption,
    type TokenSummary,
    type ChainSummary
  } from '$lib/api';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import QrScanner from '$lib/ui/QrScanner.svelte';
  import { parsePaymentUri } from '$lib/paymentUri';
  import { prices } from '$lib/stores/prices';
  import { COINGECKO_IDS } from '$lib/stores/prices';

  /** A unified entry the user can choose to send: a chain's native asset
   * or one of its tokens. */
  interface SendableAsset {
    key: string; // unique form key, e.g. "native:eth" or "token:usdt-erc20"
    kind: 'native' | 'token';
    chainId: string;
    chainName: string;
    symbol: string;
    label: string;
    decimals: number;
    /** Set only for token entries. */
    tokenId?: string;
  }

  let tokens: TokenSummary[] = [];
  let selectedKey = '';
  let to = '';
  let amount = '';
  let feeOptions: FeeOption[] = [];
  let selectedLevel = 'normal';
  let busy = false;
  let error = '';
  let result: { txid: string } | null = null;
  let scanOpen = false;

  // ENS hint state. Atlas does not yet resolve ENS to an address (no
  // mainnet RPC wired into the desktop app), but we surface the
  // normalised form + namehash so the user can sanity-check that what
  // they typed matches what they intended.
  let ensHint:
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'ens'; normalised: string; namehash: string }
    | { kind: 'error'; message: string } = { kind: 'idle' };
  let ensSeq = 0;

  // Blocklist verdict for the current recipient. Refreshed reactively
  // alongside the ENS hint so a single edit drives both lookups.
  let blocklistHit: import('$lib/api').BlocklistEntry | null = null;
  let blocklistSeq = 0;

  // Address-poisoning candidate check: compares `to` against every
  // address in the user's contact book. If a close-but-not-exact
  // match exists, the recipient is likely a look-alike of a known
  // friend; we surface a banner and disable the send button.
  let mimicMatches: import('$lib/api').MimicMatch[] = [];
  let mimicSeq = 0;

  async function refreshBlocklist(value: string) {
    const my = ++blocklistSeq;
    const trimmed = value.trim();
    if (trimmed === '') {
      blocklistHit = null;
      return;
    }
    try {
      const verdict = await api.blocklistCheck(trimmed);
      if (my !== blocklistSeq) return;
      blocklistHit = verdict.kind === 'Listed' ? verdict.data : null;
    } catch {
      if (my !== blocklistSeq) return;
      blocklistHit = null;
    }
  }
  $: void refreshBlocklist(to);

  async function refreshMimic(value: string) {
    const my = ++mimicSeq;
    const trimmed = value.trim();
    if (trimmed === '') {
      mimicMatches = [];
      return;
    }
    try {
      const matches = await api.poisoningCheckCandidate(trimmed);
      if (my !== mimicSeq) return;
      mimicMatches = matches;
    } catch {
      if (my !== mimicSeq) return;
      mimicMatches = [];
    }
  }
  $: void refreshMimic(to);

  async function refreshEnsHint(value: string) {
    const my = ++ensSeq;
    const trimmed = value.trim();
    if (trimmed === '') {
      ensHint = { kind: 'idle' };
      return;
    }
    try {
      const looks = await api.ensLooksLikeEns(trimmed);
      if (my !== ensSeq) return;
      if (!looks) {
        ensHint = { kind: 'idle' };
        return;
      }
      ensHint = { kind: 'checking' };
      const normalised = await api.ensNormalise(trimmed);
      const namehash = await api.ensNamehash(normalised);
      if (my !== ensSeq) return;
      ensHint = { kind: 'ens', normalised, namehash };
    } catch (e) {
      if (my !== ensSeq) return;
      ensHint = { kind: 'error', message: errorMessage(e) };
    }
  }

  $: void refreshEnsHint(to);

  /** Map URI scheme to a chain id we know about. */
  const SCHEME_TO_CHAIN: Record<string, string> = {
    bitcoin: 'btc',
    ethereum: 'eth',
    tron: 'trx',
    solana: 'sol'
  };

  function onScan(e: CustomEvent<string>) {
    scanOpen = false;
    const parsed = parsePaymentUri(e.detail);
    to = parsed.address;
    if (parsed.amount) amount = parsed.amount;
    // Auto-pick a matching native asset if the URI scheme tells us which chain.
    if (parsed.scheme) {
      const chainId = SCHEME_TO_CHAIN[parsed.scheme];
      if (chainId) {
        const match = assets.find((a) => a.kind === 'native' && a.chainId === chainId);
        if (match) selectedKey = match.key;
      }
    }
  }

  onMount(async () => {
    try {
      tokens = await api.listTokens();
    } catch (e) {
      console.warn('token list failed', e);
    }
  });

  $: assets = buildAssets($wallet.chains, tokens);
  $: selected = assets.find((a) => a.key === selectedKey);

  function buildAssets(chains: ChainSummary[], allTokens: TokenSummary[]): SendableAsset[] {
    const out: SendableAsset[] = [];
    for (const c of chains) {
      out.push({
        key: `native:${c.id}`,
        kind: 'native',
        chainId: c.id,
        chainName: c.display_name,
        symbol: c.symbol,
        label: `${c.symbol} — ${c.display_name}`,
        decimals: c.decimals
      });
      for (const t of allTokens) {
        if (t.chain_id !== c.id) continue;
        if (!t.enabled_by_default) continue;
        out.push({
          key: `token:${t.id}`,
          kind: 'token',
          chainId: c.id,
          chainName: c.display_name,
          symbol: t.symbol,
          label: `${t.symbol} — ${t.display_name}`,
          decimals: t.decimals,
          tokenId: t.id
        });
      }
    }
    return out;
  }

  async function loadFees(chainId: string) {
    feeOptions = [];
    try {
      feeOptions = await api.getFeeOptions(chainId);
    } catch (e) {
      console.warn('fee fetch failed', e);
    }
  }

  $: if (selected) void loadFees(selected.chainId);

  async function send() {
    if (!selected) return;
    error = '';
    result = null;
    let baseAmount: string;
    try {
      baseAmount = parseAmountToBase(amount, selected.decimals);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    // Spend-limit pre-flight. The policy is denominated in USD;
    // we use the cached CoinGecko price for the asset's chain to
    // estimate this tx's USD value. If we can't estimate (no price
    // available) we conservatively pass `0` so per-tx caps still
    // gate on `Disabled` policies (`evaluate` returns Allowed when
    // both caps are 0). Daily caps without a price estimate would
    // under-count, which is acceptable: the user can still see
    // the policy in Settings and the cap kicks in once the price
    // feed is online.
    const cgId = COINGECKO_IDS[selected.chainId];
    const priceUsd = cgId ? ($prices[cgId]?.price ?? 0) : 0;
    const amountFloat = Number.parseFloat(amount);
    const attemptUsd =
      Number.isFinite(amountFloat) && priceUsd > 0 ? Math.round(amountFloat * priceUsd) : 0;
    let evaluation: import('$lib/api').LimitEvaluation | null = null;
    try {
      evaluation = await api.spendEvaluate(Math.floor(Date.now() / 1000), attemptUsd);
    } catch (e) {
      console.warn('spend-limit evaluation failed', e);
    }
    if (evaluation) {
      if (evaluation.decision === 'Blocked') {
        error = `Spend limit blocked this transaction (${evaluation.reason}). Adjust the policy in Settings → Spend limits.`;
        return;
      }
      if (evaluation.decision === 'RequiresConfirmation') {
        const ok = window.confirm(
          `Spend limit warning: ${evaluation.reason}. Continue with this transaction?`
        );
        if (!ok) return;
      }
    }
    busy = true;
    try {
      let r;
      if (selected.kind === 'native') {
        r = await api.sendNative({
          chain_id: selected.chainId,
          to,
          amount: baseAmount,
          fee_level: selectedLevel
        });
      } else {
        r = await api.sendToken({
          token_id: selected.tokenId!,
          to,
          amount: baseAmount,
          fee_level: selectedLevel
        });
      }
      result = { txid: r.txid };
      // Persist the spend-limit window after a successful broadcast.
      // Failures here are non-fatal: the tx already went out and
      // the next evaluation will catch up at the next window roll.
      if (evaluation) {
        try {
          await api.spendCommit(evaluation.next_state);
        } catch (e) {
          console.warn('spend-limit commit failed', e);
        }
      }
      void wallet.refreshBalances();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="p-8 max-w-2xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Send</h1>
    <p class="text-fg-muted text-sm mt-1">
      Transfer the native asset or a token (USDT, USDC, …) on any supported chain.
    </p>
  </header>

  <Card>
    <div class="space-y-4">
      <div>
        <span class="text-sm text-fg-muted block mb-2">Asset</span>
        <select
          bind:value={selectedKey}
          class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5 text-sm focus:outline-none focus:border-accent"
        >
          <option value="" disabled>Select an asset</option>
          {#each assets as a (a.key)}
            <option value={a.key}>{a.label}</option>
          {/each}
        </select>
        {#if selected}
          <p class="text-xs text-fg-subtle mt-1.5">
            {selected.kind === 'native' ? 'Native asset' : 'Token'} on {selected.chainName}
          </p>
        {/if}
      </div>

      <div>
        <div class="flex items-end justify-between mb-1">
          <span class="text-sm text-fg-muted">Recipient address</span>
          <button
            type="button"
            on:click={() => (scanOpen = true)}
            class="text-xs text-accent hover:text-accent-hover flex items-center gap-1"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              class="h-3.5 w-3.5"
              aria-hidden="true"
            >
              <path
                d="M3 7V5a2 2 0 012-2h2M17 3h2a2 2 0 012 2v2M21 17v2a2 2 0 01-2 2h-2M7 21H5a2 2 0 01-2-2v-2"
              />
              <path d="M7 7h4v4H7zM13 7h4v4h-4zM7 13h4v4H7zM13 13h2M17 13v4M13 17h4" />
            </svg>
            Scan QR
          </button>
        </div>
        <Input bind:value={to} placeholder="bc1q… / 0x… / vitalik.eth" />
        {#if blocklistHit}
          <div
            class="mt-2 rounded-lg border border-rose-500/40 bg-rose-500/10 p-3 text-xs space-y-0.5"
          >
            <p class="text-rose-300 font-semibold">
              Blocked: {blocklistHit.category}
              <span class="text-rose-300/80 font-normal">(source: {blocklistHit.source})</span>
            </p>
            {#if blocklistHit.note}
              <p class="text-rose-200/90">{blocklistHit.note}</p>
            {/if}
            <p class="text-rose-200/80">
              This address is on your local blocklist. GreenWallet will not let you send to it.
            </p>
          </div>
        {/if}
        {#if mimicMatches.length > 0}
          <div
            class="mt-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-xs space-y-1"
          >
            <p class="text-amber-300 font-semibold">Possible address-poisoning attempt</p>
            <p class="text-amber-200/90">
              This recipient closely resembles {mimicMatches.length === 1
                ? 'a'
                : `${mimicMatches.length}`} contact{mimicMatches.length === 1 ? '' : 's'} in your address
              book but is not an exact match. Verify carefully before sending.
            </p>
            {#each mimicMatches.slice(0, 3) as m}
              <p class="font-mono text-amber-200/80 break-all">
                resembles 0x{m.trusted_address}
                <span class="text-amber-200/60"
                  >(prefix {m.matching_prefix} / suffix {m.matching_suffix})</span
                >
              </p>
            {/each}
          </div>
        {/if}
        {#if ensHint.kind === 'checking'}
          <p class="text-xs text-fg-subtle mt-1">Checking ENS name…</p>
        {:else if ensHint.kind === 'ens'}
          <div class="mt-1 text-xs text-fg-muted space-y-0.5">
            <p>
              Normalised: <span class="font-mono text-fg">{ensHint.normalised}</span>
            </p>
            <p class="break-all">
              Namehash: <span class="font-mono">{ensHint.namehash}</span>
            </p>
            <p class="text-amber-400">
              ENS resolution is not wired into GreenWallet yet — paste the resolved 0x address to send.
            </p>
          </div>
        {:else if ensHint.kind === 'error'}
          <p class="text-xs text-rose-400 mt-1">{ensHint.message}</p>
        {/if}
      </div>

      <Input
        label="Amount"
        type="text"
        bind:value={amount}
        placeholder="0.0"
        hint={selected
          ? `In ${selected.symbol}. Up to ${selected.decimals} decimal places.`
          : undefined}
      />

      {#if feeOptions.length > 0}
        <div>
          <span class="text-sm text-fg-muted block mb-2">Network fee</span>
          <div class="grid grid-cols-3 gap-2">
            {#each feeOptions as opt}
              <button
                class="rounded-xl px-3 py-3 border text-left transition {selectedLevel === opt.level
                  ? 'border-accent bg-accent/10'
                  : 'border-border bg-bg-elevated'}"
                on:click={() => (selectedLevel = opt.level)}
              >
                <div class="text-sm font-semibold capitalize">{opt.level}</div>
                <div class="text-xs text-fg-muted font-mono mt-1">
                  {formatAmount(opt.estimated_fee)}
                </div>
                <div class="text-[10px] text-fg-subtle mt-0.5">
                  ~{Math.max(1, Math.round(opt.eta_seconds / 60))} min
                </div>
              </button>
            {/each}
          </div>
          {#if selected?.kind === 'token'}
            <p class="text-xs text-fg-subtle mt-2">
              Tokens are paid for in the chain's native asset (gas / energy).
            </p>
          {/if}
        </div>
      {/if}

      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
      {#if result}
        <div class="rounded-xl border border-success/30 bg-success/10 p-4">
          <div class="text-sm font-semibold text-success">Broadcast successful</div>
          <div class="text-xs text-fg-muted mt-1 font-mono break-all">{result.txid}</div>
        </div>
      {/if}

      <Button
        fullWidth
        loading={busy}
        disabled={!selected || !to || !amount || blocklistHit !== null}
        on:click={send}
      >
        Review &amp; send
      </Button>
    </div>
  </Card>
</div>

{#if scanOpen}
  <QrScanner on:scan={onScan} on:close={() => (scanOpen = false)} />
{/if}
