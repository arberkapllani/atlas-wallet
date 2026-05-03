<script lang="ts">
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
</script>

<div class="min-h-screen grid place-items-center px-6">
  <div class="w-full max-w-xl space-y-6">
    <header class="text-center space-y-2">
      <h1 class="text-4xl font-bold tracking-tight">Welcome to Atlas</h1>
      <p class="text-fg-muted">A sovereign multi-chain wallet. Your keys, your coins.</p>
    </header>

    <Card>
      <div class="space-y-4">
        <a href="/onboarding/create" class="block">
          <div class="rounded-xl border border-border bg-bg-elevated p-5 hover:border-accent transition cursor-pointer">
            <h3 class="font-semibold mb-1">Create a new wallet</h3>
            <p class="text-sm text-fg-muted">
              Generate a fresh 12-word recovery phrase. Your keys never leave this device.
            </p>
          </div>
        </a>

        <a href="/onboarding/import" class="block">
          <div class="rounded-xl border border-border bg-bg-elevated p-5 hover:border-accent transition cursor-pointer">
            <h3 class="font-semibold mb-1">Import an existing wallet</h3>
            <p class="text-sm text-fg-muted">
              Recover from a 12 or 24-word BIP-39 phrase. Optional 25th-word passphrase supported.
            </p>
          </div>
        </a>
      </div>
    </Card>

    <p class="text-xs text-fg-subtle text-center">
      Open source. Non-custodial. No accounts. No KYC.
    </p>
  </div>
</div>

