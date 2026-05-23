# Capturing Beehive traffic with mitmproxy

This guide walks through capturing your own ecobee mobile-app login,
token refresh, and home-screen GraphQL traffic so you can fill in the
`TODO(capture)` blocks in `src/beehive/auth.rs` and
`src/beehive/queries.rs`.

You do this work **once**, with your phone routed through mitmproxy on
the same LAN as your computer. After you've extracted the endpoint, the
GraphQL query bodies, and a refresh token (or the email/password login
shape), the exporter doesn't need mitmproxy again.

## What you need

- A Linux/macOS computer with `mitmproxy` installed
(`pipx install mitmproxy`, or your distro's package manager).
- An Android or iOS phone with the official ecobee app installed and
a working ecobee account.
- The phone and the computer on the same Wi-Fi.

Android is significantly easier than iOS because Apple makes
installing a user CA increasingly difficult, and modern iOS apps that
use `NSAppTransportSecurity` defaults will frequently refuse to honor
user-installed CAs at all. Try Android first.

## Step 1: launch mitmproxy

```sh
mitmproxy --listen-port 8080 --set block_global=false
```

Leave this running. Note your computer's LAN IP (`ip -4 a` or
`ifconfig`).

## Step 2: route the phone through mitmproxy

### Android

1. Settings → Network & internet → tap your Wi-Fi → Advanced → Proxy
→ Manual.
2. Proxy hostname: your computer's LAN IP. Port: `8080`. Save.
3. From the phone, open a browser and visit `http://mitm.it`. Tap
the Android cert, install it as a CA. Confirm in Settings →
Security → Encryption & credentials → Trusted credentials → User
that "mitmproxy" appears.

### iOS

1. Settings → Wi-Fi → tap the (i) next to your network → Configure
Proxy → Manual. Server: your computer's LAN IP. Port: `8080`.
Save.
2. From Safari, visit `http://mitm.it`. Download the iOS profile,
then Settings → Profile Downloaded → Install. Then Settings →
General → About → Certificate Trust Settings → enable "mitmproxy".

If you don't see ecobee traffic after this, the app is using
certificate pinning. See "If the app refuses to talk" below.

## Step 3: capture a login

1. Force-quit the ecobee app on the phone.
2. Re-open it. If you're already logged in, log out from inside the
app and log back in — that triggers the full email/password +
(likely) MFA exchange.
3. As you log in, watch the mitmproxy flow list. Filter to interesting
hosts with `~d ecobee` to see only ecobee-owned traffic.
4. Once login completes, hit `w` in mitmproxy and save the whole flow
to `samples/login.flow`.

What to look for in the captured flows:

- The **auth endpoint** host and path. It might be on `beehive.…`,
`api.…`, or a Cognito host (`cognito-idp.*.amazonaws.com`).
- The **request body** of the login call: is it
`application/x-www-form-urlencoded`, JSON, or AWS Cognito's
`InitiateAuth` JSON shape?
- Whether there's a **second round-trip** for an MFA code (look for
`RespondToAuthChallenge` or `verify` in the path).
- The **response shape**: there should be an `access_token` (often a
JWT), a `refresh_token`, and an `expires_in` or `exp` field.
- Any custom headers the app sends, especially anything starting with
`x-ecobee-…` or version-related (`User-Agent`, `x-app-version`).

## Step 4: capture a token refresh

1. After a successful login, leave the phone alone for an hour or
so. Then open the ecobee app again.
2. mitmproxy should show a brief auth call as the app refreshes its
access token without prompting for a password.
3. Save that as `samples/refresh.flow`.

This is the flow you'll actually wire into the exporter, since the
exporter never needs to do an initial login if you give it a
refresh token directly.

## Step 5: capture a GraphQL data fetch

1. With the app open, force a pull-to-refresh on the thermostat
screen.
2. Look for a `POST` to whatever host Beehive lives on. The body
will be a JSON document with `query`, `operationName`, and
`variables` fields — that's the GraphQL.
3. Save the request body and response body to
`samples/list-thermostats.json` (response only is what we want
for fixture-driven tests).
4. Note the host + path — this is what goes into
`beehive.endpoint`.

The response is what `src/beehive/queries.rs::translate` needs to map
into `Vec<Thermostat>`.

## Step 6: sanitize and commit a fixture

1. Copy `samples/list-thermostats.json` to
`samples/example-list-thermostats.json`.
2. Open the example file and replace any of the following with stable
placeholder values:
  - thermostat identifiers
  - thermostat names that contain personal info
  - JWT access tokens or refresh tokens
  - account email addresses
  - latitude/longitude in any weather block
  - sensor MAC-like IDs
3. Commit the example file. The raw `list-thermostats.json` is
gitignored, so it stays local.
4. Wire up `tests/parse_sample.rs` to deserialize the example fixture
into your real `ListThermostatsResponse` and assert that
`translate()` returns the expected `Vec<Thermostat>`.

## Step 7: wire the exporter

With the captures in hand:

1. Fill in the response types in `src/beehive/queries.rs`
(`ListThermostatsResponse` and friends) to match what Beehive
actually returns. Make liberal use of `#[serde(rename = "…")]` if
the wire field names are inconvenient.
2. Implement `translate()` in the same file. Keep it boring and
allocation-light.
3. Fill in `src/beehive/auth.rs::refresh` (and `login` if you want
the exporter to be able to mint its own tokens). The
`BeehiveClient::post` helper is GraphQL-only; for the auth endpoint
you'll want a direct `client.http().post(…)`.
4. Write the refresh token into `ecobee-exporter.toml` under
`[beehive] refresh_token = "…"`.
5. `cargo run --release`. Hit `/metrics`. Verify the series populate.

## If the app refuses to talk

Symptoms: the ecobee app loads but immediately fails with a generic
"can't connect" error after you turn on the proxy, and mitmproxy logs
TLS handshake errors with `unknown ca`.

That's certificate pinning. Options, roughly in increasing order of
effort:

1. **Try `mitmproxy --set ssl_insecure=true`** — sometimes the app
pins only to specific CAs and falls back to user CAs for others.
2. **Use an older app version** on Android. Pinning changes from
release to release; an APK from 2-3 years ago may not pin at all.
APKMirror archives historical builds.
3. **Use Frida + objection on an Android device** with developer
mode to disable the pinning hook at runtime:
`objection -g com.ecobee.athenamobile explore` then `android
sslpinning disable`. This requires either a rooted phone or a
repackaged APK.
4. **Try the web app** (`home.ecobee.com`) in a desktop browser
while mitmproxy is set as the browser's proxy. If the web app
speaks to the same Beehive endpoint, you can skip the phone
entirely. The kanzash blog post implies the Haven web portal
does in fact use Beehive.

## After capture

Delete the mitmproxy CA from your phone:

- Android: Settings → Security → Encryption & credentials → User
certificates → mitmproxy → Remove.
- iOS: Settings → General → VPN & Device Management → mitmproxy
profile → Remove Profile.

And remove the proxy setting from your Wi-Fi network so your phone
stops trying to talk through your computer.