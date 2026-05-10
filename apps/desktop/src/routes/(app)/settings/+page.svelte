<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { goto } from '$app/navigation';
  import { wallet } from '$lib/stores/wallet';

  // Section components
  import BackupPhraseCard from '$lib/settings/BackupPhraseCard.svelte';
  import AutoLockCard from '$lib/settings/AutoLockCard.svelte';
  import BiometricStatusCard from '$lib/settings/BiometricStatusCard.svelte';
  import SpendLimitsCard from '$lib/settings/SpendLimitsCard.svelte';
  import BlocklistCard from '$lib/settings/BlocklistCard.svelte';
  import ShamirCard from '$lib/settings/ShamirCard.svelte';

  import NetworkHealthCard from '$lib/settings/NetworkHealthCard.svelte';
  import RpcEndpointsCard from '$lib/settings/RpcEndpointsCard.svelte';
  import TorCard from '$lib/settings/TorCard.svelte';
  import NodeConfigCard from '$lib/settings/NodeConfigCard.svelte';

  import ExchangeSettingsCard from '$lib/settings/ExchangeSettingsCard.svelte';
  import SwapFeeCard from '$lib/settings/SwapFeeCard.svelte';
  import TradesCard from '$lib/settings/TradesCard.svelte';

  import CoinControlCard from '$lib/settings/CoinControlCard.svelte';
  import SilentPaymentsCard from '$lib/settings/SilentPaymentsCard.svelte';
  import UrlSafetyCard from '$lib/settings/UrlSafetyCard.svelte';
  import EventLogCard from '$lib/settings/EventLogCard.svelte';

  import ContactsCard from '$lib/settings/ContactsCard.svelte';
  import ApprovalsCard from '$lib/settings/ApprovalsCard.svelte';

  import CalldataCard from '$lib/settings/CalldataCard.svelte';
  import TypedDataCard from '$lib/settings/TypedDataCard.svelte';
  import AaUserOpCard from '$lib/settings/AaUserOpCard.svelte';
  import PayUriCard from '$lib/settings/PayUriCard.svelte';
  import DappRegistryCard from '$lib/settings/DappRegistryCard.svelte';
  import NftGalleryCard from '$lib/settings/NftGalleryCard.svelte';
  import WalletConnectCard from '$lib/settings/WalletConnectCard.svelte';

  type Section =
    | 'security'
    | 'network'
    | 'trading'
    | 'privacy'
    | 'contacts'
    | 'developer'
    | 'about';

  const sections: { id: Section; label: string; icon: string; desc: string }[] = [
    {
      id: 'security',
      label: 'Security',
      desc: 'Recovery phrase, auto-lock, biometrics, spend limits',
      icon: 'M12 2l8 4v6c0 5-3.5 8.5-8 10-4.5-1.5-8-5-8-10V6l8-4z'
    },
    {
      id: 'network',
      label: 'Network',
      desc: 'RPC endpoints, Tor, sovereign node policy',
      icon: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM2 12h20M12 2c2.5 3 4 6.5 4 10s-1.5 7-4 10c-2.5-3-4-6.5-4-10s1.5-7 4-10z'
    },
    {
      id: 'trading',
      label: 'Trading',
      desc: 'Exchange aggregators, swap fees, P&L',
      icon: 'M3 17l6-6 4 4 8-8M14 7h7v7'
    },
    {
      id: 'privacy',
      label: 'Privacy',
      desc: 'Coin control, silent payments, URL safety',
      icon: 'M12 2l8 4v6c0 5-3.5 8.5-8 10-4.5-1.5-8-5-8-10V6l8-4zM9 11l2 2 4-4'
    },
    {
      id: 'contacts',
      label: 'Address book',
      desc: 'Contacts and ERC-20 approvals',
      icon: 'M16 14a4 4 0 1 0-8 0M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21v-2a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v2'
    },
    {
      id: 'developer',
      label: 'Advanced',
      desc: 'Calldata, EIP-712, AA, WalletConnect, dApp registry',
      icon: 'M8 6l-6 6 6 6M16 6l6 6-6 6M14 4l-4 16'
    },
    {
      id: 'about',
      label: 'About',
      desc: 'Version, license, lock wallet',
      icon: 'M12 16v-4M12 8h.01M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0z'
    }
  ];

  let active: Section = 'security';
</script>

