# Testing

All contract tests are unit tests in one file,
[`contracts/hiresettle/src/test.rs`](contracts/hiresettle/src/test.rs). There
are no integration tests under `tests/` and no doctests (`doctest = false` in
the crate's `Cargo.toml`). Tests use the in-process Soroban test environment
(`soroban-sdk` with the `testutils` feature), so you need no network, no
`stellar` CLI and no WASM build to run them.

## Running tests

Run these from `contracts/hiresettle/`:

```bash
cargo test                       # whole suite (same as `make test`)
cargo test -- --list             # list every test name without running any
```

From the repository root, name the package or the manifest instead:

```bash
cargo test -p hiresettle
cargo test --manifest-path contracts/hiresettle/Cargo.toml   # what CI runs
```

CI ([`.github/workflows/test.yml`](.github/workflows/test.yml)) runs the full
suite on every push and pull request to `main`.

### Running a single test

`cargo test <FILTER>` runs every test whose name **contains** `FILTER`. Add
`-- --exact` to match one name exactly:

```bash
# One test, exact match
cargo test test_waive_platform_fee_marks_engagement_waived -- --exact

# Same, and show println!/dbg! output (hidden by default for passing tests)
cargo test test_waive_platform_fee_marks_engagement_waived -- --exact --nocapture
```

Test names are paths inside the `test` module. If you want to be explicit, the
full path works as well: `cargo test test::test_create_engagement_success -- --exact`.

### Running a group of tests

Test names share prefixes by feature, so a substring filter selects a whole area:

```bash
cargo test platform_fee        # platform fee, fee event, fee waiver
cargo test co_recruiter        # co-recruiter split tests
cargo test streamed            # streaming milestone payouts
cargo test public_engagement   # get_public_engagement_ids
cargo test dispute             # anything with "dispute" in its name
```

Use `cargo test -- --list | grep <word>` to preview what a filter will match.

### Other useful flags

| Command | Purpose |
|---|---|
| `cargo test -- --test-threads=1` | Run tests one at a time. Output from failing tests is easier to read. |
| `cargo test -- --ignored` | Run only `#[ignore]`d tests (there are none today). |
| `cargo test --no-run` | Compile the tests without running them. A quick check after a refactor. |
| `RUST_BACKTRACE=1 cargo test <name> -- --exact` | Print a backtrace for a panic you didn't expect. |

### Test snapshots

The Soroban test environment writes a JSON ledger snapshot for each test to
`contracts/hiresettle/test_snapshots/test/<test_name>.<n>.json`. They are
handy when debugging a single test, but they are git-ignored
(`test_snapshots/` in `.gitignore`), so don't commit them. Delete the directory
whenever you like.

## How `test.rs` is organised

The file opens with a `//!` module doc containing a table of test counts by
category (core lifecycle, creation validation, disputes, fees & payouts, admin,
queries, …). Those counts are a snapshot. Recount with:

```bash
grep -c '^#\[test\]' contracts/hiresettle/src/test.rs         # all tests
grep -c '#\[should_panic' contracts/hiresettle/src/test.rs    # panic-assertion tests
```

After the header, the file is split into sections, each marked by a banner:

```rust
// ============================================================
// ISSUE #466 — STREAMING MILESTONE PAYOUT
// ============================================================
```

Most sections cover one GitHub issue or feature, with tests named
`test_<feature>_<behaviour>`. To find the tests for a feature, search the
banners:

```bash
grep -n '^// [A-Z#I]' contracts/hiresettle/src/test.rs
```

### Shared helpers (top of the file, `TEST HELPERS` section)

| Helper | What it does |
|---|---|
| `setup()` → `(env, contract_id, token_id, company, recruiter, arbiter)` | Builds a fresh `Env` at ledger `100` with `mock_all_auths()`, registers the contract and a Stellar asset token, mints `500_000_000_000` units to `company`, and calls `init(&company)`. **The company is also the contract admin**, so admin-only calls in tests pass `&company`. |
| `create_standard_engagement(env, client, token_id, company, recruiter, arbiter, id)` | Creates an engagement for `1_000_000_000` units with a single arbiter (quorum 1), job title `"Senior Engineer"`, `build_milestones` and `default_config()`. |
| `build_milestones(env)` | Three milestones, 30% placement → 40% 30-day retention → 30% 90-day retention, each depending on the one before. |
| `default_config()` | An `EngagementConfig` with every option off: no co-recruiter, `recruiter_split_bps: 10_000`, `is_public: false`, no stream, no bond. Copy it and change the one field you're testing: `let mut cfg = default_config(); cfg.is_public = true;`. |
| `advance_ledger(env, extra)` | Moves the ledger sequence forward by `extra`. Use it for retention windows, confirm and dispute windows, TTLs and time locks. |
| `has_event(env, name)` | Returns `true` if any event emitted so far has `name` as its first topic. |
| `UPGRADE_DUMMY_WASM` | Bytes of `contracts/hiresettle/testdata/upgrade_dummy.wasm`, built from the `contracts/upgrade_dummy` crate, used by the `execute_upgrade` success test. |

### Test patterns

A typical success test:

```rust
#[test]
fn test_waive_platform_fee_marks_engagement_waived() {
    let (env, contract_id, token_id, company, recruiter, arbiter) = setup();
    let client = HireSettleContractClient::new(&env, &contract_id);
    create_standard_engagement(
        &env, &client, &token_id, &company, &recruiter, &arbiter, "ENG-WAIVE",
    );
    let id = String::from_str(&env, "ENG-WAIVE");

    assert!(!client.is_fee_waived(&id));
    client.waive_platform_fee(&company, &id);

    assert!(has_event(&env, "platform_fee_waived"));
    assert!(client.is_fee_waived(&id));
}
```

Error paths use `#[should_panic(expected = "...")]` with the exact panic
string listed in the README's [Errors](README.md#errors) section and in
`errors.rs`:

```rust
#[test]
#[should_panic(expected = "unauthorized")]
fn test_waive_platform_fee_non_admin_rejected() { /* ... */ }
```

Two things to keep in mind:

- **Auth is mocked.** `mock_all_auths()` makes every `require_auth()` pass, so
  a test can't prove that a signature was required. Authorization tests instead
  pass the *wrong address* and expect the contract's explicit identity check to
  panic (`unauthorized`).
- **Token balances are the ground truth for payouts.** Payout tests read
  `token::Client::new(&env, &token_id).balance(&addr)` before and after the
  call and assert exact deltas. Don't approximate: all contract math is integer
  (see [Numeric Precision](README.md#numeric-precision)).

## Adding a test

1. Add a banner section at the end of `test.rs` for your issue or feature, or
   add to an existing section if one matches.
2. Start from `setup()` and `create_standard_engagement`, or use
   `client.create_engagement` with a modified `default_config()` when you need
   non-default options.
3. Give each engagement ID in the test its own name (`"ENG-<FEATURE>-1"`). IDs
   must be unique within a test's `Env`.
4. Name the test `test_<feature>_<behaviour>` so substring filters pick it up
   with its neighbours.
5. For an error path, match the exact panic string in `should_panic(expected = ...)`.
6. Test names must be unique across the whole file, because it is one
   module. Before pushing, run `cargo test --no-run` to catch duplicate
   definitions (`E0428`).
7. Run `cargo test <your_test> -- --exact`, then the full suite.
