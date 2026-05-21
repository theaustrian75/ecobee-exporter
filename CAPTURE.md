# Capturing the ecobee data API

Authentication is already solved — `ecobee-login` does the full Auth0
Universal Login + PKCE dance and writes a refresh token to disk. What's
still missing is the **data API** call: the request the exporter
should make, with the refresh-derived access token, to fetch
thermostat + sensor + runtime state.

From the JWT `audience` value the access token is signed for, that API
probably lives at `https://prod.ecobee.com/api/v1`. It is *not*
necessarily literally `beehive.ecobee.com` (which is an internal
gateway name); the public-facing host is what we need to discover.

This document walks through capturing that traffic. The good news is
this part doesn't require a phone — the ecobee web app at
[`home.ecobee.com`](https://home.ecobee.com) appears to use the same
backend, and desktop browsers don't pin certificates, so mitmproxy can
intercept it straightforwardly.

## What you need

  - `mitmproxy` installed (`pipx install mitmproxy`).
  - A desktop browser. Firefox is cleanest because you can set its
    proxy independently from the OS without affecting anything else.
  - An ecobee account (the same one you used for `ecobee-login`).

## Step 1: launch mitmproxy

```sh
mitmproxy --listen-port 8080
```

Leave this running. Use the filter expression `~d ecobee` (then
Enter) to hide noise from non-ecobee hosts.

## Step 2: point a browser through mitmproxy

In Firefox:

  1. Settings → Network Settings → Manual proxy configuration.
  2. HTTP Proxy: `127.0.0.1`, Port: `8080`. Tick "Also use this proxy
     for HTTPS." Save.
  3. Visit `http://mitm.it` in Firefox. Download the Firefox cert,
     then Settings → Privacy & Security → Certificates → View
     Certificates → Authorities → Import. Trust it for websites.

Verify the proxy is working: load any HTTPS site; it should appear in
the mitmproxy flow list with a valid 200 response. If it errors,
you've got the cert install wrong.

## Step 3: load the ecobee web app

  1. Visit `https://home.ecobee.com` in Firefox. If you're not logged
     in, log in (Auth0 will redirect you through the same flow
     `ecobee-login` uses; MFA will challenge you in the browser).
  2. Wait for the thermostat dashboard to render. The page makes
     several API calls to populate the thermostat tile.
  3. Force-refresh (Ctrl-F5) to make sure all the lazy-loaded calls
     fire while you're watching.

## Step 4: identify the data API call(s)

In mitmproxy, look for requests that:

  - go to a `*.ecobee.com` host that isn't `auth.ecobee.com` (those
    are the Auth0 token-management calls);
  - send `Authorization: Bearer …` with a JWT;
  - return JSON containing identifiers, names, and what look like
    sensor readings.

Likely candidates based on the audience we observed:

  - `prod.ecobee.com/api/v1/...`
  - `prod.ecobee.com/api/...`
  - `prod.ecobee.com/graphql` (if it's still GraphQL)
  - `mobile.ecobee.com/...`

When you find one, hit `w` in mitmproxy and save the flow into
`samples/list-thermostats.flow`. The raw response body is also worth
copying to `samples/list-thermostats.json` for use as a test fixture.

What you specifically need to record:

  - The full URL (host + path + query string).
  - HTTP method (almost certainly GET or POST).
  - Whether the body is GraphQL (`{ "query": "…", "variables": … }`)
    or REST (path-and-query-driven).
  - The full response body so we can map it into `model::Thermostat`.
  - Any headers beyond `Authorization` and `User-Agent` that the
    server appears to require (some Auth0-fronted APIs check for an
    `audience`-matching header).

## Step 5: sanitize and commit a fixture

  1. Copy `samples/list-thermostats.json` → `samples/example-list-thermostats.json`.
  2. Open the example and scrub:
     - thermostat identifiers (replace with `411111111111` etc.)
     - thermostat names that are personally identifying
     - any JWTs that leaked into the body
     - account email addresses
     - latitude/longitude
  3. Commit the example file; `samples/*.json` (other than
     `example-*.json`) is gitignored.

## Step 6: wire the exporter

  1. Set `beehive.endpoint` in `ecobee-exporter.toml` to the host +
     path you captured (or set it to just the host root if the path
     is per-query). If it turns out to be REST rather than GraphQL,
     the existing `BeehiveClient` will need a small refactor — see
     comments in `src/beehive/client.rs`.
  2. Update `src/beehive/queries.rs`:
     - For GraphQL: replace `LIST_THERMOSTATS` with the actual query
       body and `ListThermostatsResponse` with the real serde types.
     - For REST: replace the module's contents with a `fetch_state`
       function that does the right `GET`/`POST` and returns parsed
       types.
  3. Implement `translate()` in the same file, mapping the wire
     response into `Vec<Thermostat>`.
  4. Update `src/beehive/mod.rs::fetch` to call the new code path
     and return the translated `Vec<Thermostat>`.
  5. `cargo run --release`. Hit `/metrics`. Watch the
     `ecobee_*` series populate against your real thermostat.

## After capture

Stop mitmproxy. Untrust the mitm CA in Firefox (Settings → Privacy &
Security → Certificates → View Certificates → Authorities → mitmproxy
→ Delete or Distrust). Disable the proxy in Firefox.

## If a desktop capture doesn't yield the right host

The web app might use a different backend than the mobile app. If
that's the case, you'd need to capture from the mobile app, which is
the hard path (`auth.ecobee.com` already cert-pinned per the prior
capture attempt, and the data host probably is too). In that case:

  1. Use a rooted Android phone or an emulator.
  2. Install Frida + objection, attach to `com.ecobee.athenamobile`.
  3. `android sslpinning disable` in objection.
  4. Re-run the capture as in the original `CAPTURE.md` workflow.

But try the web-app route first — it's almost always sufficient.
