# Security

Spaceship Racer is in private-alpha development and has not been independently
audited for deployment with real funds.

Report suspected vulnerabilities through this repository's **Security → Report a
vulnerability** feature. If private reporting is unavailable, contact the
maintainer through the GitHub profile rather than publishing exploit details.

Include affected components/commits, impact and a minimal reproduction on an
isolated testnet. Do not test against the hosted alpha, drain assets, publish
secrets or modify another player's state.

The server is not intended to receive player private keys. Local fixture
mnemonics appearing in tests are intentionally public and must never hold real
assets. Live deployment keys, access-control lists, databases and historical
operator recovery files are not part of this repository.
