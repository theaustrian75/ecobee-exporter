# Capturing traffic (only if the defaults stop working)

You almost certainly don't need this file. The exporter works
out-of-the-box against `https://api.ecobee.com/1/thermostat` using an
Auth0-issued JWT minted by `ecobee-login`. The original need for a
capture-driven RE pass turned out to be unnecessary: the mobile app's
JWT is accepted by the long-documented developer REST API host, and
the response shape is the same one ecobee has been documenting for a
decade.

This guide is here in case any of the following ever changes:

  - ecobee revokes the mobile-app Auth0 client (`yg66ag34vWdf2Hs4oO2ih2BvI16KrkOR`)
    or rotates its parameters.
  - ecobee starts rejecting mobile-app JWTs at `api.ecobee.com/1`
    (e.g. tightens audience validation to require literal host match
    with `prod.ecobee.com`).
  - You want to add metrics that aren't on the
    `selection` selectors we currently send (`includeRuntime`,
    `includeSensors`, `includeSettings`).

## When the exporter stops working

Look at the error log line. Common failure modes and what to look at:

| Error                                                | Likely cause                                              |
|------------------------------------------------------|-----------------------------------------------------------|
| `Auth0 refresh failed: ... invalid_grant`            | Your refresh token was revoked; rerun `ecobee-login`.     |
| `HTTP 401` from `api.ecobee.com/1/thermostat`        | Audience tightened or scope changed; recapture login.     |
| `HTTP 403`                                            | Scope mismatch; the captured `SCOPE` constant is stale.   |
| `HTTP 404`                                            | Endpoint changed; recapture or check `api.ecobee.com/v1`. |
| `API status N` (with HTTP 200)                       | App-level error from ecobee; see the message they return. |

## Recapturing OAuth parameters

The Auth0 parameters baked into `src/auth0.rs` came from a one-shot
mitmproxy capture of the mobile-app login flow. If you ever need to
recapture them:

  1. Install `mitmproxy` (`pipx install mitmproxy`).
  2. Run `mitmproxy --listen-port 8080 --set block_global=false`.
  3. Set your phone's Wi-Fi proxy to your computer's LAN IP, port
     8080. Trust the mitmproxy CA by visiting `http://mitm.it` from
     your phone.
  4. Force-quit the ecobee app, then re-open and log in.
  5. The Auth0 Universal Login flow happens in Chrome Custom Tabs
     and *is* visible to the proxy (only the app's own networking
     stack — for the API calls — is cert-pinned). Watch for a request
     to `https://auth.ecobee.com/authorize?...` and read off:
       - `client_id`
       - `redirect_uri`
       - `audience`
       - `scope`
  6. Update the constants at the top of `src/auth0.rs`.

The token-exchange call (`POST /oauth/token`) and any data-API calls
happen from the app's own network stack and are not visible. That's
fine — you only need the `/authorize` parameters; the rest of the
flow can be driven by `ecobee-login` once those constants are right.

## Recapturing the data-API contract

Run mitmproxy against your desktop browser (no cert pinning to worry
about there) loading [`home.ecobee.com`](https://home.ecobee.com).
The thermostat-list call you see is the one we model in
`src/beehive/queries.rs`. If the path, query format, or response
shape has changed, update that file accordingly. The whole module
is unit-tested against a fixture, so update the `SAMPLE` constant in
its `tests` module to match the new shape and adjust the
deserialization types until tests pass.
