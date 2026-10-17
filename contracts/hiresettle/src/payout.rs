use soroban_sdk::{contractclient, contractimpl, token, Address, Env, String, Symbol};
use crate::*;

/// Interface a trusted swap-adapter contract must implement (issues #458, #549).
///
/// HireSettle first calls `quote` for the expected output, derives
/// `min_out = quote * (10_000 - max_slippage_bps) / 10_000` from the
/// recipient's slippage tolerance, then calls `swap`. Inside `swap` the
/// adapter pulls `amount_in` of `from_token` from `payer` (HireSettle
/// pre-authorizes exactly that `transfer(payer, adapter, amount_in)`),
/// delivers the swapped `to_token` amount to `recipient` and returns it.
///
/// If the adapter cannot deliver at least `min_out` it must panic. HireSettle
/// invokes `swap` with `try_`, so the panic rolls back the adapter's pull and
/// the payout falls back to the original token instead of failing.
#[contractclient(name = "SwapAdapterClient")]
pub trait SwapAdapter {
    fn quote(env: Env, from_token: Address, to_token: Address, amount_in: i128) -> i128;

    fn swap(
        env: Env,
        payer: Address,
        from_token: Address,
        to_token: Address,
        amount_in: i128,
        min_out: i128,
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
    /// engagement's escrow token. Also resets their slippage tolerance to the
    /// default (issue #549).
    pub fn clear_recruiter_payout_token(env: Env, recruiter: Address) {
        recruiter.require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey2::RecruiterPayoutToken(recruiter.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey2::RecruiterPayoutSlippage(recruiter.clone()));
        env.events()
            .publish((Symbol::new(&env, "payout_token_cleared"), recruiter), ());
    }

    /// Return the recruiter's preferred payout token, if any.
    pub fn get_recruiter_payout_token(env: Env, recruiter: Address) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey2::RecruiterPayoutToken(recruiter))
    }

    // ----------------------------------------------------------
    // ISSUE #549 — SWAP SLIPPAGE PROTECTION
    // ----------------------------------------------------------

    /// Recruiter sets the maximum slippage, in basis points, they accept when
    /// a payout is swapped into their preferred token. Leaves the token
    /// preference itself untouched. `10_000` accepts any output.
    ///
    /// # Panics
    /// - `"InvalidSlippageBps"` — `max_slippage_bps` exceeds 10 000.
    ///
    /// # Events
    /// `("payout_slippage_set", recruiter)` with `max_slippage_bps`.
    pub fn set_recruiter_payout_slippage(env: Env, recruiter: Address, max_slippage_bps: u32) {
        recruiter.require_auth();
        if max_slippage_bps > FULL_SPLIT_BPS {
            panic!("InvalidSlippageBps");
        }
        let key = DataKey2::RecruiterPayoutSlippage(recruiter.clone());
        env.storage().persistent().set(&key, &max_slippage_bps);
        env.storage().persistent().extend_ttl(&key, 100_000, 6_300_000);
        env.events().publish(
            (Symbol::new(&env, "payout_slippage_set"), recruiter),
            max_slippage_bps,
        );
    }

    /// Return the recruiter's slippage tolerance in basis points, or
    /// `DEFAULT_MAX_SLIPPAGE_BPS` (100 = 1 %) if they never set one.
    pub fn get_recruiter_payout_slippage(env: Env, recruiter: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey2::RecruiterPayoutSlippage(recruiter))
            .unwrap_or(DEFAULT_MAX_SLIPPAGE_BPS)
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
    /// a swap adapter are configured.
    ///
    /// Issue #549: the swap must deliver at least the `min_out` derived from
    /// the adapter's quote and the recipient's slippage tolerance. If the
    /// quote fails or the adapter cannot meet `min_out`, the original token is
    /// paid instead and `payout_swap_fallback` is emitted, so the confirmation
    /// still succeeds. An adapter that reports success below `min_out` breaks
    /// the [`SwapAdapter`] contract and the whole call panics.
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
                let min_out = match Self::try_swap_payout(
                    env,
                    token_client,
                    &adapter,
                    &to_token,
                    recipient,
                    amount,
                ) {
                    Ok(()) => return,
                    Err(min_out) => min_out,
                };
                token_client.transfer(&env.current_contract_address(), recipient, &amount);
                env.events().publish(
                    (Symbol::new(env, "payout_swap_fallback"), recipient.clone()),
                    (token_client.address.clone(), to_token, amount, min_out),
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
