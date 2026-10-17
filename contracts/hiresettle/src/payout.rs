use soroban_sdk::{contractclient, contractimpl, token, Address, Env, String, Symbol};
use crate::*;

/// Interface a trusted swap-adapter contract must implement (issue #458).
///
/// Before calling `swap`, HireSettle transfers `amount_in` of `from_token` to
/// the adapter. The adapter must deliver the swapped `to_token` amount to
/// `recipient` and return it, or panic — a panic reverts the whole payout.
#[contractclient(name = "SwapAdapterClient")]
pub trait SwapAdapter {
    fn swap(
        env: Env,
        from_token: Address,
        to_token: Address,
        amount_in: i128,
        recipient: Address,
    ) -> i128;
}

#[contractimpl]
impl HireSettleContract {
    // ----------------------------------------------------------
    // ISSUE #458 — RECRUITER PAYOUT TOKEN PREFERENCE
    // ----------------------------------------------------------

    /// Recruiter registers the token they want their net milestone payouts
    /// delivered in, across all of their engagements. Only applied when the
    /// admin has configured a swap adapter.
    pub fn set_recruiter_payout_token(env: Env, recruiter: Address, token: Address) {
        recruiter.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey2::RecruiterPayoutToken(recruiter.clone()), &token);
        env.storage().persistent().extend_ttl(
            &DataKey2::RecruiterPayoutToken(recruiter.clone()),
            100_000,
            6_300_000,
        );
        env.events().publish(
            (Symbol::new(&env, "payout_token_set"), recruiter),
            token,
        );
    }

    /// Recruiter clears their payout token preference; payouts revert to the
    /// engagement's escrow token.
    pub fn clear_recruiter_payout_token(env: Env, recruiter: Address) {
        recruiter.require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey2::RecruiterPayoutToken(recruiter.clone()));
        env.events()
            .publish((Symbol::new(&env, "payout_token_cleared"), recruiter), ());
    }

    /// Return the recruiter's preferred payout token, if any.
    pub fn get_recruiter_payout_token(env: Env, recruiter: Address) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey2::RecruiterPayoutToken(recruiter))
    }

    /// Admin registers the trusted swap-adapter contract used to convert
    /// recruiter payouts into their preferred token. Replaces any existing one.
    pub fn set_swap_adapter(env: Env, admin: Address, adapter: Address) {
        Self::assert_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Config(ConfigKey::SwapAdapter), &adapter);
        env.events()
            .publish((Symbol::new(&env, "swap_adapter_set"),), adapter);
    }

    /// Admin removes the swap adapter; payouts fall back to the escrow token.
    pub fn clear_swap_adapter(env: Env, admin: Address) {
        Self::assert_admin(&env, &admin);
        env.storage()
            .instance()
            .remove(&DataKey::Config(ConfigKey::SwapAdapter));
        env.events()
            .publish((Symbol::new(&env, "swap_adapter_cleared"),), ());
    }

    /// Return the configured swap adapter, if any.
    pub fn get_swap_adapter(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Config(ConfigKey::SwapAdapter))
    }

    /// Pay `amount` of the escrow token owed to `recipient`, swapping it into
    /// the recipient's preferred payout token first when both a preference and
    /// a swap adapter are configured. An adapter failure panics, reverting the
    /// whole call (fee transfers included) rather than paying out partially.
    pub(crate) fn pay_recruiter_share(
        env: &Env,
        token_client: &token::Client,
        recipient: &Address,
        amount: i128,
    ) {
        let preferred: Option<Address> = env
            .storage()
            .persistent()
            .get(&DataKey2::RecruiterPayoutToken(recipient.clone()));
        let adapter: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Config(ConfigKey::SwapAdapter));

        match (preferred, adapter) {
            (Some(to_token), Some(adapter))
                if amount > 0 && to_token != token_client.address =>
            {
                token_client.transfer(&env.current_contract_address(), &adapter, &amount);
                let amount_out = SwapAdapterClient::new(env, &adapter).swap(
                    &token_client.address,
                    &to_token,
                    &amount,
                    recipient,
                );
                env.events().publish(
                    (Symbol::new(env, "payout_swapped"), recipient.clone()),
                    (token_client.address.clone(), to_token, amount, amount_out),
                );
            }
            _ => {
                token_client.transfer(&env.current_contract_address(), recipient, &amount);
            }
        }
    }

    // ----------------------------------------------------------
    // ISSUE #547 — PULL-BASED PAYOUTS
    // ----------------------------------------------------------

    /// Withdraw everything credited to `recruiter` in `token` by
    /// `pull_payouts` engagements, and return the amount transferred. Works
    /// for primary recruiters and co-recruiters alike, and covers milestone
    /// payouts, streamed-payout claims and bond refunds across every
    /// engagement. Returns `0` without transferring when nothing is owed.
    ///
    /// Credited balances are always in the engagement's escrow token; the
    /// payout token preference (issue #458) is not applied.
    ///
    /// # Panics
    /// - `"ContractPaused"` — the contract is paused.
    ///
    /// # Events
    /// `("payouts_claimed", recruiter)` with `(token, amount)` when `amount > 0`.
    pub fn claim_payouts(env: Env, recruiter: Address, token: Address) -> i128 {
        Self::assert_not_paused(&env);
        recruiter.require_auth();

        let key = DataKey2::RecruiterClaimable(recruiter.clone(), token.clone());
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount <= 0 {
            return 0;
        }
        env.storage().persistent().remove(&key);
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &recruiter,
            &amount,
        );
        env.events().publish(
            (Symbol::new(&env, "payouts_claimed"), recruiter),
            (token, amount),
        );
        amount
    }

    /// Amount of `token` credited to `recruiter` and not yet claimed.
    pub fn get_claimable_payout(env: Env, recruiter: Address, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey2::RecruiterClaimable(recruiter, token))
            .unwrap_or(0)
    }

    /// Whether an engagement was created with `pull_payouts`.
    pub fn is_pull_payouts_enabled(env: Env, engagement_id: String) -> bool {
        Self::is_pull_payouts_internal(&env, &engagement_id)
    }

    pub(crate) fn is_pull_payouts_internal(env: &Env, engagement_id: &String) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey2::PullPayouts(engagement_id.clone()))
            .unwrap_or(false)
    }

    pub(crate) fn mark_pull_payouts(env: &Env, engagement_id: &String) {
        let key = DataKey2::PullPayouts(engagement_id.clone());
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 6_300_000);
    }

    /// Add `amount` to `recipient`'s claimable `token` balance. No-op for a
    /// non-positive amount.
    ///
    /// Emits `("payout_credited", recipient)` with `(engagement_id, token, amount)`.
    pub(crate) fn credit_claimable_payout(
        env: &Env,
        engagement_id: &String,
        recipient: &Address,
        token: &Address,
        amount: i128,
    ) {
        if amount <= 0 {
            return;
        }
        let key = DataKey2::RecruiterClaimable(recipient.clone(), token.clone());
        let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(balance + amount));
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 6_300_000);
        env.events().publish(
            (Symbol::new(env, "payout_credited"), recipient.clone()),
            (engagement_id.clone(), token.clone(), amount),
        );
    }

    /// Send a recruiter-side amount that is not a milestone payout (e.g. a
    /// bond refund): credited on `pull_payouts` engagements, transferred
    /// otherwise.
    pub(crate) fn pay_or_credit_recruiter(
        env: &Env,
        engagement: &Engagement,
        token_client: &token::Client,
        recipient: &Address,
        amount: i128,
    ) {
        if Self::is_pull_payouts_internal(env, &engagement.id) {
            Self::credit_claimable_payout(
                env,
                &engagement.id,
                recipient,
                &token_client.address,
                amount,
            );
        } else {
            token_client.transfer(&env.current_contract_address(), recipient, &amount);
        }
    }
}
