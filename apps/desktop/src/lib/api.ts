/**
 * Typed wrapper around `@tauri-apps/api`'s `invoke` for every Rust command
 * exposed by the desktop shell.
 *
 * Keeping all IPC calls in one module makes it trivial to migrate to
 * specta-generated bindings later without touching call sites.
 */

import { invoke } from '@tauri-apps/api/core';

// Phase-9 wired crates: re-export the specta-generated types from bindings.ts
// rather than re-declaring them, so the wire format can never drift between
// the Rust types and the front-end consumers.
export type {
  Eip1559Suggestion,
  Eip1559Suggestions,
  GasCost,
  Holding,
  DiversificationReport,
  DiversificationBand,
  PositionWeight,
  SwapSettings,
  SlippageBand,
  TxNote,
  Chain as TxNotesChain,
  Contact,
  ContactAddress,
  ContactChain,
  EventRecord,
  EventLevel,
  EventCategory,
  SpendPolicy,
  SpendState,
  LimitEvaluation,
  LimitDecision,
  ReasonCode,
  BlocklistEntry,
  BlocklistCategory,
  BlocklistVerdict,
  MimicMatch,
  PhishingReport,
  PhishingVerdict,
  PhishingConfig,
  Approval,
  ApprovalKind,
  ApprovalRisk,
  ApprovalSummary,
  ApprovalConfig,
  RiskLevel,
  DecodedCall,
  Eip712Report,
  Eip712Domain,
  Eip712Category,
  SignatureRisk,
  Trade,
  TradeKind,
  AccountingMethod,
  RealizedEvent,
  Position,
  PortfolioReport,
  PaymentIntent,
  BitcoinPayment,
  EthereumPayment,
  DappAssessment,
  DappEntry,
  DappRisk,
  DappCategory,
  GalleryView,
  GalleryFilter,
  CollectionGroup,
  OwnedNft,
  UserOperation,
  UserOpHashes,
  Share,
  WcUri,
  BiometricStatus,
  TorMode,
  TorStatus,
  TorConfig,
  ProxyDecision,
  Origin,
  UtxoRef,
  UtxoLabel,
  LabeledUtxo,
  LabeledUtxoEntry,
  MixWarning,
  SelectionStrategy,
  PrivacyBucket,
  Selection
} from './bindings';
import type {
  Eip1559Suggestion,
  Eip1559Suggestions,
  GasCost,
  Holding,
  DiversificationReport,
  SwapSettings,
  SlippageBand,
  TxNote,
  Chain as TxNotesChain,
  Contact,
  ContactAddress,
  ContactChain,
  EventRecord,
  EventLevel,
  EventCategory,
  SpendPolicy,
  SpendState,
  LimitEvaluation,
  BlocklistEntry,
  BlocklistVerdict,
  MimicMatch,
  PhishingReport,
  PhishingVerdict,
  PhishingConfig,
  Approval,
  ApprovalKind,
  ApprovalRisk,
  ApprovalSummary,
  ApprovalConfig,
  RiskLevel,
  DecodedCall,
  Eip712Report,
  Eip712Domain,
  Eip712Category,
  SignatureRisk,
  Trade,
  TradeKind,
  AccountingMethod,
  RealizedEvent,
  Position,
  PortfolioReport,
  PaymentIntent,
  BitcoinPayment,
  EthereumPayment,
  DappAssessment,
  DappEntry,
  DappRisk,
  DappCategory,
  GalleryView,
  GalleryFilter,
  CollectionGroup,
  OwnedNft,
  UserOperation,
  UserOpHashes,
  Share,
  WcUri,
  BiometricStatus,
  TorMode,
  TorStatus,
  TorConfig,
  ProxyDecision,
  Origin,
  UtxoRef,
  UtxoLabel,
  LabeledUtxo,
  LabeledUtxoEntry,
  MixWarning,
  SelectionStrategy,
  PrivacyBucket,
  Selection
} from './bindings';

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
  price: number;
  change_24h: number;
}

export type FiatCurrency = 'usd' | 'eur' | 'gbp';

export type NetworkStatus = 'synced' | 'lagging' | 'offline';

