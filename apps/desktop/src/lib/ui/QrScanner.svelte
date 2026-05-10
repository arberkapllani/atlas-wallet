<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import jsQR from 'jsqr';
  import Button from '$lib/ui/Button.svelte';

  const dispatch = createEventDispatcher<{ scan: string; close: void }>();

  let video: HTMLVideoElement;
  let canvas: HTMLCanvasElement;
  let stream: MediaStream | null = null;
  let raf = 0;
  let error = '';
  let starting = true;

  async function start() {
    starting = true;
    error = '';
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: 'environment' },
        audio: false
      });
      video.srcObject = stream;
      await video.play();
      starting = false;
      tick();
    } catch (e) {
      error = (e as Error).message ?? String(e);
      starting = false;
    }
  }

  function tick() {
    if (!video || !canvas) {
      raf = requestAnimationFrame(tick);
      return;
    }
    if (video.readyState !== video.HAVE_ENOUGH_DATA) {
      raf = requestAnimationFrame(tick);
      return;
    }
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return;
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
    const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
    const code = jsQR(img.data, img.width, img.height, { inversionAttempts: 'dontInvert' });
    if (code && code.data) {
      stop();
      dispatch('scan', code.data);
      return;
    }
    raf = requestAnimationFrame(tick);
  }

  function stop() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    if (stream) {
      for (const t of stream.getTracks()) t.stop();
      stream = null;
    }
  }

  function close() {
    stop();
    dispatch('close');
  }

  onMount(start);
  onDestroy(stop);
</script>

<div class="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
  <div
    class="bg-bg-subtle border border-border-subtle rounded-2xl shadow-card w-full max-w-md overflow-hidden"
  >
    <div class="px-5 py-4 border-b border-border-subtle flex items-center justify-between">
      <h2 class="font-semibold">Scan QR code</h2>
      <button
        class="text-fg-muted hover:text-fg text-xl leading-none px-2"
        aria-label="Close"
        on:click={close}>×</button
      >
    </div>

    <div class="p-5 space-y-3">
      {#if error}
        <p class="text-sm text-danger">Camera error: {error}</p>
        <p class="text-xs text-fg-subtle">Make sure Atlas has permission to use your camera.</p>
      {:else}
        <div class="relative aspect-square w-full bg-black rounded-xl overflow-hidden">
          <!-- svelte-ignore a11y-media-has-caption -->
          <video
            bind:this={video}
            class="absolute inset-0 w-full h-full object-cover"
            playsinline
            muted
          ></video>
          <div
            class="absolute inset-8 border-2 border-accent/80 rounded-xl pointer-events-none"
            aria-hidden="true"
          ></div>
        </div>
        <p class="text-xs text-fg-subtle text-center">
          {starting ? 'Starting camera…' : 'Point the camera at the QR code.'}
        </p>
      {/if}

      <canvas bind:this={canvas} class="hidden"></canvas>

      <Button variant="ghost" fullWidth on:click={close}>Cancel</Button>
    </div>
  </div>
</div>
