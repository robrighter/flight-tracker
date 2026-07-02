# Flight Tracker

Flight Tracker is a native Windows desktop app that finds the aircraft closest to you and shows what it knows about that plane — distance, heading, altitude, speed, operator, route, and registry — on a radar-style ops-board dashboard.

It's a self-contained Rust executable (no runtime dependencies, no installer prerequisites) that estimates your location, searches nearby live aircraft positions via ADS-B, picks the closest aircraft, enriches it with public aircraft/route/registry data, and displays everything on a single-instrument dashboard inspired by mid-century airport ops rooms.

## What It Shows

For aircraft near you, Flight Tracker can display:

- Distance from you and bearing from your location
- Callsign and ICAO24 Mode-S hex identifier
- Nearest city or place to the aircraft
- Altitude, ground speed, track, vertical rate, squawk, and position source
- Commercial airline when it can be identified
- Military operator/country when it can be inferred from known callsigns
- Origin and destination when ADSBDB has route data for the callsign
- Aircraft registration, model, owner, and photo URL when ADSBDB has metadata
- FAA registry information for U.S. aircraft when the Mode-S hex maps to an N-number

The app is intentionally honest about uncertainty. Public ADS-B state vectors do not always include operator, route, or registry metadata, so unknown fields are shown as unknown rather than guessed.

## Data Sources

- **Windows Location API**: estimates your current PC location when `--lat` and `--lon` are not provided.
- **OpenSky Network**: provides live aircraft state vectors used to find nearby aircraft.
- **OpenStreetMap Nominatim**: reverse-geocodes coordinates into a nearby city/place.
- **ADSBDB**: enriches aircraft and route data by ICAO24/callsign when available.
- **FAA Aircraft Registry**: provides registry details for U.S. aircraft when the aircraft Mode-S code maps to an N-number.

OpenSky's anonymous API is rate limited; see [OpenSky Credentials](#opensky-credentials) for a higher quota.

### Coverage

Aircraft data is only as good as OpenSky's crowdsourced ADS-B receiver network — it's ground-based, not satellite, so it only sees traffic within range of a volunteer's antenna. Coverage is strongest in North America, Europe, and Australia/New Zealand, and sparse or nonexistent in much of the rest of the world (including open ocean and oceanic long-haul routes). Outside good coverage, the app correctly shows no aircraft in range rather than erroring. The FAA registry enrichment specifically only applies to U.S.-registered aircraft.

## Build

Build a native release executable for your architecture:

```powershell
cargo build --release
```

Or target a specific architecture explicitly, e.g. for distribution:

