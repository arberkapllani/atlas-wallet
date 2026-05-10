<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type EventRecord,
    type EventLevel,
    type EventCategory
  } from '$lib/api';

  const LEVELS: EventLevel[] = ['Debug', 'Info', 'Warn', 'Error'];
  const CATEGORIES: EventCategory[] = [
    'Wallet',
    'Transaction',
    'Network',
    'Security',
    'Ui',
    'Other'
  ];

  let minLevel: EventLevel = 'Info';
  let category: EventCategory | '' = '';
  let events: EventRecord[] = [];
  let loading = false;
  let error = '';

  async function load() {
    loading = true;
    error = '';
    try {
      events = await api.eventsFilter(minLevel, category === '' ? null : category, 100);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function clearAll() {
    error = '';
    try {
      await api.eventsClear();
      await load();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function copyRedacted() {
    error = '';
    try {
      const redacted = await api.eventsExportRedacted();
      const text = redacted
        .map(
          (e) =>
            `[${new Date(e.timestamp_unix_ms).toISOString()}] ${e.level} ${e.category}: ${e.message}`
        )
        .join('\n');
      await navigator.clipboard.writeText(text);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function levelTone(level: EventLevel): string {
    switch (level) {
      case 'Error':
        return 'text-rose-400';
      case 'Warn':
        return 'text-amber-400';
      case 'Info':
        return 'text-fg';
      default:
        return 'text-fg-subtle';
    }
  }

  function ts(unix_ms: number): string {
    return new Date(unix_ms).toLocaleTimeString();
  }

  let timer: ReturnType<typeof setInterval> | null = null;
  onMount(() => {
    load();
    timer = setInterval(load, 5000);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  $: (minLevel, category, load());
</script>

<Card
  title="Event log"
  subtitle="In-memory ring buffer (last ~500 events). Cleared when the wallet quits."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex flex-wrap items-center gap-2">
      <label class="text-xs text-fg-muted flex items-center gap-2">
        Min level
        <select
          bind:value={minLevel}
          class="h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
        >
          {#each LEVELS as l}
            <option value={l}>{l}</option>
          {/each}
        </select>
      </label>

      <label class="text-xs text-fg-muted flex items-center gap-2">
        Category
        <select
          bind:value={category}
          class="h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
        >
          <option value="">All</option>
          {#each CATEGORIES as c}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </label>

      <div class="ml-auto flex items-center gap-2">
        <Button variant="secondary" on:click={copyRedacted}>Copy redacted</Button>
        <Button variant="secondary" on:click={clearAll}>Clear</Button>
      </div>
    </div>

    {#if loading && events.length === 0}
      <p class="text-fg-subtle text-xs">Loading…</p>
    {:else if events.length === 0}
      <p class="text-fg-subtle text-xs">No events match the current filter.</p>
    {:else}
      <ul
        class="border border-border-subtle rounded-lg max-h-72 overflow-y-auto divide-y divide-border-subtle"
      >
        {#each events as ev}
          <li class="px-3 py-2 flex items-start gap-3">
            <span class="font-mono text-xs text-fg-subtle shrink-0 w-20">
              {ts(ev.timestamp_unix_ms)}
            </span>
            <span class="text-xs uppercase shrink-0 w-12 {levelTone(ev.level)}">
              {ev.level}
            </span>
            <span class="text-xs text-fg-muted shrink-0 w-20">{ev.category}</span>
            <span class="text-xs text-fg break-all">{ev.message}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</Card>
