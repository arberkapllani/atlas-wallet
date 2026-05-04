<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type DecodedCall } from '$lib/api';

  let calldata = '';
  let decoded: DecodedCall | null = null;
  let error = '';
  let busy = false;

  async function decode() {
    if (!calldata.trim()) return;
    busy = true;
    error = '';
    decoded = null;
    try {
      decoded = await api.calldataDecode(calldata.trim());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function clear() {
    calldata = '';
    decoded = null;
    error = '';
  }

  function isSensitive(d: DecodedCall): boolean {
    return (
      d.tag === 'Erc20Approve' ||
      d.tag === 'Erc20IncreaseAllowance' ||
      (d.tag === 'NftSetApprovalForAll' && d.approved === true)
    );
  }

  function actionVerb(d: DecodedCall): string {
    switch (d.tag) {
      case 'NativeTransfer':
      case 'Erc20Transfer':
        return 'transfer';
      case 'Erc20TransferFrom':
        return 'transferFrom';
      case 'Erc20Approve':
      case 'Erc20IncreaseAllowance':
      case 'Erc20DecreaseAllowance':
        return 'approve';
      case 'NftSafeTransferFrom':
        return 'nft-transfer';
      case 'NftSetApprovalForAll':
        return 'nft-approve-all';
      case 'WrapDeposit':
        return 'wrap';
      case 'WrapWithdraw':
        return 'unwrap';
      case 'Multicall':
        return 'multicall';
      case 'Unknown':
        return 'unknown';
    }
  }
</script>

<Card
  title="Calldata decoder"
  subtitle="Paste raw EVM transaction data (with or without 0x prefix) to decode the action before signing."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <textarea
      bind:value={calldata}
      rows="3"
      placeholder="0xa9059cbb000000000000000000000000…"
      class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
    ></textarea>

    <div class="flex items-center gap-2">
      <Button variant="primary" on:click={decode} disabled={busy || !calldata.trim()}>
        Decode
      </Button>
      {#if decoded || calldata}
        <Button variant="secondary" on:click={clear} disabled={busy}>Clear</Button>
      {/if}
    </div>

    {#if decoded}
      {@const sensitive = isSensitive(decoded)}
      <div
        class="rounded-lg border p-3 space-y-2 {sensitive
          ? 'border-amber-500/40 bg-amber-500/5'
          : 'border-border-subtle'}"
      >
        <div class="flex items-baseline justify-between gap-2">
          <p class="text-xs font-medium">{actionVerb(decoded)}</p>
          <p class="text-[10px] text-fg-subtle">{decoded.tag}</p>
        </div>
        {#if sensitive}
          <p class="text-[11px] text-amber-400">
            High-impact operation: review the spender / operator carefully before signing.
          </p>
        {/if}
        {#if decoded.tag === 'NativeTransfer'}
          <p class="text-xs text-fg-muted">Plain native-asset transfer (empty calldata).</p>
        {:else if decoded.tag === 'Erc20Transfer'}
          <p class="text-xs">to: <span class="font-mono">{decoded.recipient}</span></p>
          <p class="text-xs">amount: <span class="font-mono">{decoded.amount}</span></p>
        {:else if decoded.tag === 'Erc20TransferFrom'}
          <p class="text-xs">from: <span class="font-mono">{decoded.from}</span></p>
          <p class="text-xs">to: <span class="font-mono">{decoded.to}</span></p>
          <p class="text-xs">amount: <span class="font-mono">{decoded.amount}</span></p>
        {:else if decoded.tag === 'Erc20Approve'}
          <p class="text-xs">spender: <span class="font-mono">{decoded.spender}</span></p>
          <p class="text-xs">
            amount:
            <span class="font-mono">{decoded.amount}</span>
            {#if decoded.unlimited}
              <span class="text-rose-400 ml-1">(unlimited)</span>
            {/if}
          </p>
        {:else if decoded.tag === 'Erc20IncreaseAllowance'}
          <p class="text-xs">spender: <span class="font-mono">{decoded.spender}</span></p>
          <p class="text-xs">added: <span class="font-mono">{decoded.added}</span></p>
        {:else if decoded.tag === 'Erc20DecreaseAllowance'}
          <p class="text-xs">spender: <span class="font-mono">{decoded.spender}</span></p>
          <p class="text-xs">subtracted: <span class="font-mono">{decoded.subtracted}</span></p>
        {:else if decoded.tag === 'NftSafeTransferFrom'}
          <p class="text-xs">from: <span class="font-mono">{decoded.from}</span></p>
          <p class="text-xs">to: <span class="font-mono">{decoded.to}</span></p>
          <p class="text-xs">tokenId: <span class="font-mono">{decoded.token_id}</span></p>
        {:else if decoded.tag === 'NftSetApprovalForAll'}
          <p class="text-xs">operator: <span class="font-mono">{decoded.operator}</span></p>
          <p class="text-xs">
            approved:
            <span class="font-mono {decoded.approved ? 'text-rose-400' : ''}"
              >{decoded.approved}</span
            >
          </p>
        {:else if decoded.tag === 'WrapDeposit'}
          <p class="text-xs text-fg-muted">deposit() — wrap native asset.</p>
        {:else if decoded.tag === 'WrapWithdraw'}
          <p class="text-xs">amount: <span class="font-mono">{decoded.amount}</span></p>
        {:else if decoded.tag === 'Multicall'}
          <p class="text-xs">inner calls: <span class="font-mono">{decoded.inner_count}</span></p>
        {:else if decoded.tag === 'Unknown'}
          <p class="text-xs">selector: <span class="font-mono">{decoded.selector}</span></p>
          <p class="text-xs text-fg-muted">
            Selector did not match any known shape — exercise caution.
          </p>
        {/if}
      </div>
    {/if}
  </div>
</Card>
