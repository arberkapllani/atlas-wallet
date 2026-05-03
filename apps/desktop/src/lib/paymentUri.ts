/** Parse the contents of a scanned QR into an address (and optional amount).
 *
 * Supports the common cases:
 *   - bare address: `bc1q…`, `0x…`, `T…`, `So1…`
 *   - BIP-21:       `bitcoin:bc1q…?amount=0.001`
 *   - EIP-681:      `ethereum:0xABC…?value=1e18`
 *                   `ethereum:0xABC…@1?value=1e18`
 *   - Tron:         `tron:T…?amount=12.5`
 *   - Solana:       `solana:So1…?amount=1.25`
 *
 * The `amount` field is **always returned as a decimal string in the chain's
 * native units** (e.g. BTC, ETH, SOL, TRX), never base units. EIP-681 `value`
 * is in wei and we don't have access to decimals here — we surface it raw
 * (the user can override before sending).
 */
export interface ParsedPaymentUri {
  address: string;
  amount?: string;
  /** `bitcoin` / `ethereum` / `tron` / `solana` / `null` when no scheme. */
  scheme: string | null;
  /** EIP-681 chain id selector, when present (e.g. `1`, `137`). */
  chainHint?: string;
}

const SCHEMES = ['bitcoin', 'ethereum', 'tron', 'solana'];

export function parsePaymentUri(input: string): ParsedPaymentUri {
  const raw = input.trim();
  if (!raw) return { address: '', scheme: null };

  const colon = raw.indexOf(':');
  const head = colon > 0 ? raw.slice(0, colon).toLowerCase() : '';
  if (!SCHEMES.includes(head)) {
    return { address: raw, scheme: null };
  }

  const rest = raw.slice(colon + 1);
  const [target, query = ''] = rest.split('?', 2);
  // Strip optional EIP-681 chain selector (`@137`).
  let address = target;
  let chainHint: string | undefined;
  const at = target.indexOf('@');
  if (at >= 0) {
    address = target.slice(0, at);
    chainHint = target.slice(at + 1);
  }
  // Strip optional EIP-681 function call (`/transfer`) — not supported.
  const slash = address.indexOf('/');
  if (slash >= 0) address = address.slice(0, slash);

  let amount: string | undefined;
  if (query) {
    const params = new URLSearchParams(query);
    amount = params.get('amount') ?? params.get('value') ?? undefined;
    if (amount) amount = amount.trim();
  }

  return { address, amount, scheme: head, chainHint };
}