export interface NetworkHealth {
  chain_id: string;
  endpoint: string;
  is_user_override: boolean;
  samples: number;
  successes: number;
  latency_p50_ms: number | null;
  latency_p95_ms: number | null;
  head_height: number | null;
  status: NetworkStatus;
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

export interface ExchangeSettings {
  api_key_set: boolean;
  base_url: string;
}

export interface ExchangeQuote {
  from_token: string;
  to_token: string;
  from_amount: string;
  to_amount: string;
  estimated_gas: number;
  protocols: string[];
}

export interface ExchangeSwapResult {
  txid: string;
  approve_txid: string | null;
  to_amount: string;
}

export interface SwapFeeConfig {
  fee_bps: number;
  thorchain_affiliate: string | null;
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

  getPrices: (ids: string[], currency?: FiatCurrency) =>
    invoke<Record<string, PricePoint>>('get_prices', { ids, currency }),
  getFiatCurrency: () => invoke<FiatCurrency>('get_fiat_currency'),
  setFiatCurrency: (currency: FiatCurrency) =>
    invoke<FiatCurrency>('set_fiat_currency', { currency }),

  /** Auto-lock timeout in minutes. `0` means disabled. */
  getAutoLockMinutes: () => invoke<number>('get_auto_lock_minutes'),
  /** Persist a new auto-lock timeout. `0` disables. Backend clamps to ≤1440. */
  setAutoLockMinutes: (minutes: number) =>
    invoke<number>('set_auto_lock_minutes', { minutes }),

  networkHealth: (chainId: string) =>
    invoke<NetworkHealth>('network_health', { chainId }),
  networkHealthAll: () => invoke<NetworkHealth[]>('network_health_all'),

  getExchangeSettings: () =>
    invoke<ExchangeSettings>('get_exchange_settings'),
  setExchangeSettings: (args: { api_key: string | null; base_url: string | null }) =>
    invoke<ExchangeSettings>('set_exchange_settings', { args }),
  exchangeQuote: (args: { chain_id: number; src: string; dst: string; amount: string }) =>
    invoke<ExchangeQuote>('exchange_quote', args),
  exchangeSwap: (args: {
    chain_id: number;
    src: string;
    dst: string;
    amount: string;
    slippage_bps: number;
    fee_level: string;
  }) => invoke<ExchangeSwapResult>('exchange_swap', { args }),

  // Atlas swap fee + THORChain affiliate.
  getSwapFeeConfig: () => invoke<SwapFeeConfig>('get_swap_fee_config'),
  setSwapFeeBps: (bps: number) =>
    invoke<number>('set_swap_fee_bps', { bps }),
  setThorchainAffiliate: (name: string | null) =>
    invoke<string | null>('set_thorchain_affiliate', { name }),

  // Flashbots Protect (private mempool for Ethereum mainnet).
  flashbotsProtectEnabled: () => invoke<boolean>('flashbots_protect_enabled'),
  setFlashbotsProtect: (enabled: boolean) =>
    invoke<boolean>('set_flashbots_protect', { enabled }),

  // ---- Phase-9 wired crates ----------------------------------------

  // atlas-fees: EVM EIP-1559 + UTXO sat/vB.
  feesEip1559Suggest: (baseFeePerGas: string, recentPriorityFees: string[]) =>
    invoke<Eip1559Suggestions>('fees_eip1559_suggest', {
      baseFeePerGas,
      recentPriorityFees
    }),
  feesBumpForReplacement: (suggestion: Eip1559Suggestion) =>
    invoke<Eip1559Suggestion>('fees_bump_for_replacement', { suggestion }),
  feesUtxoSats: (satPerVbyte: number, vsize: number) =>
    invoke<number>('fees_utxo_sats', { satPerVbyte, vsize }),

  // atlas-ens.
  ensLooksLikeEns: (name: string) =>
    invoke<boolean>('ens_looks_like_ens', { name }),
  ensNormalise: (name: string) => invoke<string>('ens_normalise', { name }),
  ensNamehash: (name: string) => invoke<string>('ens_namehash', { name }),

