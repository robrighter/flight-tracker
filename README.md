# Flight Tracker

Flight Tracker is a small app that finds the aircraft closest to you and explains what it knows about that plane in plain English.

The current distributable is a Rust CLI that compiles into a clean Windows executable. The original Python CLI/Tkinter implementation remains in `src/flight_tracker` for reference. The app estimates your location, searches nearby live aircraft positions, picks the closest aircraft, enriches it with public aircraft/route/registry data, and shows distance, heading, altitude, speed, nearest city, operator, and route when available.

## What It Shows

For the nearest aircraft, Flight Tracker can display:

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

Flight Tracker uses public web APIs and registries:

- **Windows Location API**: estimates your current PC location when `--lat` and `--lon` are not provided.
- **OpenSky Network**: provides live aircraft state vectors used to find nearby aircraft.
- **OpenStreetMap Nominatim**: reverse-geocodes the aircraft coordinates into a nearby city/place.
- **ADSBDB**: enriches aircraft and route data by ICAO24/callsign when available.
- **FAA Aircraft Registry**: provides registry details for U.S. aircraft when the aircraft Mode-S code maps to an N-number.

OpenSky's anonymous API is rate limited. You can optionally provide OpenSky OAuth credentials for a higher quota.

## Build The Windows EXE

Build a native Windows ARM64 executable:

```powershell
$env:CARGO_INCREMENTAL="0"
$env:CARGO_TARGET_DIR="$env:USERPROFILE\.codex\build\flight-tracker-target"
cargo build --release --target aarch64-pc-windows-msvc
```

The checked build artifact is copied to:

```text
dist/windows-arm64/flight-tracker.exe
```

If building directly from this WSL-mounted path, keep `CARGO_TARGET_DIR` on a normal Windows filesystem. Cargo incremental build locks can fail on `\\wsl.localhost` paths.

## Rust CLI Usage

Find the closest aircraft using the Windows PC location API:

```powershell
.\dist\windows-arm64\flight-tracker.exe
```

Use exact coordinates instead of Windows location:

```powershell
.\dist\windows-arm64\flight-tracker.exe --lat 40.7128 --lon -74.0060
```

If Windows cannot determine your location, save a fallback location in settings:

```powershell
.\dist\windows-arm64\flight-tracker.exe --set-location --lat 40.7128 --lon -74.0060 --location-label "Home"
```

View or clear that saved fallback:

```powershell
.\dist\windows-arm64\flight-tracker.exe --show-settings
.\dist\windows-arm64\flight-tracker.exe --clear-location
```

Search a wider area:

```powershell
.\dist\windows-arm64\flight-tracker.exe --radius-km 300
```

The search radius must be greater than `0` and no more than `1000` km.

Use imperial or metric distance units:

```powershell
.\dist\windows-arm64\flight-tracker.exe --units imperial
.\dist\windows-arm64\flight-tracker.exe --units metric
```

Imperial is the default.

Print structured JSON instead of the human-readable report:

```powershell
.\dist\windows-arm64\flight-tracker.exe --json
```

Show help:

```powershell
.\dist\windows-arm64\flight-tracker.exe --help
```

## Rust Desktop Dashboard

Launch the native ops-board dashboard window:

```powershell
.\dist\windows-arm64\flight-tracker.exe --ui
```

The dashboard is a single-instrument operations board inspired by mid-century
airport ops rooms (see `mockups/aviation-ui-ops-board.png`). It shows:

- A **NEAREST** flight-strip board listing the closest aircraft (up to five) with
  summary, operator, route, and motion columns. Click a row to focus it.
- A **radar scope** that plots all listed contacts by bearing and distance, with
  the focused aircraft highlighted and called out. The scope auto-scales to the
  farthest contact shown.
- **Detail panels** for the focused aircraft: summary, operator, route, registry,
  motion, and update time.
