import { get, writable } from 'svelte/store';
import { api, type PricePoint } from '$lib/api';
import { fiatCurrency } from './currency';

/** Map from chain id (`btc`, `eth`, …) to the CoinGecko id we query. */
export const COINGECKO_IDS: Record<string, string> = {
  btc: 'bitcoin',
  eth: 'ethereum',
  polygon: 'matic-network',
  arbitrum: 'ethereum', // gas pays in ETH
  optimism: 'ethereum',
  base: 'ethereum',
  bsc: 'binancecoin',
  avalanche: 'avalanche-2',
  sol: 'solana',
  trx: 'tron'
};

export const prices = writable<Record<string, PricePoint>>({});

let interval: ReturnType<typeof setInterval> | null = null;
let trackedIds: string[] = [];
let unsubCurrency: (() => void) | null = null;

async function fetchOnce() {
  if (trackedIds.length === 0) return;
  try {
    const map = await api.getPrices(trackedIds, get(fiatCurrency));
    prices.set(map);
  } catch (e) {
    console.warn('price fetch failed', e);
  }
}

export async function startPriceFeed(chainIds: string[]) {
  trackedIds = Array.from(new Set(chainIds.map((c) => COINGECKO_IDS[c]).filter(Boolean)));
  await fetchOnce();
  if (interval) clearInterval(interval);
  // CoinGecko cache TTL is 5 min; refresh every 60s and rely on backend
  // cache to coalesce.
  interval = setInterval(fetchOnce, 60_000);
  // Re-fetch immediately whenever the user picks a different currency.
  if (unsubCurrency) unsubCurrency();
  let first = true;
  unsubCurrency = fiatCurrency.subscribe(() => {
    if (first) {
      first = false;
      return;
    }
    void fetchOnce();
  });
}

export function stopPriceFeed() {
  if (interval) {
    clearInterval(interval);
    interval = null;
  }
  if (unsubCurrency) {
    unsubCurrency();
    unsubCurrency = null;
  }
  trackedIds = [];
}
