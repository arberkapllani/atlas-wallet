<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import RpcEndpointsCard from '$lib/settings/RpcEndpointsCard.svelte';
  import NetworkHealthCard from '$lib/settings/NetworkHealthCard.svelte';
  import ExchangeSettingsCard from '$lib/settings/ExchangeSettingsCard.svelte';
  import SwapFeeCard from '$lib/settings/SwapFeeCard.svelte';
  import AutoLockCard from '$lib/settings/AutoLockCard.svelte';
  import ContactsCard from '$lib/settings/ContactsCard.svelte';
  import SpendLimitsCard from '$lib/settings/SpendLimitsCard.svelte';
  import EventLogCard from '$lib/settings/EventLogCard.svelte';
  import BlocklistCard from '$lib/settings/BlocklistCard.svelte';
  import UrlSafetyCard from '$lib/settings/UrlSafetyCard.svelte';
  import ApprovalsCard from '$lib/settings/ApprovalsCard.svelte';
  import CalldataCard from '$lib/settings/CalldataCard.svelte';
  import TypedDataCard from '$lib/settings/TypedDataCard.svelte';
  import TradesCard from '$lib/settings/TradesCard.svelte';
  import PayUriCard from '$lib/settings/PayUriCard.svelte';
  import DappRegistryCard from '$lib/settings/DappRegistryCard.svelte';
  import NftGalleryCard from '$lib/settings/NftGalleryCard.svelte';
  import AaUserOpCard from '$lib/settings/AaUserOpCard.svelte';
  import ShamirCard from '$lib/settings/ShamirCard.svelte';
  import WalletConnectCard from '$lib/settings/WalletConnectCard.svelte';
  import BiometricStatusCard from '$lib/settings/BiometricStatusCard.svelte';
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

  <SwapFeeCard />

  <AutoLockCard />

  <ContactsCard />

  <SpendLimitsCard />

  <EventLogCard />

  <BlocklistCard />

  <UrlSafetyCard />

  <ApprovalsCard />

  <CalldataCard />

  <TypedDataCard />

  <TradesCard />

  <PayUriCard />

  <DappRegistryCard />

  <NftGalleryCard />

  <AaUserOpCard />

  <ShamirCard />

  <WalletConnectCard />

  <BiometricStatusCard />

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
        <span>configurable above</span>
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

