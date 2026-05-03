/**
 * Typed wrapper around `@tauri-apps/api`'s `invoke` for every Rust command
 * exposed by the desktop shell.
 *
 * Keeping all IPC calls in one module makes it trivial to migrate to
 * specta-generated bindings later without touching call sites.
 */

import { invoke } from '@tauri-apps/api/core';

export interface ChainSummary {
  id: string;
  display_name: string;
  symbol: string;
  decimals: number;
  family: 'bitcoin' | 'evm' | 'solana' | 'tron';
  enabled_by_default: boolean;
}

export interface TokenSummary {
  id: string;
  symbol: string;
  display_name: string;
  chain_id: string;
  contract: string;
  decimals: number;
  standard: 'erc20' | 'trc20';
  enabled_by_default: boolean;
}

export interface RpcEndpoint {
  chain_id: string;
  default_url: string | null;
  override_url: string | null;
  effective_url: string | null;
}

export interface Asset {
  id: string;
  symbol: string;
  decimals: number;
  logo: string | null;
}

export interface Amount {
  value: string; // u128 serialised as decimal string by serde_json
  asset: Asset;
}

export interface FeeOption {
  level: 'slow' | 'normal' | 'fast' | string;
  estimated_fee: Amount;
  eta_seconds: number;
  raw_hint: string;
}

export interface SendNativeResult {
  txid: string;
  fee: Amount;
}

export interface PricePoint {
  usd: number;
  usd_24h_change: number;
}

export interface WatchAccount {
  chain_id: string;
  address: string;
  label?: string | null;
}

export interface ProfileSummary {
  id: string;
  name: string;
  kind: 'hot' | 'watch_only';
  signing: boolean;
  created_at: string;
  watch_account_count: number;
}

export const api = {
  vaultExists: () => invoke<boolean>('vault_exists'),
  isUnlocked: () => invoke<boolean>('is_unlocked'),

  /** Returns the freshly generated mnemonic phrase — show ONCE during onboarding. */
  createWallet: (password: string, wordCount: 12 | 24, name?: string | null) =>
    invoke<string>('create_wallet', {
      args: { password, word_count: wordCount, name: name ?? null }
    }),

  importWallet: (
    password: string,
    phrase: string,
    passphrase?: string,
    name?: string | null
  ) =>
    invoke<void>('import_wallet', {
      args: { password, phrase, passphrase: passphrase ?? null, name: name ?? null }
    }),

  unlockWallet: (password: string) => invoke<void>('unlock_wallet', { password }),
  lockWallet: () => invoke<void>('lock_wallet'),

  // Profile management
  listProfiles: () => invoke<ProfileSummary[]>('list_profiles'),
  activeProfile: () => invoke<ProfileSummary | null>('active_profile'),
  switchProfile: (id: string) => invoke<void>('switch_profile', { id }),
  renameProfile: (id: string, newName: string) =>
    invoke<void>('rename_profile', { id, newName }),
  deleteProfile: (id: string) => invoke<void>('delete_profile', { id }),
  createWatchOnlyProfile: (name: string, accounts: WatchAccount[]) =>
    invoke<ProfileSummary>('create_watch_only_profile', { args: { name, accounts } }),

  listChains: () => invoke<ChainSummary[]>('list_chains'),
  listTokens: () => invoke<TokenSummary[]>('list_tokens'),

  // Sovereignty: every chain endpoint is user-overridable.
  listRpcEndpoints: () => invoke<RpcEndpoint[]>('list_rpc_endpoints'),
  setRpcEndpoint: (chainId: string, url: string) =>
    invoke<RpcEndpoint>('set_rpc_endpoint', { chainId, url }),
  clearRpcEndpoint: (chainId: string) =>
    invoke<RpcEndpoint>('clear_rpc_endpoint', { chainId }),
  getAddress: (chainId: string) => invoke<string>('get_address', { chainId }),
  getBalance: (chainId: string) => invoke<Amount>('get_balance', { chainId }),
  getFeeOptions: (chainId: string) => invoke<FeeOption[]>('get_fee_options', { chainId }),

  sendNative: (args: {
    chain_id: string;
    to: string;
    amount: string;
    fee_level: string;
  }) => invoke<SendNativeResult>('send_native', { args }),

  sendToken: (args: {
    token_id: string;
    to: string;
    amount: string;
    fee_level: string;
  }) => invoke<SendNativeResult>('send_token', { args }),

  getPrices: (ids: string[]) => invoke<Record<string, PricePoint>>('get_prices', { ids })
};

/** Format a base-unit `Amount` as a decimal string with full precision. */
export function formatAmount(a: Amount): string {
  const value = BigInt(a.value);
  const d = a.asset.decimals;
  if (d === 0) return `${value.toString()} ${a.asset.symbol}`;
  const s = value.toString().padStart(d + 1, '0');
  const i = s.length - d;
  const whole = s.slice(0, i);
  const frac = s.slice(i).replace(/0+$/, '');
  return frac.length > 0 ? `${whole}.${frac} ${a.asset.symbol}` : `${whole} ${a.asset.symbol}`;
}

/** Parse a user-typed decimal string to base units (string form, since `u128`). */
export function parseAmountToBase(input: string, decimals: number): string {
  const trimmed = input.trim();
  if (!trimmed) throw new Error('amount required');
  const [whole, frac = ''] = trimmed.split('.');
  if (!/^\d+$/.test(whole) || !/^\d*$/.test(frac)) throw new Error('not a number');
  if (frac.length > decimals) throw new Error(`too many decimal places (max ${decimals})`);
  const padded = frac.padEnd(decimals, '0');
  // Strip leading zeros from the result without losing zero itself.
  const combined = (whole + padded).replace(/^0+(\d)/, '$1');
  return combined;
}
