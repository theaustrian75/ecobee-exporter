# Sample captures

Drop captured JSON / HAR / mitmproxy flow files here as you reverse-engineer
the Beehive API.

Anything matching `*.json`, `*.flow`, or `*.har` is gitignored by default —
they will almost certainly contain access tokens, refresh tokens, account
emails, or thermostat identifiers. Only files matching `example-*.json`
(sanitized, hand-curated fixtures) are tracked.

Suggested files to capture and save here for your own reference:

- `login.flow` — full mitmproxy flow of the username/password + MFA dance
- `refresh.flow` — full flow of a refresh-token-only token mint
- `list-thermostats.json` — the response body from the pull-to-refresh
  GraphQL call on the home screen
- `example-list-thermostats.json` — a sanitized version of the above, with
  identifiers, tokens, and names redacted. *This* one is safe to commit and
  is what `tests/parse_sample.rs` should be wired against.
