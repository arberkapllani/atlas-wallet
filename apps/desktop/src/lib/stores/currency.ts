import { writable, derived, get } from 'svelte/store';
import { api, type FiatCurrency } from '$lib/api';

const SYMBOLS: Record<FiatCurrency, string> = {
  usd: '$',
  eur: '€',
  gbp: '£'
};

const LOCALES: Record<FiatCurrency, string> = {
  usd: 'en-US',
  eur: 'de-DE',
  gbp: 'en-GB'
};

/** Persisted display currency. Defaults to USD until the backend reports. */
export const fiatCurrency = writable<FiatCurrency>('usd');

/** Symbol for the currently selected currency (`$`, `€`, `£`). */
export const fiatSymbol = derived(fiatCurrency, ($c) => SYMBOLS[$c]);

/** Hydrate the store from the backend's persisted setting. */
export async function loadFiatCurrency(): Promise<void> {
  try {
    const c = await api.getFiatCurrency();
    fiatCurrency.set(c);
  } catch (e) {
    console.warn('failed to load fiat currency', e);
  }
}

/** Persist a new display currency. */
export async function setFiatCurrency(c: FiatCurrency): Promise<void> {
  await api.setFiatCurrency(c);
  fiatCurrency.set(c);
}

/**
 * Format a number using the locale that pairs with the selected currency.
 * Plain (non-reactive) helper. Prefer `$formatFiat(value)` in Svelte
 * templates so they re-render when the user picks a different currency.
 */
export function formatFiat(value: number, currency?: FiatCurrency): string {
  const c = currency ?? get(fiatCurrency);
  return new Intl.NumberFormat(LOCALES[c], {
    style: 'currency',
    currency: c.toUpperCase(),
    maximumFractionDigits: 2
  }).format(value);
}

/**
 * Reactive formatter store. In a Svelte template use:
 *   {$formatFiatStore(value)}
 * and the rendered string will update whenever the user changes currency.
 */
export const formatFiatStore = derived(
  fiatCurrency,
  ($c) =>
    (value: number): string =>
      new Intl.NumberFormat(LOCALES[$c], {
        style: 'currency',
        currency: $c.toUpperCase(),
        maximumFractionDigits: 2
      }).format(value)
);
