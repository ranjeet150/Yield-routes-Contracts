# Security Policy

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

If you discover a security vulnerability in the smart contracts or backend,
please report it responsibly:

1. Email the maintainers at the address in the GitHub profile
2. Include a description of the vulnerability, steps to reproduce, and impact
3. Allow up to 72 hours for an initial response
4. Do not disclose publicly until a fix has been deployed

## Scope

In scope:
- All Soroban contracts in `/contracts`
- Backend API in `/backend`
- Frontend wallet interactions

Out of scope:
- Denial-of-service attacks against the backend server
- Issues in third-party dependencies not introduced by this codebase
- Social engineering

## Smart Contract Security Notes

These contracts have **not been audited** by an external security firm.
Use on mainnet at your own risk. A formal audit is planned before mainnet launch.

Known limitations:
- Cross-contract calls use `invoke_contract` which trusts the called contract's address stored in admin-controlled storage
- Oracle price feeds rely on authorized reporters; reporter key compromise affects TWAP integrity
- `pause()` / emergency controls rely on the admin key being secured

## Testnet vs Mainnet

All contracts are currently deployed to Stellar **testnet only**.
Contract IDs in `.env.deployed` will change with each deployment.