- A header with **OVERVIEW / RAW REPORT** tabs, an **NM / KM** units toggle, the
  last update time, and a **REFRESH** button that re-queries live traffic.

The window works with the same location options as the CLI, e.g. explicit
coordinates and a wider search radius:

```powershell
.\dist\windows-arm64\flight-tracker.exe --ui --lat 40.7128 --lon -74.0060 --radius-km 200
```

Only the focused aircraft is enriched with route/registry/operator detail (to
respect upstream rate limits); selecting another row enriches it on demand.

## Python Reference Install

From the project directory:

```bash
python3 -m pip install -e . --no-build-isolation
```

You can also run directly without installing:

```bash
PYTHONPATH=src python3 -m flight_tracker
```

## Python CLI Usage

Find the closest aircraft using IP-based location:

```bash
flight-tracker
```

Use exact coordinates instead of IP geolocation:

```bash
flight-tracker --lat 40.7128 --lon -74.0060
```

Search a wider area:

```bash
flight-tracker --radius-km 300
```

The search radius must be greater than `0` and no more than `1000` km.

Use imperial or metric distance units:

```bash
flight-tracker --units imperial
flight-tracker --units metric
```

Imperial is the default.

Print structured JSON instead of the human-readable report:

```bash
flight-tracker --json
```

Show help:

```bash
flight-tracker --help
```

## Python Desktop UI

Launch the optional desktop UI:

```bash
flight-tracker --ui
```

The UI shows an overview tab with:

- Airline/operator and origin-to-destination banner
- Plain-English summary
- Distance, bearing, altitude, and speed cards
- Location, operator, route, registry, motion, and timestamp panels
- A raw report tab for the full text output

The UI refreshes every 60 seconds by default:

```bash
flight-tracker --ui --refresh-seconds 30
```

The UI includes a units selector, so you can switch between imperial and metric without restarting.

If something unexpected happens in the UI, the app hides detailed exception text by default. Run with `--debug` when troubleshooting:

```bash
flight-tracker --ui --debug
```

## OpenSky Credentials

Anonymous OpenSky access can be rate limited. To use OpenSky OAuth credentials:

```bash
export OPENSKY_CLIENT_ID="..."
export OPENSKY_CLIENT_SECRET="..."
flight-tracker
```

The credentials are read from environment variables and sent only to OpenSky's token endpoint.

## Privacy

This app calls third-party services.

The Rust executable uses the Windows Location API when `--lat` and `--lon` are not provided. If Windows cannot determine your location, save a fallback location with `--set-location`. It sends the search bounding box to OpenSky, aircraft coordinates to Nominatim, aircraft identifiers/callsigns to ADSBDB, and U.S. N-numbers to the FAA registry when applicable.

Use explicit coordinates if you do not want to use Windows Location Services:

```powershell
.\dist\windows-arm64\flight-tracker.exe --lat 40.7128 --lon -74.0060
```

## Security Notes

The Rust executable does not use `eval`, shell execution, subprocess calls, or unsafe deserialization.

HTTP responses are size-limited to reduce memory-exhaustion risk, and upstream display text is sanitized to remove control characters before it is shown in the terminal or UI.

## Development

Run tests:

```powershell
$env:CARGO_INCREMENTAL="0"
$env:CARGO_TARGET_DIR="$env:USERPROFILE\.codex\build\flight-tracker-target"
cargo test
```

Run Python reference tests:

```bash
PYTHONPATH=src python3 -m unittest discover -s tests
```

Run a syntax check:

```bash
python3 -m py_compile src/flight_tracker/cli.py tests/test_cli.py
```

Run without installing:

```bash
PYTHONPATH=src python3 -m flight_tracker --help
```

## Limitations

- The closest aircraft is determined from live state-vector data, not schedule data.
- Operator and route data are best-effort enrichments and may be missing or stale.
- FAA registry lookups apply only to U.S. aircraft.
- Military identification is based on known callsign patterns and may be incomplete.
- API availability and rate limits can affect results.
