import { writable, derived, get } from 'svelte/store';
import { api, type ChainSummary, type Amount } from '$lib/api';

interface WalletState {
  initialized: boolean;
  unlocked: boolean;
  statusLoaded: boolean;
  chains: ChainSummary[];
  addresses: Record<string, string>; // chainId -> address
  balances: Record<string, Amount | null>;
  loading: boolean;
}

const initial: WalletState = {
  initialized: false,
  unlocked: false,
  statusLoaded: false,
  chains: [],
  addresses: {},
  balances: {},
  loading: false
};

function createWallet() {
  const state = writable<WalletState>(initial);
  let autoLockTimer: ReturnType<typeof setTimeout> | null = null;

  const armAutoLock = () => {
    if (autoLockTimer) clearTimeout(autoLockTimer);
    // 5-minute idle auto-lock.
    autoLockTimer = setTimeout(
      () => {
        void lock();
      },
      5 * 60 * 1000
    );
  };

  async function refreshStatus() {
    const [initialized, unlocked] = await Promise.all([api.vaultExists(), api.isUnlocked()]);
    state.update((s) => ({ ...s, initialized, unlocked, statusLoaded: true }));
    if (unlocked) armAutoLock();
  }

  async function loadChains() {
    const all = await api.listChains();
    const enabled = all.filter((c) => c.enabled_by_default);
    state.update((s) => ({ ...s, chains: enabled }));
    return enabled;
  }

  async function refreshAddresses() {
    const { chains } = get(state);
    const entries = await Promise.all(
      chains.map(async (c) => [c.id, await api.getAddress(c.id)] as const)
    );
    state.update((s) => ({ ...s, addresses: Object.fromEntries(entries) }));
  }

  async function refreshBalances() {
    const { chains } = get(state);
    state.update((s) => ({ ...s, loading: true }));
    const results = await Promise.allSettled(
      chains.map(async (c) => [c.id, await api.getBalance(c.id)] as const)
    );
    const balances: Record<string, Amount | null> = {};
    for (const r of results) {
      if (r.status === 'fulfilled') {
        const [id, amt] = r.value;
        balances[id] = amt;
      }
    }
    state.update((s) => ({ ...s, balances, loading: false }));
  }

  async function unlock(password: string) {
    await api.unlockWallet(password);
    await refreshStatus();
    await loadChains();
    await refreshAddresses();
    void refreshBalances();
  }

  async function lock() {
    await api.lockWallet();
    if (autoLockTimer) {
      clearTimeout(autoLockTimer);
      autoLockTimer = null;
    }
    state.set({ ...initial, initialized: true, unlocked: false, statusLoaded: true });
  }

  async function createNew(password: string, wordCount: 12 | 24): Promise<string> {
    const phrase = await api.createWallet(password, wordCount);
    await refreshStatus();
    await loadChains();
    await refreshAddresses();
    void refreshBalances();
    return phrase;
  }

  async function importExisting(password: string, phrase: string, passphrase?: string) {
    await api.importWallet(password, phrase, passphrase);
    await refreshStatus();
    await loadChains();
    await refreshAddresses();
    void refreshBalances();
  }

  // User-activity hook to defer the auto-lock timer.
  function ping() {
    if (get(state).unlocked) armAutoLock();
  }

  return {
    subscribe: state.subscribe,
    refreshStatus,
    loadChains,
    refreshAddresses,
    refreshBalances,
    unlock,
    lock,
    createNew,
    importExisting,
    ping
  };
}

export const wallet = createWallet();

/** Convenience derived store: the full active chain list. */
export const chains = derived(wallet, ($w) => $w.chains);
