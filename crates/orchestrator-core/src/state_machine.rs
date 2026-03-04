//! Deterministic cart and checkout state machines.

use crate::contract::TransactionStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CartState {
    CartCreated,
    ItemsMutated,
    Repriced,
    Retaxed,
    GeoChecked,
    CheckoutReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CartEvent {
    ItemChanged,
    PricingResolved,
    TaxResolved,
    GeoValidated,
    MarkCheckoutReady,
}

pub fn next_cart_state(current: CartState, event: CartEvent) -> Option<CartState> {
    use CartEvent as E;
    use CartState as S;
    match (current, event) {
        (S::CartCreated, E::ItemChanged) => Some(S::ItemsMutated),
        (S::ItemsMutated, E::ItemChanged) => Some(S::ItemsMutated),
        (S::ItemsMutated, E::PricingResolved) => Some(S::Repriced),
        (S::Repriced, E::TaxResolved) => Some(S::Retaxed),
        (S::Retaxed, E::GeoValidated) => Some(S::GeoChecked),
        (S::CartCreated, E::MarkCheckoutReady)
        | (S::ItemsMutated, E::MarkCheckoutReady)
        | (S::Repriced, E::MarkCheckoutReady)
        | (S::Retaxed, E::MarkCheckoutReady) => Some(S::CheckoutReady),
        (S::GeoChecked, E::MarkCheckoutReady) => Some(S::CheckoutReady),
        (S::CheckoutReady, E::ItemChanged) => Some(S::ItemsMutated),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckoutState {
    Received,
    Validated,
    Priced,
    Taxed,
    GeoChecked,
    PaymentAuthorized,
    Committed,
    ReceiptGenerated,
    Completed,
    Rejected,
    AuthFailed,
    CommitFailed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckoutEvent {
    ValidatePassed,
    PriceResolved,
    TaxResolved,
    GeoValidated,
    PaymentAuthorized,
    Committed,
    ReceiptGenerated,
    Complete,
    Reject,
    PaymentFailed,
    CommitFailed,
    Timeout,
}

pub fn next_checkout_state(current: CheckoutState, event: CheckoutEvent) -> Option<CheckoutState> {
    use CheckoutEvent as E;
    use CheckoutState as S;
    match (current, event) {
        (S::Received, E::ValidatePassed) => Some(S::Validated),
        (S::Validated, E::PriceResolved) => Some(S::Priced),
        (S::Priced, E::TaxResolved) => Some(S::Taxed),
        (S::Taxed, E::GeoValidated) => Some(S::GeoChecked),
        (S::GeoChecked, E::PaymentAuthorized) => Some(S::PaymentAuthorized),
        (S::PaymentAuthorized, E::Committed) => Some(S::Committed),
        (S::Committed, E::ReceiptGenerated) => Some(S::ReceiptGenerated),
        (S::ReceiptGenerated, E::Complete) => Some(S::Completed),
        (S::Received, E::Reject) | (S::Validated, E::Reject) | (S::Priced, E::Reject) => {
            Some(S::Rejected)
        }
        (S::GeoChecked, E::PaymentFailed) => Some(S::AuthFailed),
        (S::PaymentAuthorized, E::CommitFailed) => Some(S::CommitFailed),
        (_, E::Timeout) => Some(S::TimedOut),
        _ => None,
    }
}

pub fn terminal_status(state: CheckoutState) -> Option<TransactionStatus> {
    match state {
        CheckoutState::Completed => Some(TransactionStatus::Completed),
        CheckoutState::Rejected => Some(TransactionStatus::Rejected),
        CheckoutState::AuthFailed => Some(TransactionStatus::AuthFailed),
        CheckoutState::CommitFailed => Some(TransactionStatus::CommitFailed),
        CheckoutState::TimedOut => Some(TransactionStatus::TimedOut),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cart_happy_path_transitions_are_deterministic() {
        let mut state = CartState::CartCreated;
        state = next_cart_state(state, CartEvent::ItemChanged).expect("item changed");
        state = next_cart_state(state, CartEvent::PricingResolved).expect("priced");
        state = next_cart_state(state, CartEvent::TaxResolved).expect("taxed");
        state = next_cart_state(state, CartEvent::GeoValidated).expect("geo");
        state = next_cart_state(state, CartEvent::MarkCheckoutReady).expect("ready");
        assert_eq!(state, CartState::CheckoutReady);
    }

    #[test]
    fn checkout_terminal_status_maps_correctly() {
        assert_eq!(
            terminal_status(CheckoutState::Completed),
            Some(TransactionStatus::Completed)
        );
        assert_eq!(
            terminal_status(CheckoutState::AuthFailed),
            Some(TransactionStatus::AuthFailed)
        );
        assert_eq!(terminal_status(CheckoutState::Validated), None);
    }
}
