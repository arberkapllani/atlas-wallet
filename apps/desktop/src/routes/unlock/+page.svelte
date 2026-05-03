<script lang="ts">
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { wallet } from '$lib/stores/wallet';

  let password = '';
  let busy = false;
  let error = '';

  async function unlock() {
    error = '';
    busy = true;
    try {
      await wallet.unlock(password);
      void goto('/portfolio');
    } catch (e) {
      error = 'Invalid password.';
      console.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="min-h-screen grid place-items-center px-6">
  <div class="w-full max-w-md">
    <Card title="Welcome back" subtitle="Unlock your wallet to continue.">
      <form on:submit|preventDefault={unlock} class="space-y-4">
        <Input label="Password" type="password" bind:value={password} autocomplete="current-password" />
        {#if error}<p class="text-sm text-danger">{error}</p>{/if}
        <Button type="submit" fullWidth loading={busy}>Unlock</Button>
      </form>
    </Card>
  </div>
</div>
