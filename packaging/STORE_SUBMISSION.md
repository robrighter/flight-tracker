# Microsoft Store submission guide — Flight Tracker

This walks through every input Partner Center asks for, in the order the
submission wizard presents them, with a recommended answer for each. Where
something needs a decision only you can make (support email, pricing), it's
called out explicitly.

## Registered identity (already fixed — do not change)

| Field | Value |
|---|---|
| Package/Identity/Name | `RobRighter.NearestFlightTracker` |
| Package/Identity/Publisher | `CN=231AD612-7448-4A51-A946-11B802E9219B` |
| PublisherDisplayName | `Rob Righter` |
| Package Family Name | `RobRighter.NearestFlightTracker_9frd0hc0c2w54` |
| Store ID | `9N944HGRLSC3` |

These are already baked into `packaging/AppxManifest.template.xml` and must
match exactly — Partner Center will reject a package whose `Identity` doesn't
match your registered app.

## Aircraft data coverage by region

The app's usefulness is gated entirely by **OpenSky Network**, a
crowdsourced *ground-receiver* network (not satellite-based) — it only sees
an aircraft if a volunteer's ADS-B antenna has line-of-sight to it (roughly
250–450 km depending on altitude/terrain). No receiver nearby means no data
at all, regardless of actual air traffic overhead. This is what the
description text and "Notes for certification" below are working around.

| Coverage | Regions |
|---|---|
| Strong | Western/Central Europe (Germany, Switzerland especially), UK, France, Benelux, Scandinavia |
| Good | Continental US, Canada |
| Moderate | Australia, New Zealand, Brazil (major metros), South Africa |
| Sparse | Most of Asia (China notably limited), Middle East, Russia, South America outside Brazil, India, Southeast Asia |
| Effectively none | Open ocean, polar regions, rural Africa, Central Asia, and oceanic long-haul routes generally (no ground receiver mid-ocean even though the traffic is real) |

Two secondary data sources have their own scope, independent of the above:
- The **FAA registry** lookup ([src/main.rs:79](src/main.rs:79)) only ever
  fires for US-registered aircraft (it's gated on the FAA's ICAO24
  allocation block) — everywhere else that one enrichment field is silently
  skipped, nothing breaks.
- **ADSBDB** and Nominatim reverse-geocoding work globally and aren't a
  coverage constraint.

None of this blocks the app from running anywhere — outside good coverage
it just shows "no aircraft in range" ([src/main.rs](src/main.rs), status
`NO CONTACTS`) instead of erroring. The fix is setting expectations in the
listing, not restricting markets (see section 3).

## 1. Product management → Properties

- **Category**: Navigation & maps. (Utilities & tools is a reasonable second
  choice if Navigation & maps doesn't fit how you want it discovered — pick
  based on how a user searching would think of it.)
- **Subcategory**: none required; leave default if the category has no
  subcategory options.
- **Privacy policy URL** (**required** — the app collects precise location):
  you need to host a privacy policy at a public URL before you can submit. A
  draft is at the bottom of this doc (`## Privacy policy draft`) — host it as
  a GitHub Pages page off this repo, a gist, or any URL you control, then
  paste that URL in.
- **Website**: `https://github.com/robrighter/flight-tracker` (matches
  `ARPURLINFOABOUT` already used in the WiX installer) unless you'd rather
  point at a dedicated project page.
- **Support contact info**: needs an email address or a support URL —
  supply whichever you want to actually field support requests from Store
  users. Not something I can fill in for you.
- **System requirements**: Windows 10 version 1809 (build 17763) or later,
  x64 or ARM64. No specific RAM/GPU/DirectX requirement — it's a lightweight
  native app. Internet connection required for aircraft data; Windows
  Location Services optional (falls back to a saved location).

## 2. Age ratings

Run the IARC questionnaire; based on what this app actually does, the
honest answers are:

- Violence, fear, sexual content, profanity, controlled substances,
  gambling: **None** / **No** to all.
- User-generated content, user-to-user chat/interaction, sharing your
  location **with other users**: **No** — the app has no social features;
  location is used only to query flight data for your own display.
- Does the app collect/transmit the user's **precise location**: **Yes** —
  disclose this. It's sent to OpenSky (as a search bounding box) and to
  Nominatim/OpenStreetMap (as a reverse-geocode point) to do the app's core
  job. This should land you at the lowest rating tier (typically "3+" /
  "Everyone") once the questionnaire accounts for the location disclosure.
- In-app purchases, ads: **No** (the app has neither).

## 3. Pricing and availability

- **Markets**: leave this open (all markets, or close to it). Store market
  selection controls billing/distribution availability, not where the app
  is technically allowed to run — restricting it wouldn't fix uneven
  OpenSky coverage, it would just arbitrarily exclude legitimate users in
  decently-covered places like Australia if you only picked, say, US +
  Europe. Set expectations instead via the listing description (see
  `## Aircraft data coverage by region` below and the description text in
  section 5) rather than by blocking markets.
- **Pricing**: your call. Given it's a hobby ADS-B dashboard with no ads or
  IAP, Free is the common choice for this kind of utility, but that's a
  business decision, not a technical one.
- **Visibility**: Public, unless you want a private/test rollout first (Store
  supports "hidden" or targeted group visibility for staged rollout).
- **Release**: "As soon as it passes certification" is fine for a first
  release.

## 4. Packages

Upload `dist\store\FlightTracker-1.0.0.0.msixbundle` (built via
`packaging\build-msix.ps1`). It's an x64 + ARM64 bundle, so Partner Center
will serve the right native binary per device automatically — no need to
upload separate submissions per architecture.