<div class="p-4 sm:p-6 lg:p-8 max-w-6xl mx-auto space-y-6 animate-fade-in-up">
  <header>
    <h1 class="text-2xl sm:text-3xl font-display font-bold tracking-tight">Settings</h1>
    <p class="text-fg-muted text-sm mt-1">Manage your wallet and security preferences.</p>
  </header>

  <div class="grid lg:grid-cols-[260px_1fr] gap-6">
    <!-- Section nav -->
    <aside class="lg:sticky lg:top-4 self-start">
      <!-- Mobile/tablet: horizontal scrolling pills -->
      <div class="lg:hidden -mx-4 px-4 overflow-x-auto pb-2">
        <div class="flex gap-2 min-w-max">
          {#each sections as s}
            <button
              type="button"
              on:click={() => (active = s.id)}
              class="px-4 py-2 rounded-full text-sm font-semibold border transition whitespace-nowrap {active ===
              s.id
                ? 'bg-brand-600 text-white border-brand-600 shadow-glow'
                : 'bg-bg-elevated border-border text-fg-muted hover:text-fg'}"
            >
              {s.label}
            </button>
          {/each}
        </div>
      </div>

      <!-- Desktop: vertical list -->
      <nav
        class="hidden lg:flex flex-col gap-1 p-2 rounded-2xl bg-bg-elevated/40 border border-border-subtle"
      >
        {#each sections as s}
          <button
            type="button"
            on:click={() => (active = s.id)}
            class="flex items-start gap-3 px-3 py-2.5 rounded-xl text-left transition {active ===
            s.id
              ? 'bg-brand-600/10 border border-brand-500/40'
              : 'border border-transparent hover:bg-bg-elevated'}"
          >
            <span
              class="mt-0.5 h-8 w-8 rounded-lg grid place-items-center flex-none {active === s.id
                ? 'bg-brand-gradient text-white'
                : 'bg-bg-elevated text-fg-muted'}"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4">
                <path stroke-linecap="round" stroke-linejoin="round" d={s.icon} />
              </svg>
            </span>
            <span class="min-w-0">
              <span class="block text-sm font-semibold {active === s.id ? 'text-fg' : ''}">{s.label}</span>
              <span class="block text-xs text-fg-muted leading-snug">{s.desc}</span>
            </span>
          </button>
        {/each}
      </nav>
    </aside>

    <!-- Section content -->
    <section class="space-y-5 min-w-0">
      {#if active === 'security'}
        <BackupPhraseCard />
        <AutoLockCard />
        <BiometricStatusCard />
        <SpendLimitsCard />
        <BlocklistCard />
        <ShamirCard />
      {:else if active === 'network'}
        <NetworkHealthCard />
        <RpcEndpointsCard />
        <TorCard />
        <NodeConfigCard />
      {:else if active === 'trading'}
        <ExchangeSettingsCard />
        <SwapFeeCard />
        <TradesCard />
      {:else if active === 'privacy'}
        <CoinControlCard />
        <SilentPaymentsCard />
        <UrlSafetyCard />
        <EventLogCard />
      {:else if active === 'contacts'}
        <ContactsCard />
        <ApprovalsCard />
      {:else if active === 'developer'}
        <CalldataCard />
        <TypedDataCard />
        <AaUserOpCard />
        <PayUriCard />
        <DappRegistryCard />
        <NftGalleryCard />
        <WalletConnectCard />
      {:else if active === 'about'}
        <Card title="Security details">
          <div class="space-y-3 text-sm">
            <div class="flex justify-between">
              <span class="text-fg-muted">Encryption</span>
              <span class="font-mono">AES-256-GCM</span>
            </div>
            <div class="flex justify-between">
              <span class="text-fg-muted">Key derivation</span>
              <span class="font-mono">Argon2id (m=64MiB, t=3, p=4)</span>
            </div>
            <div class="flex justify-between">
              <span class="text-fg-muted">Network telemetry</span>
              <span>None</span>
            </div>
          </div>
        </Card>

        <Card title="About GreenWallet">
          <div class="space-y-2 text-sm text-fg-muted">
            <p>GreenWallet — sovereign multi-chain desktop wallet.</p>
            <p>Open source. Non-custodial. Zero telemetry.</p>
            <p class="font-mono text-xs">v0.1.0 (Phase 1)</p>
          </div>
        </Card>

        <Card title="Lock wallet">
          <p class="text-sm text-fg-muted mb-3">
            End the current session. You will need your password to unlock again.
          </p>
          <Button
            variant="secondary"
            on:click={async () => {
              await wallet.lock();
              await goto('/unlock');
            }}
          >
            Lock now
          </Button>
        </Card>
      {/if}
    </section>
  </div>
</div>
