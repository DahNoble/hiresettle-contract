# Security Policy

## Reporting a Vulnerability

We take security vulnerabilities in the hiresettle smart contract seriously. If you discover a vulnerability, please report it responsibly using one of the following methods:

### Private Reporting Options

1. **GitHub Private Vulnerability Reporting** (Recommended)
   - Navigate to the repository's Security tab
   - Click "Report a vulnerability"
   - Fill out the vulnerability report form with details
   - This creates a private advisory for our team

2. **Email**
   - Send details to: [security@hiresettle.dev](mailto:security@hiresettle.dev)
   - Include a description of the vulnerability, affected versions, and potential impact
   - Do not publicly disclose details until we've had time to address the issue

### What to Include

When reporting a vulnerability, please provide:
- Description of the vulnerability and its potential impact
- Steps to reproduce the issue (if applicable)
- Affected version(s) or commit hash
- Suggested fix (if you have one)

## Disclosure Timeline

- **Initial Response:** We aim to acknowledge vulnerability reports within 48 hours
- **Assessment Period:** We will assess the vulnerability and determine severity within 5 business days
- **Fix Development:** Critical vulnerabilities will be prioritized; timeline depends on complexity
- **Coordinated Disclosure:** We will work with you on a coordinated disclosure timeline, typically 90 days from initial report unless otherwise agreed

## Supported Versions

| Version | Status | Security Updates |
|---------|--------|------------------|
| Main branch | Active | Yes |
| Tagged releases | Limited | For critical issues only |

Only the main branch receives active security updates. We recommend using the latest version from main for production deployments.

## Scope

This policy applies to vulnerabilities in:
- Smart contract code (within `contracts/hiresettle/src/`)
- Contract-related dependencies as defined in `Cargo.toml`

This policy does NOT apply to:
- Issues in demonstration code or examples
- Vulnerabilities in third-party libraries (report to the library maintainers)

## Audit History

The contract has **not yet undergone an external third-party audit**. The table
below records every security review of the contract, internal or external, so
integrators can see what has been checked and against which code.

| Date | Type | Reviewer | Scope | Commit / PR | Report |
|------|------|----------|-------|-------------|--------|
| 2026-07-24 | Internal hardening pass | Community contributor | Issues #182–#185 | PR #211 (`9ae1910`) | — |
| 2026-07-25 | Internal hardening pass | Community contributor | Issues #178–#181 | PR #214 (`044fefa`) | — |

When a review is completed, add a row with the date it finished, whether it was
internal or external, the reviewing party, the files or features in scope, the
commit it was run against, and a link to the published report (if any). Findings
from a review are tracked as GitHub issues and should be linked from the report.

## Security Best Practices for Users

- Always audit smart contracts before deploying to production
- Use the contract with appropriate access controls
- Test extensively in a testnet environment first
- Monitor contract activity regularly
- Keep Rust and Cargo dependencies updated

## Acknowledgments

We appreciate the security research community's efforts to help keep our contract safe. With permission, we will acknowledge researchers in a security advisory upon public disclosure.

## Questions?

If you have questions about this security policy, please open an issue (for non-sensitive questions) or reach out privately using the methods above.