  // atlas-gascost. `effectiveGasPriceWei` is a u128 decimal string.
  gascostEstimate: (
    gasUsed: number,
    effectiveGasPriceWei: string,
    nativePriceUsdMicro: number
  ) =>
    invoke<GasCost>('gascost_estimate', {
      gasUsed,
      effectiveGasPriceWei,
      nativePriceUsdMicro
    }),
  gascostFormatEth: (wei: string, decimals: number) =>
    invoke<string>('gascost_format_eth', { wei, decimals }),

  // atlas-diversification.
  diversificationAnalyse: (holdings: Holding[]) =>
    invoke<DiversificationReport>('diversification_analyse', { holdings }),

  // atlas-slippage. u128 amounts are decimal strings.
  slippageValidate: (slippageBps: number, deadlineSecs: number) =>
    invoke<SwapSettings>('slippage_validate', { slippageBps, deadlineSecs }),
  slippageBand: (slippageBps: number) =>
    invoke<SlippageBand>('slippage_band', { slippageBps }),
  slippageMinOut: (amountOutQuote: string, slippageBps: number) =>
    invoke<string>('slippage_min_out', { amountOutQuote, slippageBps }),
  slippageMaxIn: (amountInQuote: string, slippageBps: number) =>
    invoke<string>('slippage_max_in', { amountInQuote, slippageBps }),
  slippageDeadlineUnix: (nowUnix: number, deadlineSecs: number) =>
    invoke<number>('slippage_deadline_unix', { nowUnix, deadlineSecs }),

  // atlas-fmt: presentation helpers backed by the same crate the host uses.
  fmtCurrency: (
    amount: number,
    code: string,
    localeTag: string,
    decimals: number
  ) =>
    invoke<string>('fmt_currency', { amount, code, localeTag, decimals }),
  fmtCompact: (value: number) => invoke<string>('fmt_compact', { value }),
  fmtTokenAmount: (
    baseUnits: string,
    decimals: number,
    maxSignificant: number
  ) =>
    invoke<string>('fmt_token_amount', {
      baseUnits,
      decimals,
      maxSignificant
    }),
  fmtTruncateAddress: (addr: string) =>
    invoke<string>('fmt_truncate_address', { addr }),

  // atlas-txnotes (per-tx notes + tags, persisted to data_dir/txnotes.json).
  txnotesList: () => invoke<TxNote[]>('txnotes_list'),
  txnotesGet: (chain: TxNotesChain, txid: string) =>
    invoke<TxNote | null>('txnotes_get', { chain, txid }),
  txnotesUpsert: (
    chain: TxNotesChain,
    txid: string,
    note: string,
    tags: string[]
  ) => invoke<void>('txnotes_upsert', { chain, txid, note, tags }),
  txnotesRemove: (chain: TxNotesChain, txid: string) =>
    invoke<boolean>('txnotes_remove', { chain, txid }),
  txnotesListByTag: (tag: string) =>
    invoke<TxNote[]>('txnotes_list_by_tag', { tag }),
  txnotesAllTags: () => invoke<string[]>('txnotes_all_tags'),

  // atlas-contacts (address book, persisted to data_dir/contacts.json).
  contactsList: () => invoke<Contact[]>('contacts_list'),
  contactsGet: (id: string) =>
    invoke<Contact | null>('contacts_get', { id }),
  contactsAdd: (name: string, note: string, addresses: ContactAddress[]) =>
    invoke<string>('contacts_add', { name, note, addresses }),
  contactsUpdate: (
    id: string,
    name: string,
    note: string,
    addresses: ContactAddress[]
  ) => invoke<void>('contacts_update', { id, name, note, addresses }),
  contactsRemove: (id: string) =>
    invoke<boolean>('contacts_remove', { id }),
  contactsFindByAddress: (chain: ContactChain, address: string) =>
    invoke<Contact | null>('contacts_find_by_address', { chain, address }),
  contactsSearch: (query: string) =>
    invoke<Contact[]>('contacts_search', { query }),

