<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type UserOperation, type UserOpHashes } from '$lib/api';

  // Default EntryPoint v0.6 deployed at the canonical address on every
  // chain that supports ERC-4337.
  const ENTRY_POINT_V06 = '0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789';

  let entryPoint = ENTRY_POINT_V06;
  let chainId = 1;
  let sender = '';
  let nonce = 0;
  let initCode = '0x';
  let callTarget = '';
  let callValue = '0';
  let callData = '0x';
  let callGasLimit = 100000;
  let verificationGasLimit = 100000;
  let preVerificationGas = 21000;
  let maxFeePerGas = 1000000000;
  let maxPriorityFeePerGas = 1000000000;
  let paymasterAndData = '0x';

  let encodedCallData = '';
  let hashes: UserOpHashes | null = null;
  let error = '';
  let busy = false;

  async function encodeExecute() {
    if (!callTarget.trim()) {
      error = 'Set a call target first.';
      return;
    }
    busy = true;
    error = '';
    try {
      encodedCallData = await api.aaEncodeExecuteCalldata(
        callTarget.trim(),
        String(callValue),
        callData.trim() || '0x'
      );
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function compute() {
    if (!sender.trim()) {
      error = 'Set sender (smart-account address) first.';
      return;
    }
    busy = true;
    error = '';
    hashes = null;
    try {
      const op: UserOperation = {
        sender: sender.trim(),
        nonce: Number(nonce),
        init_code: initCode.trim() || '0x',
        call_data: (encodedCallData || callData).trim() || '0x',
        call_gas_limit: Number(callGasLimit),
        verification_gas_limit: Number(verificationGasLimit),
        pre_verification_gas: Number(preVerificationGas),
        max_fee_per_gas: Number(maxFeePerGas),
        max_priority_fee_per_gas: Number(maxPriorityFeePerGas),
        paymaster_and_data: paymasterAndData.trim() || '0x'
      };
      hashes = await api.aaUserOpHash(entryPoint.trim(), Number(chainId), op);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Card
  title="ERC-4337 user-op preview"
  subtitle="Build a UserOperation for EntryPoint v0.6, encode the SimpleAccount.execute(target,value,data) call-data, and compute the canonical userOpHash that the signer will sign. Pure offline calculation."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="grid grid-cols-2 gap-2">
      <label class="text-xs text-fg-muted block col-span-2">
        EntryPoint
        <input
          type="text"
          bind:value={entryPoint}
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
      <label class="text-xs text-fg-muted block">
        Chain ID
        <input
          type="number"
          bind:value={chainId}
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
      <label class="text-xs text-fg-muted block">
        Nonce
        <input
          type="number"
          bind:value={nonce}
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
      <label class="text-xs text-fg-muted block col-span-2">
        Sender (smart-account)
        <input
          type="text"
          bind:value={sender}
          placeholder="0x..."
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
    </div>

    <fieldset class="rounded-lg border border-border-subtle p-3 space-y-2">
      <legend class="px-1 text-[10px] uppercase text-fg-muted">execute(target, value, data)</legend>
      <input
        type="text"
        bind:value={callTarget}
        placeholder="target 0x..."
        class="w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
      />
      <div class="grid grid-cols-2 gap-2">
        <input
          type="text"
          bind:value={callValue}
          placeholder="value (wei)"
          class="h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
        <input
          type="text"
          bind:value={callData}
          placeholder="data 0x..."
          class="h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </div>
      <Button variant="secondary" on:click={encodeExecute} disabled={busy}>Encode execute()</Button>
      {#if encodedCallData}
        <p class="text-[10px] text-fg-subtle font-mono break-all">
          callData = {encodedCallData}
        </p>
      {/if}
    </fieldset>

    <details class="text-xs">
      <summary class="cursor-pointer text-fg-muted">Advanced (gas, paymaster, init code)</summary>
      <div class="mt-2 grid grid-cols-2 gap-2">
        <label>
          callGasLimit
          <input
            type="number"
            bind:value={callGasLimit}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label>
          verificationGasLimit
          <input
            type="number"
            bind:value={verificationGasLimit}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label>
          preVerificationGas
          <input
            type="number"
            bind:value={preVerificationGas}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label>
          maxFeePerGas
          <input
            type="number"
            bind:value={maxFeePerGas}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label>
          maxPriorityFeePerGas
          <input
            type="number"
            bind:value={maxPriorityFeePerGas}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label class="col-span-2">
          initCode
          <input
            type="text"
            bind:value={initCode}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <label class="col-span-2">
          paymasterAndData
          <input
            type="text"
            bind:value={paymasterAndData}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
      </div>
    </details>

    <Button variant="primary" on:click={compute} disabled={busy || !sender.trim()}>
      {busy ? 'Hashing…' : 'Compute userOpHash'}
    </Button>

    {#if hashes}
      <div class="rounded-lg border border-border-subtle p-3 space-y-1.5 text-xs">
        <p>
          <span class="text-fg-muted">userOpHash:</span>
          <span class="font-mono break-all">{hashes.user_op_hash}</span>
        </p>
        <p>
          <span class="text-fg-muted">packedHash:</span>
          <span class="font-mono break-all">{hashes.packed_hash}</span>
        </p>
        <p class="text-[10px] text-fg-subtle">
          The wallet's signer (HW or seed-derived key) signs <code>userOpHash</code> as a 65-byte ECDSA
          signature; that becomes the UserOperation.signature field.
        </p>
      </div>
    {/if}
  </div>
</Card>
