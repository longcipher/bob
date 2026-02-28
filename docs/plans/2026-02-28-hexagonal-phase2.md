# Hexagonal Phase 2 Completion Note

## Goal

Expose framework-level policy and approval extension points to the CLI
composition root, so application integrators can tune guardrails without
changing runtime internals.

## Delivered

1. Added reusable runtime guardrail adapters in `bob-adapters`:
   - `policy_static::StaticToolPolicyPort`
   - `approval_static::StaticApprovalPort`
2. Extended `cli-agent` config:
   - `policy.default_deny`
   - `[approval]` with `mode` and `deny_tools`
3. Wired `RuntimeBuilder` with configured tool policy and approval ports.
4. Updated sample docs/config to demonstrate the new settings.
5. Added and passed tests for adapter behavior and CLI wiring defaults.

## Verification

- `cargo test -p bob-adapters`
- `cargo test -p cli-agent`
- `cargo test`

All commands pass on workspace state as of 2026-02-28.
