<script lang="ts">
  import { onMount } from 'svelte';
  // @ts-expect-error - qrcode lacks bundled types, runtime contract is stable
  import QRCode from 'qrcode';

  export let value: string;
  export let size = 200;

  let dataUrl = '';

  $: if (value) {
    QRCode.toDataURL(value, {
      width: size,
      margin: 1,
      color: { dark: '#e7eaf3', light: '#0b0d14' }
    })
      .then((u: string) => (dataUrl = u))
      .catch(() => (dataUrl = ''));
  }

  onMount(() => {});
</script>

{#if dataUrl}
  <img
    src={dataUrl}
    alt="QR code"
    width={size}
    height={size}
    class="rounded-xl border border-border-subtle bg-bg p-3"
  />
{:else}
  <div
    class="rounded-xl border border-border-subtle bg-bg-elevated"
    style="width:{size}px;height:{size}px"
  ></div>
{/if}
