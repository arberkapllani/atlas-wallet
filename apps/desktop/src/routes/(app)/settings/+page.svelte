<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import RpcEndpointsCard from '$lib/settings/RpcEndpointsCard.svelte';
  import NetworkHealthCard from '$lib/settings/NetworkHealthCard.svelte';
  import ExchangeSettingsCard from '$lib/settings/ExchangeSettingsCard.svelte';
  import { goto } from '$app/navigation';
  import { wallet } from '$lib/stores/wallet';
</script>

<div class="p-8 max-w-2xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Settings</h1>
    <p class="text-fg-muted text-sm mt-1">Manage your wallet and security preferences.</p>
  </header>

  <NetworkHealthCard />

  <RpcEndpointsCard />

  <ExchangeSettingsCard />

  <Card title="Security">
    <div class="space-y-3 text-sm">
      <div class="flex justify-between">
        <span class="text-fg-muted">Encryption</span>
        <span class="font-mono">AES-256-GCM</span>
      </div>
      <div class="flex justify-between">
        <span class="text-fg-muted">KDF</span>
        <span class="font-mono">Argon2id (m=64MiB, t=3, p=4)</span>
      </div>
      <div class="flex justify-between">
        <span class="text-fg-muted">Auto-lock</span>
        <span>5 minutes idle</span>
      </div>
    </div>
  </Card>

  <Card title="About">
    <div class="space-y-2 text-sm text-fg-muted">
      <p>Atlas — sovereign multi-chain desktop wallet.</p>
      <p>Open source. Non-custodial. Zero telemetry.</p>
      <p class="font-mono text-xs">v0.1.0 (Phase 1)</p>
    </div>
  </Card>

  <Card title="Lock now">
    <Button
      variant="secondary"
      on:click={async () => {
        await wallet.lock();
        await goto('/unlock');
      }}
    >
      Lock wallet
    </Button>
  </Card>
</div>

