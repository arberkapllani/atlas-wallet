import { writable } from 'svelte/store';
import { api, type PricePoint } from '$lib/api';

/** Map from chain id (`btc`, `eth`, …) to the CoinGecko id we query. */
export const COINGECKO_IDS: Record<string, string> = {
  btc: 'bitcoin',
  eth: 'ethereum',
  polygon: 'matic-network',
  arbitrum: 'ethereum', // gas pays in ETH
  optimism: 'ethereum',
  base: 'ethereum',
  bsc: 'binancecoin',
  avalanche: 'avalanche-2'
};

export const prices = writable<Record<string, PricePoint>>({});

let interval: ReturnType<typeof setInterval> | null = null;

export async function startPriceFeed(chainIds: string[]) {
  const ids = Array.from(new Set(chainIds.map((c) => COINGECKO_IDS[c]).filter(Boolean)));
  const fetchOnce = async () => {
    try {
      const map = await api.getPrices(ids);
      prices.set(map);
    } catch (e) {
      console.warn('price fetch failed', e);
    }
  };
  await fetchOnce();
  if (interval) clearInterval(interval);
  // CoinGecko cache TTL is 5 min; we refresh every 60s and rely on the
  // backend cache to coalesce.
  interval = setInterval(fetchOnce, 60_000);
}

export function stopPriceFeed() {
  if (interval) {
    clearInterval(interval);
    interval = null;
  }
}