Partner Center will re-sign the package for distribution; the self-signed
certificate `packaging/build-msix.ps1` used locally is only there so the
package is validly signed for upload, not for end-user trust.

## 5. Store listing

- **Product name**: Flight Tracker
- **Description** (suggested, edit to taste):

  > Flight Tracker finds and displays the nearest live aircraft to your
  > location, using real-time ADS-B data from the OpenSky Network. See
  > distance, heading, altitude, speed, operator, and route for aircraft
  > near you on a radar-style dashboard, with live position updates between
  > refreshes. Aircraft and registry details are enriched from public
  > aviation databases (ADSBDB, FAA registry) when available.
  >
  > Flight Tracker uses your location (or a saved fallback location) to find
  > nearby air traffic and does not require an account, collect analytics,
  > or show ads.
  >
  > Coverage depends on OpenSky Network's volunteer receiver density and is
  > strongest in North America, Europe, and Australia/New Zealand. In areas
  > with few or no volunteer receivers nearby, the app may show no aircraft
  > in range even during normal air traffic.

- **What's new in this version**: something like "Initial Microsoft Store
  release" for the first submission.
- **Features** (bullet list Partner Center asks for separately from the
  description):
  - Live nearest-aircraft radar with smooth position updates between data
    refreshes
  - Distance, bearing, altitude, speed, operator, and route detail for
    tracked aircraft
  - Aircraft registry and FAA lookup enrichment
  - Works with Windows Location Services or a manually saved location
- **Search terms / keywords**: `flight tracker`, `ADS-B`, `aircraft radar`,
  `aviation`, `plane tracker`, `nearest aircraft`
- **Screenshots** (**required**, at least 1, recommend 3–5): captured at
  native physical resolution (1755×989, above the 1366×768 minimum) in
  `packaging/StoreListingAssets/`:
  - `Screenshot-01-Overview.png` — Overview tab with live traffic
  - `Screenshot-02-RawReport.png` — Raw Report tab showing the multi-contact
    text report
  - `Screenshot-03-Selection.png` — a non-nearest contact selected, showing
    the radar target tracking
  
  Upload in that order (Partner Center uses the first as the primary
  listing image). All show real live ADS-B data from this session.
- **Store logo / app icon for the listing**: use
  `packaging/StoreListingAssets/StoreListing-300x300.png` (generated this
  session, matches the in-app taskbar icon) wherever Partner Center asks for
  a promotional app icon image.
- **Copyright and trademark info**: e.g. `© 2026 Rob Righter` — your call on
  exact wording.

## 6. Notes for certification (optional but recommended)

Since the app requests the `location` capability and needs live network
access to show anything meaningful, leave a note for the certification
testers, e.g.:

> This app requires internet access and optionally Windows Location
> Services to function. It queries the public OpenSky Network for nearby
> ADS-B aircraft positions within a configurable radius. If no aircraft are
> currently airborne within range of the test device's location, the app
> will correctly show "no aircraft in range" rather than an error — this is
> expected, not a bug. To guarantee visible traffic during review, testers
> can pass `--lat <value> --lon <value>` pointing at a busy airport area, or
> use the in-app "set location" flow.

## Local sideload testing before you submit

The bundle is signed with a self-signed certificate
(`CN=231AD612-7448-4A51-A946-11B802E9219B`, generated in
`Cert:\CurrentUser\My`) so it's a validly-signed package, but this machine
doesn't *trust* that certificate yet, so `Add-AppxPackage` will refuse to
install it as-is. I deliberately didn't change that — trusting a
certificate (or enabling Developer Mode) is a system security setting, and
I'll only touch that with your explicit go-ahead. Two ways to actually run
the packaged build locally before submitting, if you want to verify it end
to end:

1. **Enable Developer Mode** (Settings → Privacy & security → For
   developers) — lets you sideload any signed package regardless of trust,
   without touching the certificate store. Simplest option.
2. **Trust the certificate**: export it and import into
   `Cert:\LocalMachine\Root` (requires admin). More faithful to "would a
   real user's machine accept this," but it's a machine-wide trust change.

Let me know if you want me to walk through either of those, or just do them
yourself and run:

```powershell
Add-AppxPackage -Path "dist\store\FlightTracker-1.0.0.0.msixbundle"
```

## Privacy policy draft

Host this (or your edited version of it) at a public URL and use that URL
for the "Privacy policy URL" field above.

```markdown
# Flight Tracker — Privacy Policy

Flight Tracker is a Windows desktop app that displays live nearby aircraft
using public flight-tracking data.

## What data the app uses

- **Your location**: Flight Tracker uses Windows Location Services (or a
  location you manually save in the app) to determine what aircraft are
  near you. Your coordinates are sent to:
  - OpenSky Network (opensky-network.org), to query aircraft within a
    search radius of your location.
  - OpenStreetMap Nominatim (nominatim.openstreetmap.org), to convert your
    coordinates and nearby aircraft coordinates into human-readable place
    names.
- **Aircraft identifiers** (e.g. transponder codes, callsigns) are sent to
  ADSBDB (api.adsbdb.com) and the FAA aircraft registry
  (registry.faa.gov) to look up aircraft, route, and registration details.

## What the app does not do

- No account or sign-in is required.
- No analytics, telemetry, or usage tracking.
- No advertising.
- No data is sold or shared with anyone beyond the public aviation APIs
  listed above, which are necessary to show you flight data.

## Data storage

A fallback location (if you save one) is stored locally on your device at
`%APPDATA%\Flight Tracker\settings.json`. It is never uploaded anywhere
except as part of the location queries described above.

## Contact

[your support email or contact URL here]
```