  // atlas-eventlog (in-memory ring, max 500 records).
  eventsRecord: (
    timestampUnixMs: number,
    level: EventLevel,
    category: EventCategory,
    message: string
  ) =>
    invoke<void>('events_record', {
      timestampUnixMs,
      level,
      category,
      message
    }),
  eventsRecent: (limit: number) =>
    invoke<EventRecord[]>('events_recent', { limit }),
  eventsFilter: (
    minLevel: EventLevel,
    category: EventCategory | null,
    limit: number
  ) =>
    invoke<EventRecord[]>('events_filter', { minLevel, category, limit }),
  eventsClear: () => invoke<void>('events_clear'),
  eventsExportRedacted: () =>
    invoke<EventRecord[]>('events_export_redacted'),

  // atlas-spendlimits (daily/per-tx USD caps, persisted to data_dir/spend.json).
  spendGetPolicy: () => invoke<SpendPolicy>('spend_get_policy'),
  spendSetPolicy: (policy: SpendPolicy) =>
    invoke<void>('spend_set_policy', { policy }),
  spendGetState: () => invoke<SpendState>('spend_get_state'),
  spendEvaluate: (nowUnix: number, attemptUsd: number) =>
    invoke<LimitEvaluation>('spend_evaluate', { nowUnix, attemptUsd }),
  spendCommit: (nextState: SpendState) =>
    invoke<void>('spend_commit', { nextState }),
  spendReset: () => invoke<void>('spend_reset'),

  // atlas-blocklist (malicious-address registry, persisted to data_dir/blocklist.json).
  blocklistCheck: (address: string) =>
    invoke<BlocklistVerdict>('blocklist_check', { address }),
  blocklistList: () => invoke<BlocklistEntry[]>('blocklist_list'),
  blocklistAdd: (entry: BlocklistEntry) =>
    invoke<string>('blocklist_add', { entry }),
  blocklistRemove: (address: string) =>
    invoke<boolean>('blocklist_remove', { address }),
  blocklistImportJson: (json: string) =>
    invoke<number>('blocklist_import_json', { json }),

  // atlas-address-poisoning live recipient check (consumes the contact book).
  poisoningCheckCandidate: (candidate: string) =>
    invoke<MimicMatch[]>('poisoning_check_candidate', { candidate }),

  // atlas-phishing dApp/origin URL safety check.
  phishingAnalyzeDefault: (origin: string) =>
    invoke<PhishingReport>('phishing_analyze_default', { origin }),
  phishingAnalyze: (origin: string, config: PhishingConfig) =>
    invoke<PhishingReport>('phishing_analyze', { origin, config }),

  // atlas-approvals risk dashboard.
  approvalsAnalyze: (approvals: Approval[], config: ApprovalConfig, now: number) =>
    invoke<ApprovalRisk[]>('approvals_analyze', { approvals, config, now }),
  approvalsSummarise: (rows: ApprovalRisk[]) =>
    invoke<ApprovalSummary[]>('approvals_summarise', { rows }),

  // atlas-calldata EVM tx decoder.
  calldataDecode: (data: string) => invoke<DecodedCall>('calldata_decode', { data }),

  // atlas-eip712 typed-data inspector.
  eip712Classify: (json: string) => invoke<Eip712Report>('eip712_classify', { json }),

  // atlas-pnl + persisted trade history.
  pnlCompute: (
    trades: Trade[],
    prices: Record<string, number>,
    method: AccountingMethod
  ) => invoke<PortfolioReport>('pnl_compute', { trades, prices, method }),
  tradesList: () => invoke<Trade[]>('trades_list'),
  tradesAdd: (trade: Trade) => invoke<number>('trades_add', { trade }),
  tradesRemove: (index: number) => invoke<boolean>('trades_remove', { index }),
  tradesClear: () => invoke<void>('trades_clear'),
  tradesImportJson: (json: string) => invoke<number>('trades_import_json', { json }),
  tradesComputePnl: (prices: Record<string, number>, method: AccountingMethod) =>
    invoke<PortfolioReport>('trades_compute_pnl', { prices, method }),

  // atlas-payuri payment URI parser (BIP-21 / EIP-681).
  payuriParse: (input: string) => invoke<PaymentIntent>('payuri_parse', { input }),

