//! Portfolio cost-basis & profit-and-loss tracker.
//!
//! Given a chronological list of trades (Buy / Sell) and a snapshot
//! of current prices, compute realized P&L (one row per Sell) and
//! unrealized P&L (one row per asset still held). Three accounting
//! methods are supported: FIFO, LIFO, and AverageCost.
//!
//! All quantities and prices are `f64` USD — this is for the
//! portfolio display and is documented as not authoritative for
//! tax. No I/O.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum PnlError {
    #[error("invalid quantity: {0}")]
    InvalidQuantity(String),
    #[error("oversold {asset}: tried to sell {qty} but only {held} held")]
    Oversold { asset: String, qty: f64, held: f64 },
}

/// What the trade is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TradeKind {
    Buy,
    Sell,
}

/// One trade entry.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Trade {
    pub kind: TradeKind,
    pub asset: String,
    pub quantity: f64,
    /// Price per unit in USD at the time of the trade.
    pub unit_price_usd: f64,
    /// Unix seconds.
    pub ts: i64,
}

/// Accounting method for matching sells against buys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AccountingMethod {
    Fifo,
    Lifo,
    AverageCost,
}

/// Output: one row per Sell.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RealizedEvent {
    pub asset: String,
    pub ts: i64,
    pub quantity: f64,
    pub proceeds_usd: f64,
    pub cost_basis_usd: f64,
    pub gain_usd: f64,
}

/// Output: one row per asset still held.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Position {
    pub asset: String,
    pub quantity_held: f64,
    pub cost_basis_usd: f64,
    pub market_value_usd: f64,
    pub unrealized_pnl_usd: f64,
}

/// Aggregate report.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PortfolioReport {
    pub positions: Vec<Position>,
    pub realized: Vec<RealizedEvent>,
    pub total_realized_usd: f64,
    pub total_unrealized_usd: f64,
}

/// Input: known prices keyed by asset symbol.
pub type PriceMap = HashMap<String, f64>;

