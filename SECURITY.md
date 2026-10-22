# Security

## Reporting a vulnerability

Report it privately through GitHub: **Security → Report a vulnerability** on this repository. You can also email **support@ribqa.com**. Include what you found, how to reproduce it and what it affects. Please do not open a public issue for a vulnerability.

The same contact is published at [ribqa.com/.well-known/security.txt](https://ribqa.com/.well-known/security.txt).

## Supported versions

Only the latest release gets security fixes. The app updates itself from a signed feed; an update is refused unless its signature matches the public key built into the app.

## Scope

- The Aleph Edge desktop app in this repository.
- The update feed at `https://ribqa.com/downloads/aleph-edge/latest.json` and the installers next to it.
- The TradingView relay endpoints the app uses on `ribqa.com`.

## Keys and secrets

- Exchange API keys stay on your computer, in an encrypted vault. The app never sends them to our servers.
- Use trade-only keys. Never enable withdrawals on a key you give the app.
- The TradingView webhook URL is a secret: anyone who has it can send alerts to your bot. Rotate it in Settings → TradingView if it leaks, and set a passphrase on the TradingView bot.
- Never commit keys, `.env` files or vault files.
