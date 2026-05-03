<script lang="ts">
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { wallet } from '$lib/stores/wallet';
  import { errorMessage } from '$lib/api';

  let phrase = '';
  let passphrase = '';
  let password = '';
  let confirmPwd = '';
  let busy = false;
  let error = '';

  async function submit() {
    error = '';
    if (password.length < 8) { error = 'Password must be at least 8 characters.'; return; }
    if (password !== confirmPwd) { error = 'Passwords do not match.'; return; }
    const wc = phrase.trim().split(/\s+/).length;
    if (wc !== 12 && wc !== 24) { error = 'Phrase must be 12 or 24 words.'; return; }
    busy = true;
    try {
      await wallet.importExisting(password, phrase, passphrase || undefined);
      void goto('/portfolio');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="min-h-screen grid place-items-center px-6 py-10">
  <div class="w-full max-w-xl">
    <Card title="Import a wallet" subtitle="Recover an existing BIP-39 wallet on this device.">
      <div class="space-y-4">
        <Input label="Recovery phrase" multiline rows={4} bind:value={phrase} placeholder="word1 word2 …" autocomplete="off" />
        <Input label="Passphrase (optional, BIP-39 25th word)" type="password" bind:value={passphrase} autocomplete="off" />
        <Input label="New password" type="password" bind:value={password} autocomplete="new-password" />
        <Input label="Confirm password" type="password" bind:value={confirmPwd} autocomplete="new-password" />
        {#if error}<p class="text-sm text-danger">{error}</p>{/if}
        <div class="flex gap-3">
          <Button variant="secondary" on:click={() => goto('/onboarding')}>Back</Button>
          <Button fullWidth loading={busy} on:click={submit}>Import</Button>
        </div>
      </div>
    </Card>
  </div>
</div>