  // atlas-dapp-registry origin assessor.
  dappAssessOrigin: (url: string) => invoke<DappAssessment>('dapp_assess_origin', { url }),
  dappListCurated: () => invoke<DappEntry[]>('dapp_list_curated'),

  // atlas-nft-gallery aggregation over owned NFTs.
  nftGalleryView: (items: OwnedNft[], filter: GalleryFilter) =>
    invoke<GalleryView>('nft_gallery_view', { items, filter }),

  // atlas-aa-erc4337 user-op hash + execute() calldata encoder.
  aaUserOpHash: (entryPoint: string, chainId: number, op: UserOperation) =>
    invoke<UserOpHashes>('aa_user_op_hash', { entryPoint, chainId, op }),
  aaEncodeExecuteCalldata: (target: string, value: string, data: string) =>
    invoke<string>('aa_encode_execute_calldata', { target, value, data }),

  // atlas-shamir SLIP-39-style secret sharing.
  shamirSplit: (secretHex: string, threshold: number, total: number) =>
    invoke<Share[]>('shamir_split', { secretHex, threshold, total }),
  shamirCombine: (shares: Share[]) => invoke<string>('shamir_combine', { shares }),

  // atlas-walletconnect v2 pairing-URI parser.
  wcParseUri: (uri: string) => invoke<WcUri>('wc_parse_uri', { uri }),
  wcBuildUri: (uri: WcUri) => invoke<string>('wc_build_uri', { uri }),

  // atlas-biometric status + preference.
  biometricStatusReport: () => invoke<BiometricStatus>('biometric_status'),

  // atlas-tor proxy posture + kill-switch.
  torStatus: () => invoke<TorStatus>('tor_status'),
  torGetMode: () => invoke<TorMode>('tor_get_mode'),
  torSetMode: (mode: TorMode) => invoke<TorMode>('tor_set_mode', { mode }),
  torGetConfig: () => invoke<TorConfig>('tor_get_config'),
  torSetConfig: (config: TorConfig) => invoke<TorConfig>('tor_set_config', { config }),
  torStart: () => invoke<TorStatus>('tor_start'),
  torStop: () => invoke<null>('tor_stop'),
  torNewCircuit: () => invoke<null>('tor_new_circuit'),
  torEnforceDecision: () => invoke<ProxyDecision>('tor_enforce_decision'),

  // atlas-coincontrol UTXO labelling + privacy-aware selection.
  coincontrolLabelList: () => invoke<LabeledUtxoEntry[]>('coincontrol_label_list'),
  coincontrolLabelUpsert: (utxo: UtxoRef, label: UtxoLabel) =>
    invoke<null>('coincontrol_label_upsert', { utxo, label }),
  coincontrolLabelRemove: (utxo: UtxoRef) =>
    invoke<boolean>('coincontrol_label_remove', { utxo }),
  coincontrolDetectMix: (selected: LabeledUtxo[]) =>
    invoke<MixWarning[]>('coincontrol_detect_mix', { selected }),
  coincontrolSuggestSelection: (
    target: number,
    available: LabeledUtxo[],
    strategy: SelectionStrategy
  ) =>
    invoke<Selection>('coincontrol_suggest_selection', { target, available, strategy })
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

/**
 * Render a thrown value as a user-friendly string.
 *
 * Tauri commands that fail with our `CmdError` produce a JSON-serialised
 * `{ kind, message }` object on the JS side; plain `String(e)` on that
 * object yields the useless "[object Object]". This helper extracts the
 * message regardless of whether the error is a string, an `Error`, or
 * a serialised tagged enum.
 */
export function errorMessage(e: unknown): string {
  if (e == null) return 'unknown error';
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  if (typeof e === 'object') {
    const obj = e as { message?: unknown; kind?: unknown };
    if (typeof obj.message === 'string' && obj.message.length > 0) {
      return typeof obj.kind === 'string' ? `${obj.kind}: ${obj.message}` : obj.message;
    }
    if (typeof obj.kind === 'string') return obj.kind;
    try {
      return JSON.stringify(e);
    } catch {
      return String(e);
    }
  }
  return String(e);
}