/// Compute the report. Trades are processed in `ts` order
/// regardless of input order to avoid surprising results.
pub fn compute(
    trades: &[Trade],
    prices: &PriceMap,
    method: AccountingMethod,
) -> Result<PortfolioReport, PnlError> {
    // Validate inputs.
    for t in trades {
        if !t.quantity.is_finite() || t.quantity <= 0.0 {
            return Err(PnlError::InvalidQuantity(format!(
                "{}: {}",
                t.asset, t.quantity
            )));
        }
        if !t.unit_price_usd.is_finite() || t.unit_price_usd < 0.0 {
            return Err(PnlError::InvalidQuantity(format!(
                "{} unit_price: {}",
                t.asset, t.unit_price_usd
            )));
        }
    }

    let mut sorted: Vec<Trade> = trades.to_vec();
    sorted.sort_by_key(|t| t.ts);

    let mut realized: Vec<RealizedEvent> = Vec::new();

    // Per-asset state. We keep two parallel layouts to handle the
    // three methods uniformly:
    //   - FIFO/LIFO: a queue of (qty, unit_cost, _ts) per asset.
    //   - AverageCost: a (qty, total_cost) running sum per asset.
    let mut lots: BTreeMap<String, VecDeque<(f64, f64, i64)>> = BTreeMap::new();
    let mut avg: BTreeMap<String, (f64, f64)> = BTreeMap::new();

    for t in &sorted {
        match t.kind {
            TradeKind::Buy => match method {
                AccountingMethod::Fifo | AccountingMethod::Lifo => {
                    lots.entry(t.asset.clone()).or_default().push_back((
                        t.quantity,
                        t.unit_price_usd,
                        t.ts,
                    ));
                }
                AccountingMethod::AverageCost => {
                    let e = avg.entry(t.asset.clone()).or_insert((0.0, 0.0));
                    e.0 += t.quantity;
                    e.1 += t.quantity * t.unit_price_usd;
                }
            },
            TradeKind::Sell => match method {
                AccountingMethod::Fifo | AccountingMethod::Lifo => {
                    let queue = lots.entry(t.asset.clone()).or_default();
                    let mut remaining = t.quantity;
                    let mut basis = 0.0;
                    while remaining > 0.0 {
                        let head = match method {
                            AccountingMethod::Fifo => queue.front_mut(),
                            AccountingMethod::Lifo => queue.back_mut(),
                            _ => unreachable!(),
                        };
                        let Some(lot) = head else {
                            return Err(PnlError::Oversold {
                                asset: t.asset.clone(),
                                qty: t.quantity,
                                held: t.quantity - remaining,
                            });
                        };
                        let take = remaining.min(lot.0);
                        basis += take * lot.1;
                        lot.0 -= take;
                        remaining -= take;
                        if lot.0 <= 0.0 {
                            match method {
                                AccountingMethod::Fifo => {
                                    queue.pop_front();
                                }
                                AccountingMethod::Lifo => {
                                    queue.pop_back();
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    let proceeds = t.quantity * t.unit_price_usd;
                    realized.push(RealizedEvent {
                        asset: t.asset.clone(),
                        ts: t.ts,
                        quantity: t.quantity,
                        proceeds_usd: proceeds,
                        cost_basis_usd: basis,
                        gain_usd: proceeds - basis,
                    });
                }
                AccountingMethod::AverageCost => {
                    let e = avg.entry(t.asset.clone()).or_insert((0.0, 0.0));
                    if t.quantity > e.0 + 1e-12 {
                        return Err(PnlError::Oversold {
                            asset: t.asset.clone(),
                            qty: t.quantity,
                            held: e.0,
                        });
                    }
                    let avg_unit = if e.0 > 0.0 { e.1 / e.0 } else { 0.0 };
                    let basis = t.quantity * avg_unit;
                    let proceeds = t.quantity * t.unit_price_usd;
                    realized.push(RealizedEvent {
                        asset: t.asset.clone(),
                        ts: t.ts,
                        quantity: t.quantity,
                        proceeds_usd: proceeds,
                        cost_basis_usd: basis,
                        gain_usd: proceeds - basis,
                    });
                    e.0 -= t.quantity;
                    e.1 -= basis;
                    if e.0 < 1e-12 {
                        e.0 = 0.0;
                        e.1 = 0.0;
                    }
                }
            },
        }
    }

    // Build positions.
    let mut positions: Vec<Position> = Vec::new();
    match method {
        AccountingMethod::Fifo | AccountingMethod::Lifo => {
            for (asset, queue) in &lots {
                let qty: f64 = queue.iter().map(|(q, _, _)| *q).sum();
                if qty <= 0.0 {
                    continue;
                }
                let basis: f64 = queue.iter().map(|(q, p, _)| q * p).sum();
                let price = prices.get(asset).copied().unwrap_or(0.0);
                let mv = qty * price;
                positions.push(Position {
                    asset: asset.clone(),
                    quantity_held: qty,
                    cost_basis_usd: basis,
                    market_value_usd: mv,
                    unrealized_pnl_usd: mv - basis,
                });
            }
        }
        AccountingMethod::AverageCost => {
            for (asset, (qty, total_cost)) in &avg {
                if *qty <= 0.0 {
                    continue;
                }
                let price = prices.get(asset).copied().unwrap_or(0.0);
                let mv = qty * price;
                positions.push(Position {
                    asset: asset.clone(),
                    quantity_held: *qty,
                    cost_basis_usd: *total_cost,
                    market_value_usd: mv,
                    unrealized_pnl_usd: mv - *total_cost,
                });
            }
        }
    }
    positions.sort_by(|a, b| a.asset.cmp(&b.asset));

    let total_realized: f64 = realized.iter().map(|r| r.gain_usd).sum();
    let total_unrealized: f64 = positions.iter().map(|p| p.unrealized_pnl_usd).sum();

    Ok(PortfolioReport {
        positions,
        realized,
        total_realized_usd: total_realized,
        total_unrealized_usd: total_unrealized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buy(asset: &str, qty: f64, p: f64, ts: i64) -> Trade {
        Trade {
            kind: TradeKind::Buy,
            asset: asset.into(),
            quantity: qty,
            unit_price_usd: p,
            ts,
        }
    }
    fn sell(asset: &str, qty: f64, p: f64, ts: i64) -> Trade {
        Trade {
            kind: TradeKind::Sell,
            asset: asset.into(),
            quantity: qty,
            unit_price_usd: p,
            ts,
        }
    }
    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn empty_input_is_empty_report() {
        let r = compute(&[], &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        assert!(r.positions.is_empty());
        assert!(r.realized.is_empty());
    }

    #[test]
    fn single_buy_yields_position_with_unrealized() {
        let trades = vec![buy("BTC", 1.0, 30000.0, 1)];
        let mut prices = PriceMap::new();
        prices.insert("BTC".into(), 50000.0);
        let r = compute(&trades, &prices, AccountingMethod::Fifo).unwrap();
        assert_eq!(r.positions.len(), 1);
        assert!(approx(r.positions[0].unrealized_pnl_usd, 20000.0));
        assert!(approx(r.total_unrealized_usd, 20000.0));
    }

    #[test]
    fn fifo_matches_first_lot_first() {
        let trades = vec![
            buy("BTC", 1.0, 10000.0, 1),
            buy("BTC", 1.0, 30000.0, 2),
            sell("BTC", 1.0, 40000.0, 3),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        assert_eq!(r.realized.len(), 1);
        // Sold the 10k lot, so gain = 40k - 10k = 30k.
        assert!(approx(r.realized[0].gain_usd, 30000.0));
    }

    #[test]
    fn lifo_matches_last_lot_first() {
        let trades = vec![
            buy("BTC", 1.0, 10000.0, 1),
            buy("BTC", 1.0, 30000.0, 2),
            sell("BTC", 1.0, 40000.0, 3),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Lifo).unwrap();
        // Sold the 30k lot, so gain = 40k - 30k = 10k.
        assert!(approx(r.realized[0].gain_usd, 10000.0));
    }

    #[test]
    fn average_cost_uses_running_average() {
        let trades = vec![
            buy("BTC", 1.0, 10000.0, 1),
            buy("BTC", 1.0, 30000.0, 2),
            sell("BTC", 1.0, 40000.0, 3),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::AverageCost).unwrap();
        // Average cost = 20k, so gain = 40k - 20k = 20k.
        assert!(approx(r.realized[0].gain_usd, 20000.0));
    }

    #[test]
    fn fifo_can_split_a_lot_across_a_sell() {
        let trades = vec![
            buy("BTC", 1.0, 10000.0, 1),
            buy("BTC", 1.0, 30000.0, 2),
            sell("BTC", 1.5, 40000.0, 3),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        // Sold all of lot1 (10k basis) + 0.5 of lot2 (15k basis) = 25k basis.
        // Proceeds = 1.5 * 40k = 60k. Gain = 35k.
        assert!(approx(r.realized[0].gain_usd, 35000.0));
        // 0.5 of the 30k lot remains.
        assert_eq!(r.positions.len(), 1);
        assert!(approx(r.positions[0].quantity_held, 0.5));
        assert!(approx(r.positions[0].cost_basis_usd, 15000.0));
    }

    #[test]
    fn oversold_fifo_returns_error() {
        let trades = vec![buy("BTC", 1.0, 10000.0, 1), sell("BTC", 2.0, 20000.0, 2)];
        let err = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap_err();
        assert!(matches!(err, PnlError::Oversold { .. }));
    }

    #[test]
    fn oversold_avg_returns_error() {
        let trades = vec![buy("BTC", 1.0, 10000.0, 1), sell("BTC", 2.0, 20000.0, 2)];
        let err = compute(&trades, &PriceMap::new(), AccountingMethod::AverageCost).unwrap_err();
        assert!(matches!(err, PnlError::Oversold { .. }));
    }

    #[test]
    fn invalid_quantity_rejected() {
        let trades = vec![buy("BTC", 0.0, 10000.0, 1)];
        let err = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap_err();
        assert!(matches!(err, PnlError::InvalidQuantity(_)));
    }

    #[test]
    fn out_of_order_trades_are_sorted_by_ts() {
        let trades = vec![
            sell("BTC", 1.0, 40000.0, 3),
            buy("BTC", 1.0, 30000.0, 2),
            buy("BTC", 1.0, 10000.0, 1),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        // FIFO over chronological order: gain = 40k - 10k = 30k.
        assert!(approx(r.realized[0].gain_usd, 30000.0));
    }

    #[test]
    fn missing_price_uses_zero_market_value() {
        let trades = vec![buy("ETH", 2.0, 1000.0, 1)];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        assert!(approx(r.positions[0].market_value_usd, 0.0));
        assert!(approx(r.positions[0].unrealized_pnl_usd, -2000.0));
    }

    #[test]
    fn multi_asset_report_is_sorted() {
        let trades = vec![
            buy("ZEC", 1.0, 100.0, 1),
            buy("BTC", 1.0, 30000.0, 2),
            buy("ETH", 1.0, 2000.0, 3),
        ];
        let r = compute(&trades, &PriceMap::new(), AccountingMethod::Fifo).unwrap();
        assert_eq!(r.positions.len(), 3);
        assert_eq!(r.positions[0].asset, "BTC");
        assert_eq!(r.positions[1].asset, "ETH");
        assert_eq!(r.positions[2].asset, "ZEC");
    }

    #[test]
    fn totals_aggregate_correctly() {
        let trades = vec![
            buy("BTC", 1.0, 10000.0, 1),
            sell("BTC", 1.0, 15000.0, 2),
            buy("ETH", 1.0, 1000.0, 3),
        ];
        let mut prices = PriceMap::new();
        prices.insert("ETH".into(), 1500.0);
        let r = compute(&trades, &prices, AccountingMethod::Fifo).unwrap();
        assert!(approx(r.total_realized_usd, 5000.0));
        assert!(approx(r.total_unrealized_usd, 500.0));
    }
}
