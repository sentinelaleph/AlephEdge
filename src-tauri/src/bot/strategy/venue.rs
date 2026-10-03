//! Where strategy orders go. v1 has ONLY `PaperVenue`: pure bookkeeping of
//! the orders a cycle rests, with the Planned -> Open -> Filled | Canceled
//! lifecycle written to `strategy_orders`. No live venue type exists; the
//! trait and the deterministic client-order-id scheme are here so a later,
//! separately approved adapter plugs in without touching dca.rs / grid.rs.

use std::collections::{BTreeMap, HashSet};

use super::model::{OrderState, OrderType, SimOrder};

pub trait OrderVenue {
    fn place_limit(&mut self, order: SimOrder) -> Result<SimOrder, String>;
    fn place_market(&mut self, order: SimOrder) -> Result<SimOrder, String>;
    fn cancel(&mut self, client_id: &str) -> Result<SimOrder, String>;
    fn query_by_client_id(&self, client_id: &str) -> Option<SimOrder>;
    fn open_orders(&self) -> Vec<SimOrder>;
}

/// Paper order book of one bot.
#[derive(Default, Debug, Clone)]
pub struct PaperVenue {
    book: BTreeMap<String, SimOrder>,
}

impl PaperVenue {
    /// Rebuilds the book from stored open orders (app restart).
    pub fn from_open(orders: Vec<SimOrder>) -> Self {
        Self {
            book: orders
                .into_iter()
                .filter(|o| !o.state.is_terminal())
                .map(|o| (o.client_id.clone(), o))
                .collect(),
        }
    }

    /// Brings the book in line with the cycle's resting orders after a bar:
    /// ids in `filled` become Filled, orders no longer desired are Canceled,
    /// new ones are placed. Returns every order whose row changed.
    pub fn sync(&mut self, desired: Vec<SimOrder>, filled: &HashSet<String>) -> Vec<SimOrder> {
        let mut changed = Vec::new();
        let want: HashSet<String> = desired.iter().map(|o| o.client_id.clone()).collect();
        let open_ids: Vec<String> = self.book.keys().cloned().collect();
        for id in open_ids {
            if filled.contains(&id) {
                if let Some(mut o) = self.book.remove(&id) {
                    o.state = OrderState::Filled;
                    changed.push(o);
                }
            } else if !want.contains(&id) {
                if let Ok(o) = self.cancel(&id) {
                    changed.push(o);
                }
            }
        }
        for o in desired {
            if let Some(cur) = self.query_by_client_id(&o.client_id) {
                // price or qty can move with the average (same id = same order)
                if cur.price != o.price || cur.qty != o.qty {
                    let mut moved = cur;
                    moved.price = o.price;
                    moved.qty = o.qty;
                    self.book.insert(moved.client_id.clone(), moved.clone());
                    changed.push(moved);
                }
                continue;
            }
            if filled.contains(&o.client_id) {
                continue;
            }
            if let Ok(placed) = self.place_limit(o) {
                changed.push(placed);
            }
        }
        changed
    }

}

impl OrderVenue for PaperVenue {
    fn place_limit(&mut self, mut order: SimOrder) -> Result<SimOrder, String> {
        if order.client_id.len() > 36 {
            return Err("clientIdTooLong".into());
        }
        order.state = OrderState::Open;
        self.book.insert(order.client_id.clone(), order.clone());
        Ok(order)
    }

    /// A paper market order fills immediately; it never rests in the book.
    fn place_market(&mut self, mut order: SimOrder) -> Result<SimOrder, String> {
        if order.client_id.len() > 36 {
            return Err("clientIdTooLong".into());
        }
        order.kind = OrderType::Market;
        order.state = OrderState::Filled;
        Ok(order)
    }

    fn cancel(&mut self, client_id: &str) -> Result<SimOrder, String> {
        let mut o = self
            .book
            .remove(client_id)
            .ok_or_else(|| "orderUnknown".to_string())?;
        o.state = OrderState::Canceled;
        Ok(o)
    }

    fn query_by_client_id(&self, client_id: &str) -> Option<SimOrder> {
        self.book.get(client_id).cloned()
    }

    fn open_orders(&self) -> Vec<SimOrder> {
        self.book.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::model::{OrderRole, OrderSide};

    fn order(id: &str, px: f64) -> SimOrder {
        SimOrder {
            client_id: id.into(),
            role: OrderRole::TakeProfit,
            side: OrderSide::Sell,
            kind: OrderType::Limit,
            price: px,
            qty: 1.0,
            active_from_leg: 0,
            state: OrderState::Planned,
        }
    }

    #[test]
    fn lifecycle_planned_open_filled_canceled() {
        let mut v = PaperVenue::default();
        let ch = v.sync(vec![order("a", 1.0), order("b", 2.0)], &HashSet::new());
        assert_eq!(ch.len(), 2);
        assert!(ch.iter().all(|o| o.state == OrderState::Open));
        let filled: HashSet<String> = ["a".to_string()].into();
        let ch = v.sync(vec![order("c", 3.0)], &filled);
        let state = |id: &str| ch.iter().find(|o| o.client_id == id).unwrap().state;
        assert_eq!(state("a"), OrderState::Filled);
        assert_eq!(state("b"), OrderState::Canceled);
        assert_eq!(state("c"), OrderState::Open);
        assert_eq!(v.open_orders().len(), 1);
        assert!(v.query_by_client_id("c").is_some());
        let m = v.place_market(order("m", 1.0)).unwrap();
        assert_eq!(m.state, OrderState::Filled);
        assert_eq!(v.sync(Vec::new(), &HashSet::new()).len(), 1);
        assert!(v.open_orders().is_empty());
    }

    #[test]
    fn client_ids_respect_the_exchange_limit() {
        let mut v = PaperVenue::default();
        assert!(v.place_limit(order(&"x".repeat(37), 1.0)).is_err());
        let id = crate::bot::strategy::cycle::client_prefix("sb_abcdefghijkl", 123_456);
        assert!(id.len() + "tp25".len() <= 36, "{id}");
        assert!(id.starts_with("aeabcdefgh-"));
    }
}