```powershell
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

The binary is self-contained — UI assets and the app icon are baked in at compile time via `include_bytes!`, so nothing else needs to ship alongside `flight-tracker.exe`.

## CLI Usage

Find the closest aircraft using the Windows PC location API:

```powershell
.\target\release\flight-tracker.exe
```

Use exact coordinates instead of Windows location:

```powershell
.\target\release\flight-tracker.exe --lat 40.7128 --lon -74.0060
```

If Windows cannot determine your location, save a fallback location in settings:

```powershell
.\target\release\flight-tracker.exe --set-location --lat 40.7128 --lon -74.0060 --location-label "Home"
```

View or clear that saved fallback:

```powershell
.\target\release\flight-tracker.exe --show-settings
.\target\release\flight-tracker.exe --clear-location
```

Search a wider area (radius must be greater than `0` and no more than `1000` km):

```powershell
.\target\release\flight-tracker.exe --radius-km 300
```

Use imperial or metric distance units (imperial is the default):

```powershell
.\target\release\flight-tracker.exe --units imperial
.\target\release\flight-tracker.exe --units metric
```

Print structured JSON instead of the human-readable report:

```powershell
.\target\release\flight-tracker.exe --json
```

Show help:

```powershell
.\target\release\flight-tracker.exe --help
```

## Desktop Dashboard

Launch the native ops-board dashboard window:

```powershell
.\target\release\flight-tracker.exe --ui
```

Launching the built exe with no arguments at all (e.g. from a Start Menu tile or File Explorer, with no terminal attached) also opens the dashboard automatically.

The dashboard shows:

- A **NEAREST** flight-strip board listing the closest aircraft (up to five) with summary, operator, route, and motion columns. Click a row — or click a plane icon directly on the radar — to select it.
- A **radar scope** that plots all listed contacts by bearing and distance, with the selected aircraft highlighted and target-bracketed. Contacts drift smoothly between OpenSky refreshes, extrapolated from each aircraft's own last reported heading and speed. Selection persists across refreshes (by aircraft identity) even as the list re-sorts by distance.
- **Detail panels** for the selected aircraft: summary, operator, route (with the city pair, e.g. "Los Angeles → San Francisco"), registry, motion, and a live clock that ticks every second independent of the data refresh cycle.
- A header with **OVERVIEW / RAW REPORT** tabs, an **NM / KM** units toggle, and the last-refresh timestamp. The Raw Report tab lists a full text report for every tracked aircraft, not just the selected one.
- Times throughout the UI are shown in your system's local time zone.

The window works with the same location options as the CLI, e.g. explicit coordinates and a wider search radius:

```powershell
.\target\release\flight-tracker.exe --ui --lat 40.7128 --lon -74.0060 --radius-km 200
```

Only the selected aircraft is enriched with route/registry/operator detail on each refresh (to respect upstream rate limits); selecting another row enriches it on demand.

## OpenSky Credentials

Anonymous OpenSky access can be rate limited. To use OpenSky OAuth credentials:

```powershell
$env:OPENSKY_CLIENT_ID="..."
$env:OPENSKY_CLIENT_SECRET="..."
.\target\release\flight-tracker.exe
```

The credentials are read from environment variables and sent only to OpenSky's token endpoint.

## Privacy

This app calls third-party services. It uses the Windows Location API when `--lat`/`--lon` aren't provided (or a saved fallback location via `--set-location`), and sends the search bounding box to OpenSky, aircraft coordinates to Nominatim, aircraft identifiers/callsigns to ADSBDB, and U.S. N-numbers to the FAA registry when applicable. See `packaging/STORE_SUBMISSION.md` for the full privacy policy used in the Microsoft Store listing.

Use explicit coordinates if you do not want to use Windows Location Services:

```powershell
.\target\release\flight-tracker.exe --lat 40.7128 --lon -74.0060
```

## Security Notes

The executable does not use `eval`, shell execution, subprocess calls, or unsafe deserialization. All third-party API calls use HTTPS. HTTP response bodies are size-limited to reduce memory-exhaustion risk, and all externally-sourced display text (callsigns, operator names, registries, place names) is sanitized to strip control characters and Unicode bidi-override characters before being shown, since ADS-B and crowdsourced aviation data have no authentication and can't be assumed trustworthy. No secrets are stored in the repository; OpenSky OAuth credentials are read only from environment variables.

## Development

Run tests:

```powershell
cargo test --release
```

## Packaging & Distribution

- **`installer/`** — a WiX-based MSI installer (`build-installer.ps1`) for direct/side-loaded distribution, installing to `C:\Program Files\Flight Tracker` with Start Menu and Desktop shortcuts.
- **`packaging/`** — MSIX packaging for Microsoft Store submission (`build-msix.ps1`), producing a signed x64 + ARM64 `.msixbundle`. See `packaging/STORE_SUBMISSION.md` for the full submission checklist (Store listing copy, screenshots, privacy policy, age ratings, etc.).

## Limitations

- The closest aircraft is determined from live state-vector data, not schedule data.
- Operator and route data are best-effort enrichments and may be missing or stale.
- FAA registry lookups apply only to U.S. aircraft.
- Military identification is based on known callsign patterns and may be incomplete.
- API availability, rate limits, and OpenSky receiver coverage (see [Coverage](#coverage)) can affect results.
