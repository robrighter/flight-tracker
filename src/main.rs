// Build as a GUI-subsystem app on Windows so launching the desktop dashboard
// never spawns a background console window. CLI runs re-attach to the parent
// terminal at startup (see `attach_parent_console`) so their output still shows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use chrono::{DateTime, Local, Timelike, Utc};
use clap::{Parser, ValueEnum};
use reqwest::blocking::{Client, Response};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::fmt::{self, Display};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
#[cfg(windows)]
use std::sync::Mutex;
use std::sync::OnceLock;
#[cfg(windows)]
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use windows::Devices::Geolocation::{Geolocator, PositionAccuracy};
#[cfg(windows)]
use windows::Foundation::TimeSpan;
#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC,
    DeleteObject, DrawTextW, Ellipse, EndPaint, FF_DONTCARE, FW_BOLD, FW_NORMAL, FillRect,
    GetStockObject, HDC, HGDIOBJ, HOLLOW_BRUSH, InvalidateRect, LineTo, MoveToEx, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, PS_SOLID, Polygon,
    RGBQUAD, SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, StretchDIBits,
    TRANSPARENT, UpdateWindow,
};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
#[cfg(windows)]
use windows::Win32::UI::Controls::MARGINS;
#[cfg(windows)]
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetClientRect, GetMessageW, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW,
    KillTimer, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_MINIMIZE, SW_SHOW, SetTimer,
    ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN,
    WM_MOUSEWHEEL, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_PAINT, WM_PRINTCLIENT,
    WM_SIZE, WM_TIMER, WNDCLASSW, WS_CAPTION,
    WS_MINIMIZEBOX, WS_SYSMENU, WS_VISIBLE,
};
#[cfg(windows)]
use windows::core::{PCWSTR, w};

const NOMINATIM_REVERSE_URL: &str = "https://nominatim.openstreetmap.org/reverse";
const ADSBDB_API_URL: &str = "https://api.adsbdb.com/v0";
const FAA_N_NUMBER_URL: &str = "https://registry.faa.gov/AircraftInquiry/Search/NNumberResult";
const OPENSKY_API_URL: &str = "https://opensky-network.org/api";
const OPENSKY_TOKEN_URL: &str =
    "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token";
const APP_USER_AGENT: &str = "flight-tracker-rust/0.1";
const EARTH_RADIUS_KM: f64 = 6371.0088;
const MPS_TO_KNOTS: f64 = 1.9438444924406;
const M_TO_FEET: f64 = 3.2808398950131;
const US_ICAO24_START: i64 = 0xA00001;
const US_ICAO24_COUNT: i64 = 915399;
const N_NUMBER_LETTERS: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ";
const MAX_RADIUS_KM: f64 = 1000.0;
const MAX_JSON_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const MAX_TEXT_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const SETTINGS_DIR_NAME: &str = "Flight Tracker";
const SETTINGS_FILE_NAME: &str = "settings.json";
#[cfg(windows)]
static UI_STATE: OnceLock<Mutex<UiState>> = OnceLock::new();
const UI_LIST_LIMIT: usize = 5;
#[cfg(windows)]
const DESIGN_WIDTH: i32 = 1672;
#[cfg(windows)]
const DESIGN_HEIGHT: i32 = 941;
#[cfg(windows)]
const WINDOW_WIDTH: i32 = 1170;
#[cfg(windows)]
const WINDOW_HEIGHT: i32 = 659;
// Undocumented messages Uxtheme/DWM send to force a caption/frame redraw
// outside the normal WM_NCPAINT/WM_NCACTIVATE flow; left unhandled they can
// paint a native titlebar over our borderless window regardless of the
// WM_NCCALCSIZE/WM_NCACTIVATE handling below.
#[cfg(windows)]
const WM_NCUAHDRAWCAPTION: u32 = 0x00AE;
#[cfg(windows)]
const WM_NCUAHDRAWFRAME: u32 = 0x00AF;
#[cfg(windows)]
const AUTO_REFRESH_TIMER_ID: usize = 1;
#[cfg(windows)]
const AUTO_REFRESH_INTERVAL_MS: u32 = 20_000;
#[cfg(windows)]
const RADAR_ANIMATION_TIMER_ID: usize = 2;
/// Redraw rate for extrapolated aircraft motion between OpenSky refreshes.
#[cfg(windows)]
const RADAR_ANIMATION_INTERVAL_MS: u32 = 100;
/// Cap on how far a contact's last known speed/track is trusted to
/// extrapolate before it's shown parked at its last reported position.
#[cfg(windows)]
const RADAR_EXTRAPOLATION_LIMIT_S: i64 = 90;
#[cfg(windows)]
const ASSET_BACKGROUND_PNG: &[u8] = include_bytes!("../assets/ui/mock-texture-background-70.png");
#[cfg(windows)]
const ASSET_SELECTED_ROW: &[u8] = include_bytes!("../assets/ui/selected-row-underlay-70.bmp");
#[cfg(windows)]
const ASSET_RAW_REPORT: &[u8] = include_bytes!("../assets/ui/raw-report-70.bmp");

const STATE_FIELDS: [&str; 17] = [
    "icao24",
    "callsign",
    "origin_country",
    "time_position",
    "last_contact",
    "longitude",
    "latitude",
    "baro_altitude",
    "on_ground",
    "velocity",
    "true_track",
    "vertical_rate",
    "sensors",
    "geo_altitude",
    "squawk",
    "spi",
    "position_source",
];

#[derive(Debug)]
struct FlightTrackerError(String);

impl Display for FlightTrackerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FlightTrackerError {}

type AppResult<T> = Result<T, FlightTrackerError>;

#[derive(Debug, Parser)]
#[command(
    name = "flight-tracker",
    about = "Find the nearest aircraft to your current location."
)]
struct Cli {
    #[arg(
        long,
        allow_hyphen_values = true,
        help = "Latitude in decimal degrees."
    )]
    lat: Option<f64>,
    #[arg(
        long,
        allow_hyphen_values = true,
        help = "Longitude in decimal degrees."
    )]
    lon: Option<f64>,
    #[arg(
        long,
        default_value_t = 150.0,
        help = "Search radius around the location."
    )]
    radius_km: f64,
    #[arg(
        long,
        default_value_t = 15.0,
        help = "HTTP request timeout in seconds."
    )]
    timeout: f64,
    #[arg(long, help = "Print machine-readable JSON instead of a text report.")]
    json: bool,
    #[arg(
        long,
        help = "Save the provided --lat/--lon as the fallback settings location and exit."
    )]
    set_location: bool,
    #[arg(long, help = "Optional label to save with --set-location.")]
    location_label: Option<String>,
    #[arg(long, help = "Print the saved fallback location and exit.")]
    show_settings: bool,
    #[arg(long, help = "Clear the saved fallback location and exit.")]
    clear_location: bool,
    #[arg(long, help = "Launch the native desktop dashboard window.")]
    ui: bool,
    #[arg(long, default_value_t = 60, help = "Desktop UI refresh interval hint.")]
    refresh_seconds: u64,
    #[arg(long, value_enum, default_value_t = Units::Imperial)]
    units: Units,
    #[arg(long, help = "Reserved for detailed future UI errors.")]
    debug: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Units {
    Metric,
    Imperial,
}

#[derive(Debug, Serialize, Clone)]
struct Location {
    latitude: f64,
    longitude: f64,
    label: String,
    source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SavedLocation {
    latitude: f64,
    longitude: f64,
    label: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Settings {
    location: Option<SavedLocation>,
}

#[derive(Debug, Serialize, Clone)]
struct Place {
    label: String,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
    source: String,
}

#[derive(Debug, Serialize, Clone, Default)]
struct FlightState {
    icao24: Option<String>,
    callsign: Option<String>,
    origin_country: Option<String>,
    time_position: Option<i64>,
    last_contact: Option<i64>,
    longitude: Option<f64>,
    latitude: Option<f64>,
    baro_altitude: Option<f64>,
    on_ground: Option<bool>,
    velocity: Option<f64>,
    true_track: Option<f64>,
    vertical_rate: Option<f64>,
    sensors: Option<Value>,
    geo_altitude: Option<f64>,
    squawk: Option<String>,
    spi: Option<bool>,
    position_source: Option<i64>,
    category: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
struct Classification {
    #[serde(rename = "type")]
    kind: String,
    operator: Option<String>,
    country: Option<String>,
    confidence: String,
}

#[derive(Debug, Serialize, Clone)]
struct NearestAircraft {
    distance_km: f64,
    bearing_degrees: f64,
    state: FlightState,
    nearest_place: Option<Place>,
    metadata: Option<Value>,
    faa: Option<BTreeMap<String, String>>,
    classification: Classification,
    api_time: Option<i64>,
}

#[derive(Debug, Serialize)]
struct JsonOutput {
    location: Location,
    nearest_aircraft: NearestAircraft,
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct UiModel {
    callsign: String,
    operator: String,
    /// Compact airport codes for the ROUTE panel, e.g. "KLAX → KSFO".
    route_codes: String,
    /// From/destination city pair, e.g. "Los Angeles → San Francisco".
    route_cities: String,
    summary: String,
    altitude: String,
    heading: String,
    registry: String,
    aircraft_type: String,
    updated: String,
    /// Live values behind the MOTION gauges (None → no needle / dashes).
    track_deg: Option<f64>,
    speed_kt: Option<f64>,
    vertical_fpm: Option<f64>,
    /// UPDATED panel: date line ("20 MAY 2025") and clock-hand positions
    /// (hour in [0,12), minute in [0,60), second in [0,60)).
    date: String,
    clock: Option<(f64, f64, f64)>,
}

#[cfg(windows)]
impl UiModel {
    fn from_flight(
        _location: &Location,
        nearest: &NearestAircraft,
        units: Units,
        is_nearest: bool,
    ) -> Self {
        let state = &nearest.state;
        let metadata = nearest.metadata.as_ref();
        let aircraft = nested_object(metadata, &["aircraft"]);
        let flightroute = nested_object(metadata, &["flightroute"]);
        let registry = aircraft
            .and_then(|aircraft| object_str(Some(aircraft), "registration"))
            .or_else(|| {
                nearest
                    .faa
                    .as_ref()
                    .and_then(|faa| faa.get("n_number").cloned())
            })
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let aircraft_type = aircraft
            .and_then(|aircraft| object_str(Some(aircraft), "type"))
            .or_else(|| {
                nearest
                    .faa
                    .as_ref()
                    .and_then(|faa| faa.get("model").cloned())
            })
            .unwrap_or_else(|| "Unknown aircraft".to_string());
        let operator = nearest
            .classification
            .operator
            .clone()
            .unwrap_or_else(|| "Operator unknown".to_string());
        let updated = nearest
            .api_time
            .map(|timestamp| format_timestamp(Some(timestamp)))
            .unwrap_or_else(|| "unknown".to_string());
        let callsign = state
            .callsign
            .clone()
            .map(|callsign| callsign.trim().to_string())
            .filter(|callsign| !callsign.is_empty())
            .unwrap_or_else(|| "NO CALLSIGN".to_string());
        // `units` currently affects distance/summary phrasing only; motion is
        // shown in standard aviation units (knots, flight level) like the mock.
        let _ = units;

        let route_codes = format_route_codes_for_ui(flightroute);
        let route_cities = format_route_cities_for_ui(flightroute);
        let (date, clock) = match nearest
            .api_time
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
            .map(|time| time.with_timezone(&Local))
        {
            Some(time) => (
                time.format("%d %b %Y").to_string().to_uppercase(),
                Some((
                    (time.hour() % 12) as f64 + time.minute() as f64 / 60.0,
                    time.minute() as f64 + time.second() as f64 / 60.0,
                    time.second() as f64,
                )),
            ),
            None => (String::new(), None),
        };

        Self {
            callsign,
            operator,
            route_codes,
            route_cities,
            summary: ui_summary_card(nearest, units, is_nearest),
            altitude: ui_flight_level(state.baro_altitude),
            heading: ui_heading(state.true_track),
            registry,
            aircraft_type,
            updated,
            track_deg: state.true_track,
            speed_kt: state.velocity.map(|mps| mps * MPS_TO_KNOTS),
            vertical_fpm: state.vertical_rate.map(|mps| mps * M_TO_FEET * 60.0),
            date,
            clock,
        }
    }
}

#[derive(Clone)]
struct ApiClient {
    client: Client,
}

impl ApiClient {
    fn new(timeout: f64) -> AppResult<Self> {
        let timeout = if timeout > 0.0 { timeout } else { 15.0 };
        let client = Client::builder()
            .timeout(Duration::from_secs_f64(timeout))
            .user_agent(APP_USER_AGENT)
            .build()
            .map_err(|error| {
                FlightTrackerError(format!("could not create HTTP client: {error}"))
            })?;
        Ok(Self { client })
    }

    fn opensky_get_json(&self, path: &str, params: Vec<(&str, String)>) -> AppResult<Value> {
        let url = format!("{OPENSKY_API_URL}{path}");
        let mut request = self.client.get(&url).query(&params);

        if let (Ok(client_id), Ok(client_secret)) = (
            env::var("OPENSKY_CLIENT_ID"),
            env::var("OPENSKY_CLIENT_SECRET"),
        ) {
            if !client_id.is_empty() && !client_secret.is_empty() {
                let token = self.fetch_opensky_token(&client_id, &client_secret)?;
                request = request.bearer_auth(token);
            }
        }

        let response = request
            .send()
            .map_err(|error| FlightTrackerError(format!("could not reach {url}: {error}")))?;
        read_json_response(response, &url)
    }

    fn fetch_opensky_token(&self, client_id: &str, client_secret: &str) -> AppResult<String> {
        let body = [
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];
        let response = self
            .client
            .post(OPENSKY_TOKEN_URL)
            .form(&body)
            .send()
            .map_err(|error| {
                FlightTrackerError(format!("could not reach {OPENSKY_TOKEN_URL}: {error}"))
            })?;
        let data = read_json_response(response, OPENSKY_TOKEN_URL)?;
        data.get("access_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                FlightTrackerError(
                    "OpenSky authentication did not return an access token".to_string(),
                )
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    RawReport,
}

/// Mutable state shared with the native dashboard window. The window proc reads
/// it while painting and mutates it in response to clicks (tab/units toggles,
/// row selection, refresh).
struct UiState {
    client: ApiClient,
    location: Location,
    radius_km: f64,
    units: Units,
    tab: Tab,
    aircraft: Vec<NearestAircraft>,
    enriched: Vec<bool>,
    selected: usize,
    report: String,
    raw_scroll_lines: usize,
    status: String,
    refreshing: bool,
}

impl UiState {
    fn new(
        client: ApiClient,
        location: Location,
        radius_km: f64,
        units: Units,
        aircraft: Vec<NearestAircraft>,
    ) -> Self {
        let has_aircraft = !aircraft.is_empty();
        let enriched = vec![false; aircraft.len()];
        let mut state = Self {
            client,
            location,
            radius_km,
            units,
            tab: Tab::Overview,
            aircraft,
            enriched,
            selected: 0,
            report: String::new(),
            raw_scroll_lines: 0,
            status: if has_aircraft {
                "LIVE ADS-B".to_string()
            } else {
                "WAITING FOR ADS-B".to_string()
            },
            refreshing: false,
        };
        state.rebuild_report();
        state
    }

    fn selected_aircraft(&self) -> Option<&NearestAircraft> {
        self.aircraft.get(self.selected)
    }

    /// Enrich the selected aircraft with place/route/registry/operator data on
    /// demand, so the bottom panels and radar callout have full detail without
    /// paying that cost for every contact in the list.
    fn enrich_selected(&mut self) {
        let index = self.selected;
        if self.enriched.get(index).copied().unwrap_or(true) {
            return;
        }
        if let Some(aircraft) = self.aircraft.get(index) {
            let mut enriched = aircraft.clone();
            enrich_aircraft(&mut enriched, &self.client);
            self.aircraft[index] = enriched;
            self.enriched[index] = true;
        }
    }

    fn select(&mut self, index: usize) -> bool {
        if index >= self.aircraft.len() || index == self.selected {
            return false;
        }
        self.selected = index;
        self.raw_scroll_lines = 0;
        self.rebuild_report();
        true
    }

    fn set_units(&mut self, units: Units) {
        self.units = units;
        self.raw_scroll_lines = 0;
        self.rebuild_report();
    }

    fn rebuild_report(&mut self) {
        self.report = match self.selected_aircraft() {
            Some(aircraft) => build_report(&self.location, aircraft, self.radius_km, self.units),
            None => "No aircraft in range.".to_string(),
        };
    }

    /// Re-query OpenSky for the current location and rebuild the contact list.
    /// Runs synchronously on the UI thread (the message loop is briefly blocked,
    /// the same as the initial fetch before the window opens).
    fn refresh(&mut self) {
        match find_nearby_aircraft(&self.location, self.radius_km, &self.client, UI_LIST_LIMIT) {
            Ok(aircraft) if !aircraft.is_empty() => {
                self.aircraft = aircraft;
                self.enriched = vec![false; self.aircraft.len()];
                self.selected = 0;
                self.status = "LIVE ADS-B".to_string();
                self.enrich_selected();
                self.rebuild_report();
            }
            Ok(_) => {
                self.status = "NO CONTACTS".to_string();
            }
            Err(error) => {
                self.status = format!("REFRESH FAILED: {error}");
            }
        }
    }
}

/// Re-attach to the launching terminal's console, if there is one. Because the
/// binary is built for the Windows GUI subsystem (no auto-allocated console),
/// this is what lets CLI output appear when the tool is run from a shell, while
/// a double-click / `Start-Process` launch of the dashboard shows no terminal.
#[cfg(windows)]
fn attach_parent_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_parent_console();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> AppResult<()> {
    let args = Cli::parse();

    if args.show_settings {
        return show_settings();
    }

    if args.clear_location {
        let mut settings = load_settings()?;
        settings.location = None;
        save_settings(&settings)?;
        println!("Cleared saved fallback location.");
        return Ok(());
    }

    if args.set_location {
        return set_location(&args);
    }

    if args.ui {
        if args.json {
            return Err(FlightTrackerError(
                "--ui and --json cannot be used together".to_string(),
            ));
        }
        let client = ApiClient::new(args.timeout)?;
        let mut location = resolve_location(&args, &client)?;
        // Give the observer's own location a human-readable city label for the
        // status bar (Windows/CLI labels are generic); keep coords on failure.
        if let Some(place) = reverse_geocode(location.latitude, location.longitude, &client) {
            location.label = match (place.city, place.region) {
                (Some(city), Some(region)) => format!("{city}, {region}"),
                (Some(city), None) => city,
                _ => place.label,
            };
        }
        let aircraft = Vec::new();
        let state = UiState::new(client, location, args.radius_km, args.units, aircraft);
        return run_dashboard_window(state);
    }

    let client = ApiClient::new(args.timeout)?;
    let location = resolve_location(&args, &client)?;
    let nearest = find_nearest_aircraft(&location, args.radius_km, &client)?;

    if args.json {
        let output = JsonOutput {
            location,
            nearest_aircraft: nearest,
        };
        let json = serde_json::to_string_pretty(&output)
            .map_err(|error| FlightTrackerError(format!("could not serialize JSON: {error}")))?;
        println!("{json}");
    } else {
        print!(
            "{}",
            build_report(&location, &nearest, args.radius_km, args.units)
        );
    }

    Ok(())
}

fn resolve_location(args: &Cli, _client: &ApiClient) -> AppResult<Location> {
    match (args.lat, args.lon) {
        (Some(latitude), Some(longitude)) => {
            validate_lat_lon(latitude, longitude)?;
            Ok(Location {
                latitude,
                longitude,
                label: "provided coordinates".to_string(),
                source: "cli".to_string(),
            })
        }
        (Some(_), None) | (None, Some(_)) => Err(FlightTrackerError(
            "--lat and --lon must be provided together".to_string(),
        )),
        (None, None) => {
            if let Ok(location) = resolve_windows_location(args.timeout) {
                return Ok(location);
            }

            if let Some(saved) = load_settings()?.location {
                validate_lat_lon(saved.latitude, saved.longitude)?;
                return Ok(Location {
                    latitude: saved.latitude,
                    longitude: saved.longitude,
                    label: saved.label,
                    source: "settings".to_string(),
                });
            }

            Err(FlightTrackerError(
                "could not determine this PC's location from Windows Location API, and no fallback location is saved. Enable Location Services for this app in Windows settings, or save a fallback with: flight-tracker --set-location --lat <latitude> --lon <longitude> --location-label \"Home\"".to_string(),
            ))
        }
    }
}

fn set_location(args: &Cli) -> AppResult<()> {
    let (Some(latitude), Some(longitude)) = (args.lat, args.lon) else {
        return Err(FlightTrackerError(
            "--set-location requires --lat and --lon".to_string(),
        ));
    };
    validate_lat_lon(latitude, longitude)?;

    let mut settings = load_settings()?;
    let label = args
        .location_label
        .clone()
        .unwrap_or_else(|| "saved fallback location".to_string());
    settings.location = Some(SavedLocation {
        latitude,
        longitude,
        label: label.clone(),
    });
    save_settings(&settings)?;
    println!("Saved fallback location: {label} ({latitude:.5}, {longitude:.5})");
    Ok(())
}

fn show_settings() -> AppResult<()> {
    let settings = load_settings()?;
    if let Some(location) = settings.location {
        println!("Saved fallback location");
        println!("Label: {}", location.label);
        println!(
            "Coordinates: {:.5}, {:.5}",
            location.latitude, location.longitude
        );
        println!("Settings file: {}", settings_path()?.display());
    } else {
        println!("No fallback location is saved.");
        println!("Settings file: {}", settings_path()?.display());
    }
    Ok(())
}

fn load_settings() -> AppResult<Settings> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|error| {
        FlightTrackerError(format!(
            "could not read settings file {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&content).map_err(|error| {
        FlightTrackerError(format!(
            "could not parse settings file {}: {error}",
            path.display()
        ))
    })
}

fn save_settings(settings: &Settings) -> AppResult<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            FlightTrackerError(format!(
                "could not create settings directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| FlightTrackerError(format!("could not serialize settings: {error}")))?;
    std::fs::write(&path, content).map_err(|error| {
        FlightTrackerError(format!(
            "could not write settings file {}: {error}",
            path.display()
        ))
    })
}

fn settings_path() -> AppResult<PathBuf> {
    let base = env::var_os("APPDATA")
        .or_else(|| env::var_os("LOCALAPPDATA"))
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            FlightTrackerError(
                "could not find APPDATA, LOCALAPPDATA, or USERPROFILE for settings".to_string(),
            )
        })?;
    Ok(base.join(SETTINGS_DIR_NAME).join(SETTINGS_FILE_NAME))
}

/// Window geometry recomputed on every paint and click so the draw code and the
/// hit-testing code never drift apart.
#[cfg(windows)]
struct Layout {
    width: i32,
    height: i32,
    overview_tab: RECT,
    raw_tab: RECT,
    nm_btn: RECT,
    km_btn: RECT,
    minimize_btn: RECT,
    close_btn: RECT,
    main: RECT,
    radar: RECT,
    bottom: RECT,
    board_clip: RECT,
    row_y: i32,
    row_h: i32,
}

#[cfg(windows)]
impl Layout {
    fn sx(&self, value: i32) -> i32 {
        scale_x(self.width, value)
    }

    fn sy(&self, value: i32) -> i32 {
        scale_y(self.height, value)
    }

    fn font(&self, size: i32) -> i32 {
        scale_font(self.width, size)
    }

    fn row_rect(&self, index: usize) -> RECT {
        let top = self.row_y + self.row_h * index as i32;
        rect_xy(
            self.board_clip.left,
            top,
            self.board_clip.right,
            top + self.row_h,
        )
    }
}

#[cfg(windows)]
fn compute_layout(width: i32, height: i32) -> Layout {
    let sx = |value: i32| scale_x(width, value);
    let sy = |value: i32| scale_y(height, value);

    let board_clip = rect_xy(sx(32), sy(134), sx(910), sy(648));
    let header_y = board_clip.top + sy(58);
    let row_y = header_y + sy(34);

    Layout {
        width,
        height,
        overview_tab: rect_xy(sx(553), sy(15), sx(738), sy(64)),
        raw_tab: rect_xy(sx(739), sy(15), sx(938), sy(64)),
        nm_btn: rect_xy(sx(1084), sy(16), sx(1162), sy(63)),
        km_btn: rect_xy(sx(1163), sy(16), sx(1236), sy(63)),
        minimize_btn: rect_xy(sx(1547), sy(19), sx(1597), sy(54)),
        close_btn: rect_xy(sx(1599), sy(19), sx(1648), sy(54)),
        main: rect_xy(sx(14), sy(78), sx(1659), sy(868)),
        radar: rect_xy(sx(933), sy(78), sx(1660), sy(674)),
        bottom: rect_xy(sx(14), sy(686), sx(1659), sy(868)),
        board_clip,
        row_y,
        row_h: sy(84).max(1),
    }
}

/// Clip the frameless window to a rounded rectangle so its corners match the
/// rounded frame painted into the background art (otherwise the square window
/// shows the art's dark corner pixels). The radius tracks the art: ~14px in the
/// 1672-wide design space, scaled to the live client size.
#[cfg(windows)]
fn apply_rounded_region(hwnd: HWND, width: i32, height: i32) {
    let diameter = (2 * 14 * width / DESIGN_WIDTH).max(2);
    unsafe {
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
        // The window owns the region after this call; Windows frees it.
        let _ = SetWindowRgn(hwnd, Some(region), true);
    }
}

/// Returns true when `point` falls inside `rect`.
#[cfg(windows)]
fn point_in(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

#[cfg(windows)]
fn run_dashboard_window(state: UiState) -> AppResult<()> {
    if UI_STATE.set(Mutex::new(state)).is_err() {
        return Err(FlightTrackerError(
            "dashboard state was already initialized".to_string(),
        ));
    }
    unsafe {
        let module = GetModuleHandleW(None)
            .map_err(|error| FlightTrackerError(format!("could not get module handle: {error}")))?;
        let instance = HINSTANCE(module.0);
        let class_name = w!("FlightTrackerDashboardWindow");
        let cursor = LoadCursorW(None, IDC_ARROW)
            .map_err(|error| FlightTrackerError(format!("could not load cursor: {error}")))?;
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(dashboard_window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&window_class);

        let title = to_wide("Flight Tracker");
        // A real captioned/sizable style (rather than WS_POPUP) so DWM treats
        // this as a normal top-level window and animates minimize/restore;
        // WM_NCCALCSIZE below strips the drawn frame so it still looks
        // borderless.
        let window_style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE;
        let window_ex_style = WINDOW_EX_STYLE(0);
        let hwnd = CreateWindowExW(
            window_ex_style,
            class_name,
            PCWSTR(title.as_ptr()),
            window_style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|error| FlightTrackerError(format!("could not create window: {error}")))?;

        // Tells DWM this window has "some" composited frame even though the
        // client area covers it entirely (WM_NCCALCSIZE above). Without this,
        // DWM occasionally falls back to drawing its own native titlebar over
        // ours during animations/thumbnails despite the message handling.
        let _ = DwmExtendFrameIntoClientArea(
            hwnd,
            &MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 1,
                cyBottomHeight: 0,
            },
        );

        let timer_id = SetTimer(
            Some(hwnd),
            AUTO_REFRESH_TIMER_ID,
            AUTO_REFRESH_INTERVAL_MS,
            None,
        );
        if timer_id == 0 {
            return Err(FlightTrackerError(
                "could not start dashboard refresh timer".to_string(),
            ));
        }
        let _ = SetTimer(
            Some(hwnd),
            RADAR_ANIMATION_TIMER_ID,
            RADAR_ANIMATION_INTERVAL_MS,
            None,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        apply_rounded_region(hwnd, WINDOW_WIDTH, WINDOW_HEIGHT);
        let _ = UpdateWindow(hwnd);
        refresh_dashboard(hwnd, true);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_dashboard_window(_state: UiState) -> AppResult<()> {
    Err(FlightTrackerError(
        "the native window is only available in the Windows executable".to_string(),
    ))
}

#[cfg(windows)]
extern "system" fn dashboard_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_PAINT => {
                paint_dashboard(hwnd);
                LRESULT(0)
            }
            // DWM sends this when it needs a bitmap of our content outside of
            // normal painting (minimize/restore animation, taskbar/Alt-Tab
            // thumbnails, Aero Peek). Without a handler it falls back to a
            // generic titlebar-shaped placeholder instead of our real UI.
            WM_PRINTCLIENT => {
                let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
                render_dashboard_into(hwnd, hdc);
                LRESULT(0)
            }
            // We repaint the whole client area from a back buffer, so suppress
            // the default background erase to avoid flicker.
            WM_ERASEBKGND => LRESULT(1),
            // We have no nonclient area to redraw (WM_NCCALCSIZE below
            // collapses it to zero), so skip the default caption repaint on
            // activate/deactivate — passing lParam -1 tells DefWindowProc not
            // to redraw the nonclient area, avoiding a caption-frame flash.
            WM_NCACTIVATE => DefWindowProcW(hwnd, message, wparam, LPARAM(-1)),
            // We have no nonclient area at all (see WM_NCCALCSIZE below), so
            // never let the default handler paint one — covers stray
            // WM_NCPAINT dispatches DWM sends outside the activate/animation
            // paths already handled above.
            WM_NCPAINT => LRESULT(0),
            // Uxtheme/DWM-internal, not in the public message table; skip
            // default handling entirely rather than let it draw a caption.
            WM_NCUAHDRAWCAPTION | WM_NCUAHDRAWFRAME => LRESULT(0),
            WM_SIZE => {
                let mut rect = RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok() {
                    apply_rounded_region(hwnd, rect.right - rect.left, rect.bottom - rect.top);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            // Collapse the non-client frame to zero so the real WS_CAPTION
            // frame (kept for DWM's benefit — shadow, minimize/restore
            // animation) never actually draws a titlebar/border.
            WM_NCCALCSIZE => {
                if wparam.0 != 0 {
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
            WM_NCHITTEST => hit_test_frameless_window(hwnd, lparam),
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                handle_click(hwnd, x, y);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                handle_mouse_wheel(hwnd, wparam);
                LRESULT(0)
            }
            WM_TIMER => {
                if wparam.0 == AUTO_REFRESH_TIMER_ID {
                    refresh_dashboard(hwnd, false);
                    LRESULT(0)
                } else if wparam.0 == RADAR_ANIMATION_TIMER_ID {
                    animate_radar_tick(hwnd);
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), AUTO_REFRESH_TIMER_ID);
                let _ = KillTimer(Some(hwnd), RADAR_ANIMATION_TIMER_ID);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

#[cfg(windows)]
fn hit_test_frameless_window(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut window = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window) }.is_err() {
        return unsafe { DefWindowProcW(hwnd, WM_NCHITTEST, WPARAM(0), lparam) };
    }
    let x = screen_x - window.left;
    let y = screen_y - window.top;
    let layout = compute_layout(WINDOW_WIDTH, WINDOW_HEIGHT);
    let over_control = point_in(layout.overview_tab, x, y)
        || point_in(layout.raw_tab, x, y)
        || point_in(layout.nm_btn, x, y)
        || point_in(layout.km_btn, x, y)
        || point_in(layout.minimize_btn, x, y)
        || point_in(layout.close_btn, x, y);

    if y < scale_y(WINDOW_HEIGHT, 74) && !over_control {
        LRESULT(HTCAPTION as isize)
    } else {
        LRESULT(HTCLIENT as isize)
    }
}

/// Translate a click in the client area into a state change (tab/units/refresh
/// or row selection) and repaint if anything changed.
#[cfg(windows)]
fn handle_click(hwnd: HWND, x: i32, y: i32) {
    let Some(state_mutex) = UI_STATE.get() else {
        return;
    };
    let Ok(mut state) = state_mutex.lock() else {
        return;
    };

    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
        return;
    }
    let width = (client.right - client.left).max(1);
    let height = (client.bottom - client.top).max(1);
    let layout = compute_layout(width, height);

    if point_in(layout.close_btn, x, y) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return;
    } else if point_in(layout.minimize_btn, x, y) {
        let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        return;
    }

    let mut changed = true;
    let mut enrich_index = None;
    if point_in(layout.overview_tab, x, y) {
        state.tab = Tab::Overview;
    } else if point_in(layout.raw_tab, x, y) {
        state.tab = Tab::RawReport;
        state.raw_scroll_lines = 0;
    } else if point_in(layout.nm_btn, x, y) {
        state.set_units(Units::Imperial);
    } else if point_in(layout.km_btn, x, y) {
        state.set_units(Units::Metric);
    } else if state.tab == Tab::Overview {
        let mut hit = false;
        for index in 0..state.aircraft.len().min(UI_LIST_LIMIT) {
            if point_in(layout.row_rect(index), x, y) {
                if state.select(index) {
                    enrich_index = Some(index);
                }
                hit = true;
                break;
            }
        }
        if !hit {
            if let Some(index) = radar_hit_test(&layout, &state, x, y) {
                if state.select(index) {
                    enrich_index = Some(index);
                }
                hit = true;
            }
        }
        changed = hit;
    } else {
        changed = false;
    }

    if false {
        // Show a "refreshing" status immediately, then perform the blocking
        // network call and repaint with the fresh contacts.
        state.status = "REFRESHING…".to_string();
        drop(state);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        let _ = unsafe { UpdateWindow(hwnd) };
        if let Ok(mut state) = state_mutex.lock() {
            state.refresh();
        }
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        return;
    }

    drop(state);
    if changed {
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
    if let Some(index) = enrich_index {
        enrich_selected_async(hwnd, index);
    }
}

#[cfg(windows)]
fn handle_mouse_wheel(hwnd: HWND, wparam: WPARAM) {
    let Some(state_mutex) = UI_STATE.get() else {
        return;
    };
    let Ok(mut state) = state_mutex.lock() else {
        return;
    };
    if state.tab != Tab::RawReport {
        return;
    }

    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
        return;
    }
    let width = (client.right - client.left).max(1);
    let height = (client.bottom - client.top).max(1);
    let layout = compute_layout(width, height);
    let max_scroll = raw_report_max_scroll_lines(&layout, &state.report);
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let notches = (delta.abs() / 120).max(1) as usize;
    let step = notches * 3;
    let current = state.raw_scroll_lines;
    state.raw_scroll_lines = if delta > 0 {
        state.raw_scroll_lines.saturating_sub(step)
    } else {
        (state.raw_scroll_lines + step).min(max_scroll)
    };
    let changed = state.raw_scroll_lines != current;
    drop(state);
    if changed {
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
}

#[cfg(windows)]
fn enrich_selected_async(hwnd: HWND, index: usize) {
    let Some(state_mutex) = UI_STATE.get() else {
        return;
    };
    let (client, mut aircraft, icao24) = {
        let Ok(mut state) = state_mutex.lock() else {
            return;
        };
        if state.enriched.get(index).copied().unwrap_or(true) {
            return;
        }
        let Some(aircraft) = state.aircraft.get(index).cloned() else {
            return;
        };
        state.status = "LOOKING UP DETAILS".to_string();
        (
            state.client.clone(),
            aircraft.clone(),
            aircraft.state.icao24,
        )
    };

    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    let hwnd_value = hwnd.0 as isize;
    thread::spawn(move || {
        enrich_aircraft(&mut aircraft, &client);
        if let Some(state_mutex) = UI_STATE.get() {
            if let Ok(mut state) = state_mutex.lock() {
                let same_aircraft = state
                    .aircraft
                    .get(index)
                    .map(|current| current.state.icao24 == icao24)
                    .unwrap_or(false);
                if same_aircraft {
                    state.aircraft[index] = aircraft;
                    if let Some(enriched) = state.enriched.get_mut(index) {
                        *enriched = true;
                    }
                    if state.selected == index {
                        state.status = "LIVE ADS-B".to_string();
                        state.rebuild_report();
                    }
                }
            }
        }
        let hwnd = HWND(hwnd_value as *mut std::ffi::c_void);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    });
}

/// Repaint on the fast animation timer so radar contacts visibly drift along
/// their track between OpenSky refreshes. No interpolation state is kept —
/// draw_radar recomputes each contact's extrapolated position from its own
/// last-known speed/track against the wall clock on every tick.
#[cfg(windows)]
fn animate_radar_tick(hwnd: HWND) {
    let Some(state_mutex) = UI_STATE.get() else {
        return;
    };
    let Ok(state) = state_mutex.lock() else {
        return;
    };
    if state.tab != Tab::Overview || state.aircraft.is_empty() {
        return;
    }
    drop(state);
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

#[cfg(windows)]
fn refresh_dashboard(hwnd: HWND, show_status: bool) {
    let Some(state_mutex) = UI_STATE.get() else {
        return;
    };
    let (client, location, radius_km) = {
        let Ok(mut state) = state_mutex.lock() else {
            return;
        };
        if state.refreshing {
            return;
        }
        state.refreshing = true;
        if show_status {
            state.status = "REFRESHING".to_string();
        }
        (
            state.client.clone(),
            state.location.clone(),
            state.radius_km,
        )
    };

    if show_status {
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
        let _ = unsafe { UpdateWindow(hwnd) };
    }

    let hwnd_value = hwnd.0 as isize;
    thread::spawn(move || {
        let result = match find_nearby_aircraft(&location, radius_km, &client, UI_LIST_LIMIT) {
            Ok(mut aircraft) if !aircraft.is_empty() => {
                let mut enriched = vec![false; aircraft.len()];
                for aircraft in &mut aircraft {
                    enrich_aircraft_table_details(aircraft, &client);
                }
                if let Some(selected) = aircraft.get_mut(0) {
                    enrich_aircraft(selected, &client);
                    enriched[0] = true;
                }
                Ok(Some((aircraft, enriched)))
            }
            Ok(_) => Ok(None),
            Err(error) => Err(error),
        };
        if let Some(state_mutex) = UI_STATE.get() {
            if let Ok(mut state) = state_mutex.lock() {
                match result {
                    Ok(Some((aircraft, enriched))) => {
                        state.aircraft = aircraft;
                        state.enriched = enriched;
                        state.selected = 0;
                        state.raw_scroll_lines = 0;
                        state.status = "LIVE ADS-B".to_string();
                        state.rebuild_report();
                    }
                    Ok(None) => {
                        state.status = "NO CONTACTS".to_string();
                    }
                    Err(error) => {
                        state.status = format!("REFRESH FAILED: {error}");
                    }
                }
                state.refreshing = false;
            }
        }
        let hwnd = HWND(hwnd_value as *mut std::ffi::c_void);
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    });
}

/// Render the dashboard into an arbitrary target HDC (the window's own DC on
/// WM_PAINT, or a DC DWM hands us on WM_PRINTCLIENT so it has real content to
/// composite into the minimize/restore animation and thumbnails instead of
/// falling back to a generic titlebar placeholder).
#[cfg(windows)]
fn render_dashboard_into(hwnd: HWND, hdc: HDC) {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        // Draw the frame into an off-screen bitmap, then blit it in one pass so
        // the now-frequent repaints (clicks, resizes, refresh) don't flicker.
        unsafe {
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let bitmap = CreateCompatibleBitmap(hdc, width, height);
            let old_bitmap = SelectObject(mem_dc, HGDIOBJ::from(bitmap));
            if let Some(state_mutex) = UI_STATE.get() {
                if let Ok(state) = state_mutex.lock() {
                    draw_dashboard(mem_dc, rect, &state);
                }
            }
            let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
            let _ = SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(HGDIOBJ::from(bitmap));
            let _ = DeleteDC(mem_dc);
        }
    }
}

#[cfg(windows)]
fn paint_dashboard(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    render_dashboard_into(hwnd, hdc);
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

#[cfg(windows)]
fn draw_dashboard(hdc: HDC, rect: RECT, state: &UiState) {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let layout = compute_layout(width, height);
        let shell = rect_xy(0, 0, width, height);
        fill_rect(hdc, shell, rgb(8, 18, 27));
        draw_png_background_1x(hdc);
        draw_tab_state(hdc, &layout, state.tab);
        draw_unit_toggle(hdc, &layout, state.units);

        let model = state.selected_aircraft().map(|aircraft| {
            UiModel::from_flight(&state.location, aircraft, state.units, state.selected == 0)
        });

        let updated = match &model {
            Some(model) => compact_time(&model.updated),
            None => state.status.clone(),
        };
        draw_text(
            hdc,
            &updated,
            scale_rect(width, height, 1362, 18, 1498, 54),
            scale_font(width, 18),
            true,
            rgb(240, 226, 196),
            Align::Left,
        );

        match state.tab {
            Tab::Overview => {
                draw_board(hdc, &layout, state);
                draw_radar(hdc, &layout, state);
                draw_bottom_panels(hdc, &layout, model.as_ref(), &state.status);
            }
            Tab::RawReport => {
                draw_raw_report(hdc, &layout, &state.report, state.raw_scroll_lines);
            }
        }

        draw_status_bar(hdc, &layout, &state.location, &state.status);
    }
}

#[cfg(windows)]
fn draw_tab_state(hdc: HDC, layout: &Layout, active: Tab) {
    draw_tab(
        hdc,
        layout,
        layout.overview_tab,
        "OVERVIEW",
        active == Tab::Overview,
    );
    draw_tab(
        hdc,
        layout,
        layout.raw_tab,
        "RAW REPORT",
        active == Tab::RawReport,
    );
}

#[cfg(windows)]
fn draw_tab(hdc: HDC, layout: &Layout, rect: RECT, label: &str, active: bool) {
    let fill = if active {
        rgb(126, 18, 16)
    } else {
        rgb(8, 21, 31)
    };
    let top = if active {
        rgb(184, 42, 33)
    } else {
        rgb(30, 55, 70)
    };
    let bottom = if active {
        rgb(62, 12, 12)
    } else {
        rgb(2, 8, 13)
    };
    let border = rgb(82, 96, 102);
    fill_rect(hdc, rect, fill);
    draw_line(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.top,
        top,
        scale_len(layout.width, 2),
    );
    draw_line(
        hdc,
        rect.left,
        rect.bottom - 1,
        rect.right,
        rect.bottom - 1,
        bottom,
        scale_len(layout.width, 2),
    );
    draw_line(
        hdc,
        rect.left,
        rect.top,
        rect.left,
        rect.bottom,
        border,
        scale_len(layout.width, 1),
    );
    draw_line(
        hdc,
        rect.right - 1,
        rect.top,
        rect.right - 1,
        rect.bottom,
        border,
        scale_len(layout.width, 1),
    );
    draw_text(
        hdc,
        label,
        inset(rect, layout.sx(12), layout.sy(8)),
        layout.font(19),
        true,
        if active {
            rgb(246, 228, 199)
        } else {
            rgb(184, 181, 170)
        },
        Align::Center,
    );
}

/// One contact rendered into the NEAREST flight-strip board.
#[cfg(windows)]
struct RowView {
    callsign: String,
    sub: String,
    operator: String,
    route: String,
    motion_heading: String,
    motion_speed: String,
    motion_track: Option<f64>,
    motion_altitude: String,
}

#[cfg(windows)]
fn row_view(aircraft: &NearestAircraft, units: Units) -> RowView {
    let state = &aircraft.state;
    // Compact ICAO codes ("KLAX → KSFO") like the mock and the ROUTE detail box,
    // so the narrow board column never wraps onto two lines.
    let route = format_route_codes_for_ui(nested_object(aircraft.metadata.as_ref(), &["flightroute"]));
    let operator = aircraft
        .classification
        .operator
        .clone()
        .or_else(|| state.origin_country.clone())
        .unwrap_or_else(|| "\u{2014}".to_string());
    RowView {
        callsign: state
            .callsign
            .clone()
            .map(|callsign| callsign.trim().to_string())
            .filter(|callsign| !callsign.is_empty())
            .unwrap_or_else(|| "NO CALLSIGN".to_string()),
        sub: format!(
            "{}  {}",
            ui_distance(aircraft.distance_km, units),
            compass16(aircraft.bearing_degrees)
        ),
        operator,
        route,
        motion_heading: ui_heading(state.true_track),
        motion_speed: ui_speed(state.velocity),
        motion_track: state.true_track,
        motion_altitude: ui_flight_level(state.baro_altitude),
    }
}

#[cfg(windows)]
fn ui_summary_card(nearest: &NearestAircraft, units: Units, is_nearest: bool) -> String {
    let callsign = nearest
        .state
        .callsign
        .as_deref()
        .map(str::trim)
        .filter(|callsign| !callsign.is_empty())
        .unwrap_or("This aircraft");
    let headline = if is_nearest {
        format!("{callsign} is nearest aircraft.")
    } else {
        format!("{callsign}.")
    };
    let mut lines = vec![
        headline,
        format!(
            "{} away {}.",
            ui_distance(nearest.distance_km, units),
            compass16(nearest.bearing_degrees),
        ),
    ];
    // City the aircraft is currently over (reverse-geocoded during enrichment).
    if let Some(city) = nearest
        .nearest_place
        .as_ref()
        .and_then(|place| place.city.clone().or_else(|| place.region.clone()))
    {
        lines.push(format!("Over {city}."));
    }
    lines.push(format!("Altitude {}.", ui_flight_level(nearest.state.baro_altitude)));
    lines.push(format!("Speed {}.", ui_speed(nearest.state.velocity)));
    lines.join("\n")
}

#[cfg(windows)]
fn draw_board(hdc: HDC, layout: &Layout, state: &UiState) {
    let clip = layout.board_clip;
    let sx = |value: i32| layout.sx(value);
    let sy = |value: i32| layout.sy(value);
    let font = |size: i32| layout.font(size);
    let header_y = clip.top + sy(58);
    // Column data left-aligns at these design-space x's so it sits under the
    // baked SUMMARY / OPERATOR / ROUTE / MOTION headers (which are centred over
    // each column's content in the art). Everything scales with the window.
    let col = [sx(118), sx(296), sx(478), sx(692)];
    // Divider lines sit at the midpoints between the baked header labels (matched
    // to the mock), so each header is centred in its column. Cells clip just shy
    // of the divider on their right so text never crosses the line.
    let div = [sx(256), sx(449), sx(648)];
    let row_h = layout.row_h;
    let count = state.aircraft.len().min(UI_LIST_LIMIT);
    for idx in 0..UI_LIST_LIMIT {
        let row = layout.row_rect(idx);
        let selected = idx == state.selected && idx < count;
        if selected {
            draw_bmp_stretched(hdc, row, ASSET_SELECTED_ROW);
        }

        if idx >= count {
            continue;
        }
        let view = row_view(&state.aircraft[idx], state.units);

        let text_dark = rgb(21, 31, 35);
        draw_text(
            hdc,
            &view.callsign,
            rect_xy(col[0], row.top + sy(14), div[0] - sx(8), row.top + sy(45)),
            font(22),
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.sub,
            rect_xy(col[0], row.top + sy(46), div[0] - sx(8), row.top + sy(70)),
            font(14),
            true,
            rgb(141, 26, 22),
            Align::Left,
        );
        draw_text(
            hdc,
            &view.operator,
            rect_xy(col[1], row.top + sy(12), div[1] - sx(8), row.top + sy(72)),
            font(15),
            false,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.route,
            rect_xy(col[2], row.top + sy(12), div[2] - sx(8), row.top + sy(72)),
            font(15),
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.motion_heading,
            rect_xy(col[3], row.top + sy(16), col[3] + sx(60), row.top + sy(44)),
            font(16),
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.motion_speed,
            rect_xy(
                col[3] + sx(66),
                row.top + sy(16),
                clip.right - sx(16),
                row.top + sy(44),
            ),
            font(16),
            true,
            text_dark,
            Align::Left,
        );
        draw_heading_arrow(
            hdc,
            rect_xy(
                col[3] + sx(2),
                row.top + sy(44),
                col[3] + sx(30),
                row.top + sy(72),
            ),
            view.motion_track,
            rgb(80, 60, 40),
            layout.width,
        );
        draw_text(
            hdc,
            &view.motion_altitude,
            rect_xy(
                col[3] + sx(36),
                row.top + sy(46),
                clip.right - sx(16),
                row.top + sy(72),
            ),
            font(15),
            false,
            rgb(80, 60, 40),
            Align::Left,
        );
    }

    for x in div {
        draw_line(
            hdc,
            x,
            header_y + sy(6),
            x,
            clip.bottom - sy(12),
            rgb(185, 145, 83),
            scale_len(layout.width, 1),
        );
    }
    for idx in 0..=UI_LIST_LIMIT {
        let y = layout.row_y + row_h * idx as i32;
        draw_line(
            hdc,
            clip.left,
            y,
            clip.right,
            y,
            rgb(183, 141, 82),
            scale_len(layout.width, 1),
        );
    }
}

/// Shared radar scope geometry (centre and the pixel radius/distance span it
/// covers) so click hit-testing agrees exactly with where draw_radar plots
/// each contact.
#[cfg(windows)]
fn radar_scope(layout: &Layout, state: &UiState) -> (i32, i32, f64, f64) {
    let area = layout.radar;
    let width = area.right - area.left;
    let height = area.bottom - area.top;
    let cx = area.left + width * 46 / 100;
    let cy = area.top + height * 53 / 100;
    let radius = width.min(height) * 47 / 100;
    let usable = (radius as f64) * 0.86;
    // Scale the scope to the farthest displayed contact (not the whole search
    // radius) so nearby traffic spreads across the scope instead of collapsing
    // onto the centre.
    let span = state
        .aircraft
        .iter()
        .take(UI_LIST_LIMIT)
        .map(|aircraft| aircraft.distance_km)
        .fold(0.0_f64, f64::max)
        .max(1.0)
        * 1.15;
    (cx, cy, usable, span)
}

/// Returns the aircraft list index whose radar marker sits under (x, y), if
/// any preferring the selected contact when markers overlap, since it's the
/// one drawn on top.
#[cfg(windows)]
fn radar_hit_test(layout: &Layout, state: &UiState, x: i32, y: i32) -> Option<usize> {
    if !point_in(layout.radar, x, y) {
        return None;
    }
    let (cx, cy, usable, span) = radar_scope(layout, state);
    let count = state.aircraft.len().min(UI_LIST_LIMIT);
    let hit_radius = layout.sx(16) as i64;
    let now_unix = Utc::now().timestamp();
    let mut order: Vec<usize> = Vec::with_capacity(count);
    if state.selected < count {
        order.push(state.selected);
    }
    order.extend((0..count).filter(|&idx| idx != state.selected));

    for idx in order {
        let (distance_km, bearing_degrees) = extrapolated_polar(&state.aircraft[idx], now_unix);
        let (px, py) = radar_point(cx, cy, usable, span, distance_km, bearing_degrees);
        let dx = (x - px) as i64;
        let dy = (y - py) as i64;
        if dx * dx + dy * dy <= hit_radius * hit_radius {
            return Some(idx);
        }
    }
    None
}

#[cfg(windows)]
fn draw_radar(hdc: HDC, layout: &Layout, state: &UiState) {
    let sx = |value: i32| layout.sx(value);
    let sy = |value: i32| layout.sy(value);
    let font = |size: i32| layout.font(size);
    let (cx, cy, usable, span) = radar_scope(layout, state);
    let now_unix = Utc::now().timestamp();
    // Centre marker for the observer's own position.
    fill_ellipse(
        hdc,
        cx - sx(4),
        cy - sy(4),
        cx + sx(4),
        cy + sy(4),
        rgb(236, 211, 161),
        rgb(236, 211, 161),
        scale_len(layout.width, 1),
    );

    let count = state.aircraft.len().min(UI_LIST_LIMIT);
    // Plot non-selected contacts first so the selected one paints on top.
    for idx in 0..count {
        if idx == state.selected {
            continue;
        }
        let aircraft = &state.aircraft[idx];
        let (distance_km, bearing_degrees) = extrapolated_polar(aircraft, now_unix);
        let (bx, by) = radar_point(cx, cy, usable, span, distance_km, bearing_degrees);
        draw_plane_marker(
            hdc,
            bx,
            by,
            sx(12),
            aircraft.state.true_track,
            rgb(120, 196, 224),
            rgb(40, 92, 112),
        );
    }

    if let Some(aircraft) = state.aircraft.get(state.selected) {
        let (distance_km, bearing_degrees) = extrapolated_polar(aircraft, now_unix);
        let (tx, ty) = radar_point(cx, cy, usable, span, distance_km, bearing_degrees);
        draw_line(
            hdc,
            cx,
            cy,
            tx,
            ty,
            rgb(240, 190, 36),
            scale_len(layout.width, 2),
        );
        draw_target_brackets(hdc, tx, ty, sx(26), rgb(240, 190, 36), layout.width);
        fill_ellipse(
            hdc,
            tx - sx(19),
            ty - sy(19),
            tx + sx(19),
            ty + sy(19),
            rgb(99, 35, 26),
            rgb(240, 190, 36),
            scale_len(layout.width, 2),
        );
        draw_plane_marker(
            hdc,
            tx,
            ty,
            sx(14),
            aircraft.state.true_track,
            rgb(255, 225, 138),
            rgb(120, 70, 20),
        );
        let callsign = aircraft
            .state
            .callsign
            .clone()
            .map(|callsign| callsign.trim().to_string())
            .filter(|callsign| !callsign.is_empty())
            .unwrap_or_else(|| "NO CALLSIGN".to_string());
        draw_text(
            hdc,
            &format!(
                "{}\n{}  {}\n{}",
                callsign,
                ui_distance(distance_km, state.units),
                compass16(bearing_degrees),
                ui_flight_level(aircraft.state.baro_altitude)
            ),
            rect_xy(tx + sx(22), ty - sy(14), tx + sx(168), ty + sy(70)),
            font(15),
            true,
            rgb(242, 218, 171),
            Align::Left,
        );
    }

}

/// Extrapolate a contact's observer-relative distance/bearing forward to
/// `now` using its last known ground speed and track, so the radar shows it
/// drifting along its heading between OpenSky refreshes instead of jumping
/// once every refresh. A flat-plane projection is accurate enough at the
/// ranges (typically well under 200 km) and extrapolation windows (capped at
/// `RADAR_EXTRAPOLATION_LIMIT_S`) involved here.
#[cfg(windows)]
fn extrapolated_polar(aircraft: &NearestAircraft, now_unix: i64) -> (f64, f64) {
    let (Some(speed_mps), Some(track_deg)) = (aircraft.state.velocity, aircraft.state.true_track)
    else {
        return (aircraft.distance_km, aircraft.bearing_degrees);
    };
    let reference = aircraft
        .state
        .time_position
        .or(aircraft.api_time)
        .unwrap_or(now_unix);
    let elapsed_s = (now_unix - reference).clamp(0, RADAR_EXTRAPOLATION_LIMIT_S) as f64;
    if elapsed_s <= 0.0 || speed_mps <= 0.0 {
        return (aircraft.distance_km, aircraft.bearing_degrees);
    }

    let bearing_rad = aircraft.bearing_degrees.to_radians();
    let mut east_km = aircraft.distance_km * bearing_rad.sin();
    let mut north_km = aircraft.distance_km * bearing_rad.cos();

    let track_rad = track_deg.to_radians();
    let travelled_km = speed_mps * elapsed_s / 1000.0;
    east_km += travelled_km * track_rad.sin();
    north_km += travelled_km * track_rad.cos();

    let distance_km = east_km.hypot(north_km);
    let bearing_degrees = east_km.atan2(north_km).to_degrees().rem_euclid(360.0);
    (distance_km, bearing_degrees)
}

/// Project a bearing/distance onto the radar scope (North up).
#[cfg(windows)]
fn radar_point(
    cx: i32,
    cy: i32,
    usable: f64,
    span_km: f64,
    distance_km: f64,
    bearing_degrees: f64,
) -> (i32, i32) {
    let r = usable * (distance_km / span_km).clamp(0.0, 1.0);
    let angle = (bearing_degrees - 90.0).to_radians();
    (cx + (angle.cos() * r) as i32, cy + (angle.sin() * r) as i32)
}

/// Status strip across the very bottom of the window: the observer's own
/// location on the left, and the live feed status (amber, matching the lamps
/// baked into the bar) on the right — flanking the centre emblem.
#[cfg(windows)]
fn draw_status_bar(hdc: HDC, layout: &Layout, location: &Location, status: &str) {
    let here = format!(
        "{}     {:.3}, {:.3}",
        location.label.to_uppercase(),
        location.latitude,
        location.longitude,
    );
    draw_text(
        hdc,
        &here,
        scale_rect(layout.width, layout.height, 100, 886, 730, 920),
        layout.font(15),
        true,
        rgb(202, 190, 156),
        Align::Left,
    );
    draw_text(
        hdc,
        status,
        scale_rect(layout.width, layout.height, 946, 886, 1520, 920),
        layout.font(15),
        true,
        rgb(236, 190, 80),
        Align::Right,
    );
}

#[cfg(windows)]
fn draw_bottom_panels(hdc: HDC, layout: &Layout, model: Option<&UiModel>, status: &str) {
    let area = layout.bottom;
    let Some(model) = model else {
        let panel = rect_xy(area.left, area.top, area.right, area.bottom);
        draw_text(
            hdc,
            status,
            inset(panel, layout.sx(30), layout.sy(54)),
            layout.font(18),
            true,
            rgb(113, 25, 23),
            Align::Left,
        );
        return;
    };
    draw_text(
        hdc,
        &model.summary,
        scale_rect(layout.width, layout.height, 162, 736, 386, 844),
        layout.font(16),
        false,
        rgb(22, 33, 38),
        Align::Left,
    );
    draw_text(
        hdc,
        &model.operator,
        scale_rect(layout.width, layout.height, 438, 738, 604, 786),
        layout.font(21),
        true,
        rgb(22, 33, 38),
        Align::Left,
    );
    draw_text(
        hdc,
        &model.callsign,
        scale_rect(layout.width, layout.height, 438, 818, 604, 850),
        layout.font(18),
        true,
        rgb(113, 25, 23),
        Align::Left,
    );
    // ROUTE — compact airport codes like the mock ("KLAX → KSFO"), with the
    // from/destination city pair underneath in a smaller typeface.
    draw_text(
        hdc,
        &model.route_codes,
        scale_rect(layout.width, layout.height, 656, 744, 852, 792),
        layout.font(26),
        true,
        rgb(22, 33, 38),
        Align::Center,
    );
    if !model.route_cities.is_empty() {
        draw_text(
            hdc,
            &model.route_cities,
            scale_rect(layout.width, layout.height, 656, 800, 852, 824),
            layout.font(14),
            false,
            rgb(96, 82, 68),
            Align::Center,
        );
    }
    draw_text(
        hdc,
        &model.registry,
        scale_rect(layout.width, layout.height, 908, 738, 1020, 776),
        layout.font(22),
        true,
        rgb(22, 33, 38),
        Align::Left,
    );
    draw_text(
        hdc,
        &model.aircraft_type,
        scale_rect(layout.width, layout.height, 908, 818, 1020, 850),
        layout.font(17),
        false,
        rgb(22, 33, 38),
        Align::Left,
    );

    // MOTION — analog gauges + vertical-speed / altitude readout, like the mock.
    draw_hdg_gauge(hdc, layout, 1104, 767, 40, model.track_deg, &model.heading);
    draw_speed_gauge(hdc, layout, 1207, 767, 39, model.speed_kt);
    draw_motion_readout(hdc, layout, model);

    // UPDATED — analog clock + timestamp + date, like the mock.
    if let Some((hours, minutes, seconds)) = model.clock {
        draw_clock(hdc, layout, 1451, 784, 44, hours, minutes, seconds);
    }
    draw_text(
        hdc,
        &compact_time(&model.updated),
        scale_rect(layout.width, layout.height, 1508, 752, 1650, 794),
        layout.font(27),
        true,
        rgb(22, 33, 38),
        Align::Left,
    );
    draw_text(
        hdc,
        &model.date,
        scale_rect(layout.width, layout.height, 1508, 800, 1650, 830),
        layout.font(19),
        true,
        rgb(62, 50, 42),
        Align::Left,
    );
}

/// Speedometer number below the dial, plus the VS/ALT stack to its right.
///
/// The baked "VS / FT/MIN / ALT" labels were cleared from the art so this block
/// can lay the two readouts out with mock-style spacing (label, value, divider).
#[cfg(windows)]
fn draw_motion_readout(hdc: HDC, layout: &Layout, model: &UiModel) {
    let ink = rgb(22, 33, 38);
    let label = rgb(96, 82, 68);
    // Speed value sits under the dial, above the baked "KT" label.
    let speed = model
        .speed_kt
        .map(|kt| format!("{kt:.0}"))
        .unwrap_or_else(|| "\u{2014}".to_string());
    draw_text(
        hdc,
        &speed,
        scale_rect(layout.width, layout.height, 1168, 812, 1248, 844),
        layout.font(22),
        true,
        ink,
        Align::Center,
    );

    // Vertical speed: arrow + "VS" on one line, big value, then "FT/MIN".
    let vs_value = match model.vertical_fpm {
        Some(fpm) => {
            draw_vertical_arrow(
                hdc,
                layout.sx(1298),
                layout.sy(748),
                layout.sy(772),
                fpm >= 0.0,
                ink,
                scale_len(layout.width, 3),
            );
            format!("{:.0}", fpm.abs())
        }
        None => "\u{2014}".to_string(),
    };
    draw_text(
        hdc,
        "VS",
        scale_rect(layout.width, layout.height, 1320, 748, 1376, 768),
        layout.font(15),
        true,
        label,
        Align::Left,
    );
    draw_text(
        hdc,
        &vs_value,
        scale_rect(layout.width, layout.height, 1300, 770, 1378, 800),
        layout.font(23),
        true,
        ink,
        Align::Left,
    );
    draw_text(
        hdc,
        "FT/MIN",
        scale_rect(layout.width, layout.height, 1300, 801, 1378, 816),
        layout.font(13),
        true,
        label,
        Align::Left,
    );

    // Divider, then altitude / flight level.
    draw_line(
        hdc,
        layout.sx(1294),
        layout.sy(820),
        layout.sx(1374),
        layout.sy(820),
        rgb(150, 120, 80),
        scale_len(layout.width, 1),
    );
    draw_text(
        hdc,
        "ALT",
        scale_rect(layout.width, layout.height, 1300, 824, 1378, 842),
        layout.font(15),
        true,
        label,
        Align::Left,
    );
    draw_text(
        hdc,
        &model.altitude,
        scale_rect(layout.width, layout.height, 1300, 840, 1378, 866),
        layout.font(22),
        true,
        ink,
        Align::Left,
    );
}

/// Point on a circle: `deg` measured clockwise from straight up (compass style).
#[cfg(windows)]
fn polar(cx: i32, cy: i32, r: f64, deg: f64) -> (i32, i32) {
    let rad = deg.to_radians();
    (
        cx + (r * rad.sin()).round() as i32,
        cy - (r * rad.cos()).round() as i32,
    )
}

/// Unfilled circle outline (rim) drawn on top of the panel texture.
#[cfg(windows)]
fn draw_circle(hdc: HDC, cx: i32, cy: i32, r: i32, color: COLORREF, width: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, width, color);
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let old_brush = SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
        let _ = Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(pen));
    }
}

/// Small filled triangle pointing down from `top_y` (used as the compass index).
#[cfg(windows)]
fn fill_triangle_down(hdc: HDC, cx: i32, top_y: i32, size: i32, color: COLORREF) {
    unsafe {
        let points = [
            POINT { x: cx - size, y: top_y },
            POINT { x: cx + size, y: top_y },
            POINT { x: cx, y: top_y + size * 2 },
        ];
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(PS_SOLID, 1, color);
        let old_brush = SelectObject(hdc, HGDIOBJ::from(brush));
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let _ = Polygon(hdc, &points);
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(brush));
        let _ = DeleteObject(HGDIOBJ::from(pen));
    }
}

/// Vertical arrow (climb = up, descent = down) with a small chevron head.
#[cfg(windows)]
fn draw_vertical_arrow(
    hdc: HDC,
    x: i32,
    top: i32,
    bottom: i32,
    up: bool,
    color: COLORREF,
    width: i32,
) {
    draw_line(hdc, x, top, x, bottom, color, width);
    let head = (bottom - top) / 3;
    let (tip, back) = if up { (top, top + head) } else { (bottom, bottom - head) };
    draw_line(hdc, x, tip, x - head / 2, back, color, width);
    draw_line(hdc, x, tip, x + head / 2, back, color, width);
}

/// Heading indicator: rim, tick ring, red north index, needle, centre degrees.
#[cfg(windows)]
fn draw_hdg_gauge(hdc: HDC, layout: &Layout, dcx: i32, dcy: i32, dr: i32, track: Option<f64>, value: &str) {
    let cx = layout.sx(dcx);
    let cy = layout.sy(dcy);
    let r = scale_len(layout.width, dr);
    let ink = rgb(74, 60, 44);
    draw_circle(hdc, cx, cy, r, ink, scale_len(layout.width, 2));
    for i in 0..12 {
        let major = i % 3 == 0;
        let inner = r - scale_len(layout.width, if major { 9 } else { 5 });
        let (ix, iy) = polar(cx, cy, inner as f64, i as f64 * 30.0);
        let (ox, oy) = polar(cx, cy, r as f64, i as f64 * 30.0);
        draw_line(hdc, ix, iy, ox, oy, ink, scale_len(layout.width, if major { 2 } else { 1 }));
    }
    fill_triangle_down(hdc, cx, cy - r + scale_len(layout.width, 1), scale_len(layout.width, 5), rgb(176, 42, 34));
    if let Some(track) = track {
        let (nx, ny) = polar(cx, cy, r as f64 * 0.9, track);
        let (bx, by) = polar(cx, cy, r as f64 * 0.5, track);
        draw_line(hdc, bx, by, nx, ny, rgb(176, 42, 34), scale_len(layout.width, 2));
    }
    draw_text(
        hdc,
        value,
        rect_xy(cx - r, cy - scale_len(layout.width, 15), cx + r, cy + scale_len(layout.width, 15)),
        layout.font(21),
        true,
        rgb(22, 33, 38),
        Align::Center,
    );
}

/// Speedometer: 270° tick arc with a red high-speed band, needle, hub.
#[cfg(windows)]
fn draw_speed_gauge(hdc: HDC, layout: &Layout, dcx: i32, dcy: i32, dr: i32, speed_kt: Option<f64>) {
    let cx = layout.sx(dcx);
    let cy = layout.sy(dcy);
    let r = scale_len(layout.width, dr);
    let ink = rgb(74, 60, 44);
    const START: f64 = -135.0;
    const SWEEP: f64 = 270.0;
    const MAX: f64 = 550.0;
    const RED_FROM: f64 = 0.66;
    draw_circle(hdc, cx, cy, r, ink, scale_len(layout.width, 2));
    for i in 0..=9 {
        let f = i as f64 / 9.0;
        let deg = START + SWEEP * f;
        let major = i % 3 == 0;
        let inner = r - scale_len(layout.width, if major { 8 } else { 5 });
        let (ix, iy) = polar(cx, cy, inner as f64, deg);
        let (ox, oy) = polar(cx, cy, r as f64, deg);
        let color = if f >= RED_FROM { rgb(176, 42, 34) } else { ink };
        draw_line(hdc, ix, iy, ox, oy, color, scale_len(layout.width, if major { 2 } else { 1 }));
    }
    // Thick red band along the high-speed portion of the rim.
    let band = r - scale_len(layout.width, 2);
    let mut prev: Option<(i32, i32)> = None;
    let mut t = RED_FROM;
    while t <= 1.0001 {
        let point = polar(cx, cy, band as f64, START + SWEEP * t.min(1.0));
        if let Some((px, py)) = prev {
            draw_line(hdc, px, py, point.0, point.1, rgb(176, 42, 34), scale_len(layout.width, 3));
        }
        prev = Some(point);
        t += 0.06;
    }
    if let Some(kt) = speed_kt {
        let f = (kt / MAX).clamp(0.0, 1.0);
        let (nx, ny) = polar(cx, cy, r as f64 * 0.82, START + SWEEP * f);
        draw_line(hdc, cx, cy, nx, ny, rgb(176, 42, 34), scale_len(layout.width, 2));
    }
    let hub = scale_len(layout.width, 4);
    fill_ellipse(hdc, cx - hub, cy - hub, cx + hub, cy + hub, rgb(40, 30, 24), ink, scale_len(layout.width, 1));
}

/// Analog clock: ivory face, hour ticks, hour/minute/second hands.
#[cfg(windows)]
fn draw_clock(hdc: HDC, layout: &Layout, dcx: i32, dcy: i32, dr: i32, hours: f64, minutes: f64, seconds: f64) {
    let cx = layout.sx(dcx);
    let cy = layout.sy(dcy);
    let r = scale_len(layout.width, dr);
    let ink = rgb(58, 46, 38);
    fill_ellipse(hdc, cx - r, cy - r, cx + r, cy + r, rgb(238, 228, 206), rgb(78, 60, 42), scale_len(layout.width, 3));
    for i in 0..12 {
        let major = i % 3 == 0;
        let inner = r - scale_len(layout.width, if major { 8 } else { 5 });
        let (ix, iy) = polar(cx, cy, inner as f64, i as f64 * 30.0);
        let (ox, oy) = polar(cx, cy, (r - scale_len(layout.width, 2)) as f64, i as f64 * 30.0);
        draw_line(hdc, ix, iy, ox, oy, ink, scale_len(layout.width, if major { 2 } else { 1 }));
    }
    let (hx, hy) = polar(cx, cy, r as f64 * 0.5, hours / 12.0 * 360.0);
    draw_line(hdc, cx, cy, hx, hy, rgb(30, 26, 22), scale_len(layout.width, 3));
    let (mx, my) = polar(cx, cy, r as f64 * 0.74, minutes / 60.0 * 360.0);
    draw_line(hdc, cx, cy, mx, my, rgb(30, 26, 22), scale_len(layout.width, 2));
    let (sx2, sy2) = polar(cx, cy, r as f64 * 0.8, seconds / 60.0 * 360.0);
    draw_line(hdc, cx, cy, sx2, sy2, rgb(176, 42, 34), scale_len(layout.width, 1));
    let hub = scale_len(layout.width, 3);
    fill_ellipse(hdc, cx - hub, cy - hub, cx + hub, cy + hub, rgb(30, 26, 22), rgb(30, 26, 22), 1);
}

/// Rectangle outline drawn as four lines (for the unit toggle buttons).
#[cfg(windows)]
fn draw_border(hdc: HDC, rect: RECT, color: COLORREF, width: i32) {
    draw_line(hdc, rect.left, rect.top, rect.right, rect.top, color, width);
    draw_line(hdc, rect.left, rect.bottom, rect.right, rect.bottom, color, width);
    draw_line(hdc, rect.left, rect.top, rect.left, rect.bottom, color, width);
    draw_line(hdc, rect.right, rect.top, rect.right, rect.bottom, color, width);
}

/// The NM / KM unit switch — the selected side is filled crimson with a gold
/// border so the highlight tracks `units` (the art is drawn neutral underneath).
#[cfg(windows)]
fn draw_unit_toggle(hdc: HDC, layout: &Layout, units: Units) {
    let nm = scale_rect(layout.width, layout.height, 1097, 22, 1161, 55);
    let km = scale_rect(layout.width, layout.height, 1170, 22, 1234, 55);
    draw_unit_button(hdc, layout, nm, "NM", units == Units::Imperial);
    draw_unit_button(hdc, layout, km, "KM", units == Units::Metric);
}

#[cfg(windows)]
fn draw_unit_button(hdc: HDC, layout: &Layout, rect: RECT, label: &str, selected: bool) {
    if selected {
        fill_rect(hdc, rect, rgb(122, 26, 20));
        draw_line(hdc, rect.left, rect.top + scale_len(layout.width, 1), rect.right, rect.top + scale_len(layout.width, 1), rgb(178, 46, 36), scale_len(layout.width, 2));
        draw_border(hdc, rect, rgb(198, 164, 86), scale_len(layout.width, 2));
    } else {
        fill_rect(hdc, rect, rgb(12, 24, 32));
        draw_border(hdc, rect, rgb(46, 60, 68), scale_len(layout.width, 1));
    }
    let color = if selected { rgb(240, 232, 210) } else { rgb(150, 166, 175) };
    draw_text(hdc, label, rect, layout.font(21), true, color, Align::Center);
}

/// Full text report panel shown on the RAW REPORT tab.
#[cfg(windows)]
fn draw_raw_report(hdc: HDC, layout: &Layout, report: &str, scroll_lines: usize) {
    let area = layout.main;
    draw_bmp_stretched(hdc, area, ASSET_RAW_REPORT);
    let clip = inset(area, layout.sx(24), layout.sy(18));
    let body = rect_xy(clip.left, clip.top + layout.sy(46), clip.right, clip.bottom);
    let max_scroll = raw_report_max_scroll_lines(layout, report);
    let scroll_lines = scroll_lines.min(max_scroll);
    let visible_report = report
        .lines()
        .skip(scroll_lines)
        .collect::<Vec<_>>()
        .join("\n");
    draw_text(
        hdc,
        "RAW REPORT",
        rect_xy(clip.left, clip.top, clip.right, clip.top + layout.sy(30)),
        layout.font(20),
        true,
        rgb(139, 26, 22),
        Align::Left,
    );
    draw_line(
        hdc,
        clip.left,
        clip.top + layout.sy(36),
        clip.right,
        clip.top + layout.sy(36),
        rgb(151, 114, 61),
        scale_len(layout.width, 1),
    );
    draw_mono_text(hdc, &visible_report, body, layout.font(15), rgb(28, 36, 38));
    draw_raw_scrollbar(hdc, layout, body, scroll_lines, max_scroll);
}

#[cfg(windows)]
fn raw_report_max_scroll_lines(layout: &Layout, report: &str) -> usize {
    let area = layout.main;
    let clip = inset(area, layout.sx(24), layout.sy(18));
    let body_height = (clip.bottom - (clip.top + layout.sy(46))).max(1);
    let line_height = raw_report_line_height(layout).max(1);
    let visible_lines = (body_height / line_height).max(1) as usize;
    report.lines().count().saturating_sub(visible_lines)
}

#[cfg(windows)]
fn raw_report_line_height(layout: &Layout) -> i32 {
    layout.font(15) + layout.sy(5).max(3)
}

#[cfg(windows)]
fn draw_raw_scrollbar(
    hdc: HDC,
    layout: &Layout,
    body: RECT,
    scroll_lines: usize,
    max_scroll: usize,
) {
    if max_scroll == 0 {
        return;
    }
    let track = rect_xy(
        body.right - layout.sx(12),
        body.top,
        body.right - layout.sx(7),
        body.bottom,
    );
    fill_rect(hdc, track, rgb(181, 152, 100));
    let track_h = (track.bottom - track.top).max(1);
    let thumb_h = (track_h / 4).max(layout.sy(26));
    let travel = (track_h - thumb_h).max(1);
    let thumb_top = track.top + (travel as usize * scroll_lines / max_scroll) as i32;
    fill_rect(
        hdc,
        rect_xy(track.left, thumb_top, track.right, thumb_top + thumb_h),
        rgb(111, 38, 30),
    );
}

/// Draw a compact heading arrow for the table's MOTION column.
#[cfg(windows)]
fn draw_heading_arrow(hdc: HDC, rect: RECT, track: Option<f64>, color: COLORREF, width: i32) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let span = ((rect.right - rect.left).min(rect.bottom - rect.top)).max(8);
    let line_width = scale_len(width, 2);
    let Some(track) = track else {
        let half = span / 5;
        draw_line(hdc, cx - half, cy, cx + half, cy, color, line_width);
        return;
    };

    let angle = track.rem_euclid(360.0).to_radians();
    let dx = angle.sin();
    let dy = -angle.cos();
    let length = (span as f64) * 0.72;
    let tail = length * 0.34;
    let head = length * 0.46;
    let x1 = cx - (dx * tail) as i32;
    let y1 = cy - (dy * tail) as i32;
    let x2 = cx + (dx * head) as i32;
    let y2 = cy + (dy * head) as i32;
    draw_line(hdc, x1, y1, x2, y2, color, line_width);

    let wing_len = (span as f64) * 0.22;
    for turn in [-2.45_f64, 2.45_f64] {
        let wing_angle = angle + turn;
        let wx = x2 + (wing_angle.sin() * wing_len) as i32;
        let wy = y2 - (wing_angle.cos() * wing_len) as i32;
        draw_line(hdc, x2, y2, wx, wy, color, line_width);
    }
}

/// Draw a small aircraft glyph rotated to its heading (North up).
#[cfg(windows)]
fn draw_plane_marker(
    hdc: HDC,
    cx: i32,
    cy: i32,
    size: i32,
    heading: Option<f64>,
    fill: COLORREF,
    border: COLORREF,
) {
    let h = heading.unwrap_or(0.0).to_radians();
    let s = size as f64;
    // Arrowhead pointing "up" before rotation: nose, right, tail-notch, left.
    let local = [
        (0.0, -s),
        (0.75 * s, 0.7 * s),
        (0.0, 0.35 * s),
        (-0.75 * s, 0.7 * s),
    ];
    let (sin, cos) = (h.sin(), h.cos());
    let points: Vec<POINT> = local
        .iter()
        .map(|(px, py)| POINT {
            x: cx + (px * cos - py * sin) as i32,
            y: cy + (px * sin + py * cos) as i32,
        })
        .collect();
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, 1, border);
        let old_brush = SelectObject(hdc, HGDIOBJ::from(brush));
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let _ = Polygon(hdc, &points);
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(brush));
        let _ = DeleteObject(HGDIOBJ::from(pen));
    }
}

/// 16-point compass abbreviation for a bearing in degrees.
#[cfg(windows)]
fn compass16(degrees: f64) -> &'static str {
    const POINTS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let normalized = degrees.rem_euclid(360.0);
    let index = ((normalized / 22.5).round() as usize) % 16;
    POINTS[index]
}

#[cfg(windows)]
fn ui_distance(distance_km: f64, units: Units) -> String {
    match units {
        Units::Imperial => format!("{:.1} NM", distance_km * 0.539957),
        Units::Metric => format!("{distance_km:.1} KM"),
    }
}

#[cfg(windows)]
fn ui_speed(velocity_mps: Option<f64>) -> String {
    match velocity_mps {
        Some(mps) => format!("{:.0} KT", mps * MPS_TO_KNOTS),
        None => "\u{2014}".to_string(),
    }
}

#[cfg(windows)]
fn ui_heading(track: Option<f64>) -> String {
    match track {
        Some(degrees) => format!("{:.0}\u{00B0}", degrees.rem_euclid(360.0)),
        None => "\u{2014}".to_string(),
    }
}

/// Compact flight level from a barometric altitude in metres, e.g. `FL340`.
#[cfg(windows)]
fn ui_flight_level(baro_altitude_m: Option<f64>) -> String {
    match baro_altitude_m {
        Some(meters) => {
            let feet = meters * M_TO_FEET;
            if feet < 0.0 {
                "GND".to_string()
            } else if feet < 18_000.0 {
                format!("{:.0} ft", feet)
            } else {
                format!("FL{:03.0}", feet / 100.0)
            }
        }
        None => "\u{2014}".to_string(),
    }
}

#[cfg(windows)]
fn draw_target_brackets(hdc: HDC, cx: i32, cy: i32, radius: i32, color: COLORREF, width: i32) {
    let len = scale_len(width, 12);
    let line_width = scale_len(width, 2);
    for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
        let x = cx + sx * radius;
        let y = cy + sy * radius;
        draw_line(hdc, x, y, x - sx * len, y, color, line_width);
        draw_line(hdc, x, y, x, y - sy * len, color, line_width);
    }
}

#[cfg(windows)]
struct DecodedPng {
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

#[cfg(windows)]
fn draw_png_background_1x(hdc: HDC) {
    let Some(image) = decoded_background_png() else {
        return;
    };
    draw_decoded_png_1x(hdc, 0, 0, image);
}

#[cfg(windows)]
fn decoded_background_png() -> Option<&'static DecodedPng> {
    static BACKGROUND: OnceLock<Option<DecodedPng>> = OnceLock::new();
    BACKGROUND
        .get_or_init(|| decode_png_to_bgra(ASSET_BACKGROUND_PNG))
        .as_ref()
}

#[cfg(windows)]
fn decode_png_to_bgra(bytes: &[u8]) -> Option<DecodedPng> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in rgba.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        bgra.extend_from_slice(&[blue, green, red, alpha]);
    }
    Some(DecodedPng {
        width: width as i32,
        height: height as i32,
        bgra,
    })
}

#[cfg(windows)]
fn draw_decoded_png_1x(hdc: HDC, x: i32, y: i32, image: &DecodedPng) {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width,
            // Negative height makes the DIB top-down, matching PNG scanline order.
            biHeight: -image.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: image.bgra.len() as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default()],
    };
    unsafe {
        let _ = StretchDIBits(
            hdc,
            x,
            y,
            image.width,
            image.height,
            0,
            0,
            image.width,
            image.height,
            Some(image.bgra.as_ptr() as *const core::ffi::c_void),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

#[cfg(windows)]
fn draw_bmp_stretched(hdc: HDC, rect: RECT, bytes: &[u8]) {
    let Some((width, height, bit_count, pixel_offset)) = bmp_info(bytes) else {
        fill_rect(hdc, rect, rgb(18, 28, 34));
        return;
    };
    let row_stride = (((width as usize * bit_count as usize) + 31) / 32) * 4;
    let image_size = row_stride.saturating_mul(height as usize);
    if bytes.len() < pixel_offset.saturating_add(image_size) {
        fill_rect(hdc, rect, rgb(18, 28, 34));
        return;
    }

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: bit_count,
            biCompression: BI_RGB.0,
            biSizeImage: image_size as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default()],
    };
    unsafe {
        let _ = StretchDIBits(
            hdc,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            0,
            0,
            width,
            height,
            Some(bytes[pixel_offset..].as_ptr() as *const core::ffi::c_void),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

#[cfg(windows)]
fn bmp_info(bytes: &[u8]) -> Option<(i32, i32, u16, usize)> {
    if bytes.len() < 54 || bytes.get(0..2)? != b"BM" {
        return None;
    }
    let pixel_offset = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?) as usize;
    let width = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
    let height = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
    let planes = u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?);
    let bit_count = u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?);
    let compression = u32::from_le_bytes(bytes.get(30..34)?.try_into().ok()?);
    if width <= 0
        || height <= 0
        || planes != 1
        || !matches!(bit_count, 24 | 32)
        || compression != BI_RGB.0
    {
        return None;
    }
    Some((width, height, bit_count, pixel_offset))
}

#[cfg(windows)]
fn fill_rect(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let _ = FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ::from(brush));
    }
}

#[cfg(windows)]
fn draw_text(
    hdc: HDC,
    text: &str,
    mut rect: RECT,
    size: i32,
    bold: bool,
    color: COLORREF,
    align: Align,
) {
    unsafe {
        let face = to_wide("Bahnschrift");
        let font = CreateFontW(
            -size,
            0,
            0,
            0,
            if bold {
                FW_BOLD.0 as i32
            } else {
                FW_NORMAL.0 as i32
            },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        );
        let old_font = SelectObject(hdc, HGDIOBJ::from(font));
        let _ = SetTextColor(hdc, color);
        let format = match align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
            Align::Right => DT_RIGHT,
        };
        let rect_h = rect.bottom - rect.top;
        let single_line = !text.contains('\n') && rect_h <= size * 2 + 4;
        let mut format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(format.0 | DT_WORDBREAK.0);
        if single_line {
            format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
                format.0 | DT_SINGLELINE.0 | DT_VCENTER.0 | DT_END_ELLIPSIS.0,
            );
        }
        let mut wide = text.encode_utf16().collect::<Vec<u16>>();
        let _ = DrawTextW(hdc, &mut wide, &mut rect, format);
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(HGDIOBJ::from(font));
    }
}

/// Draw multi-line preformatted text in a monospace face, preserving newlines.
#[cfg(windows)]
fn draw_mono_text(hdc: HDC, text: &str, mut rect: RECT, size: i32, color: COLORREF) {
    unsafe {
        let face = to_wide("Consolas");
        let font = CreateFontW(
            -size,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        );
        let old_font = SelectObject(hdc, HGDIOBJ::from(font));
        let _ = SetTextColor(hdc, color);
        let format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
            DT_LEFT.0 | DT_NOPREFIX.0 | DT_WORDBREAK.0,
        );
        let mut wide = text.encode_utf16().collect::<Vec<u16>>();
        let _ = DrawTextW(hdc, &mut wide, &mut rect, format);
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(HGDIOBJ::from(font));
    }
}

#[cfg(windows)]
fn draw_line(hdc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, color: COLORREF, width: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, width, color);
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let _ = MoveToEx(hdc, x1, y1, None);
        let _ = LineTo(hdc, x2, y2);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(pen));
    }
}

#[cfg(windows)]
fn fill_ellipse(
    hdc: HDC,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    fill: COLORREF,
    border: COLORREF,
    width: i32,
) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, width, border);
        let old_brush = SelectObject(hdc, HGDIOBJ::from(brush));
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let _ = Ellipse(hdc, left, top, right, bottom);
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ::from(pen));
        let _ = DeleteObject(HGDIOBJ::from(brush));
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

#[cfg(windows)]
fn rect_xy(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

#[cfg(windows)]
fn scale_x(width: i32, value: i32) -> i32 {
    width * value / DESIGN_WIDTH
}

#[cfg(windows)]
fn scale_y(height: i32, value: i32) -> i32 {
    height * value / DESIGN_HEIGHT
}

#[cfg(windows)]
fn scale_len(width: i32, value: i32) -> i32 {
    (width * value / DESIGN_WIDTH).max(1)
}

#[cfg(windows)]
fn scale_font(width: i32, size: i32) -> i32 {
    (width * size / DESIGN_WIDTH).max(8)
}

#[cfg(windows)]
fn scale_rect(width: i32, height: i32, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    rect_xy(
        scale_x(width, left),
        scale_y(height, top),
        scale_x(width, right),
        scale_y(height, bottom),
    )
}

#[cfg(windows)]
fn inset(rect: RECT, x: i32, y: i32) -> RECT {
    RECT {
        left: rect.left + x,
        top: rect.top + y,
        right: rect.right - x,
        bottom: rect.bottom - y,
    }
}

#[cfg(windows)]
fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | ((green as u32) << 8) | ((blue as u32) << 16))
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn resolve_windows_location(timeout_seconds: f64) -> AppResult<Location> {
    let locator = Geolocator::new().map_err(|error| {
        FlightTrackerError(format!("could not create Windows geolocator: {error}"))
    })?;
    locator
        .SetDesiredAccuracy(PositionAccuracy::Default)
        .map_err(|error| {
            FlightTrackerError(format!("could not configure Windows geolocator: {error}"))
        })?;

    let timeout_seconds = timeout_seconds.clamp(1.0, 60.0);
    let operation = locator
        .GetGeopositionAsyncWithAgeAndTimeout(
            seconds_to_timespan(60.0),
            seconds_to_timespan(timeout_seconds),
        )
        .map_err(|error| {
            FlightTrackerError(format!("Windows Location API did not start: {error}"))
        })?;
    let geoposition = operation
        .join()
        .map_err(|error| FlightTrackerError(format!("Windows Location API failed: {error}")))?;
    let coordinate = geoposition.Coordinate().map_err(|error| {
        FlightTrackerError(format!("Windows location had no coordinate: {error}"))
    })?;
    let point = coordinate
        .Point()
        .map_err(|error| FlightTrackerError(format!("Windows location had no point: {error}")))?;
    let position = point.Position().map_err(|error| {
        FlightTrackerError(format!("Windows location had no position: {error}"))
    })?;

    validate_lat_lon(position.Latitude, position.Longitude)?;
    Ok(Location {
        latitude: position.Latitude,
        longitude: position.Longitude,
        label: "Windows PC location".to_string(),
        source: "Windows Location API".to_string(),
    })
}

#[cfg(not(windows))]
fn resolve_windows_location(_timeout_seconds: f64) -> AppResult<Location> {
    Err(FlightTrackerError(
        "Windows Location API is only available on Windows".to_string(),
    ))
}

#[cfg(windows)]
fn seconds_to_timespan(seconds: f64) -> TimeSpan {
    TimeSpan {
        Duration: (seconds * 10_000_000.0).round() as i64,
    }
}

fn find_nearest_aircraft(
    location: &Location,
    radius_km: f64,
    client: &ApiClient,
) -> AppResult<NearestAircraft> {
    let mut nearest = find_nearby_aircraft(location, radius_km, client, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            FlightTrackerError(format!(
                "no aircraft with current position found within {radius_km} km"
            ))
        })?;
    enrich_aircraft(&mut nearest, client);
    Ok(nearest)
}

/// Fetch up to `limit` aircraft within `radius_km`, ordered nearest-first.
///
/// Each entry carries the raw OpenSky state plus a cheap callsign-based
/// classification (no per-aircraft network calls). Full enrichment
/// (reverse geocode, ADSBDB, FAA) is deferred to [`enrich_aircraft`] so the
/// list can show many contacts without multiplying rate-limited requests.
fn find_nearby_aircraft(
    location: &Location,
    radius_km: f64,
    client: &ApiClient,
    limit: usize,
) -> AppResult<Vec<NearestAircraft>> {
    validate_radius_km(radius_km)?;
    let box_ = bounding_box(location.latitude, location.longitude, radius_km);
    let params = vec![
        ("lamin", box_.lamin.to_string()),
        ("lomin", box_.lomin.to_string()),
        ("lamax", box_.lamax.to_string()),
        ("lomax", box_.lomax.to_string()),
        ("extended", "1".to_string()),
    ];
    let data = client.opensky_get_json("/states/all", params)?;
    let api_time = data.get("time").and_then(Value::as_i64);
    let states = data
        .get("states")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut candidates: Vec<NearestAircraft> = Vec::new();
    for raw_state in states {
        let Some(raw_array) = raw_state.as_array() else {
            continue;
        };
        let state = parse_state(raw_array);
        let (Some(aircraft_lat), Some(aircraft_lon)) = (state.latitude, state.longitude) else {
            continue;
        };

        let distance_km = haversine_km(
            location.latitude,
            location.longitude,
            aircraft_lat,
            aircraft_lon,
        );
        if distance_km > radius_km {
            continue;
        }

        let classification = classify_aircraft(&state, None, None);
        candidates.push(NearestAircraft {
            distance_km,
            bearing_degrees: bearing_degrees(
                location.latitude,
                location.longitude,
                aircraft_lat,
                aircraft_lon,
            ),
            state,
            nearest_place: None,
            metadata: None,
            faa: None,
            classification,
            api_time,
        });
    }

    candidates.sort_by(|a, b| {
        a.distance_km
            .partial_cmp(&b.distance_km)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if candidates.len() > limit {
        candidates.truncate(limit);
    }

    if candidates.is_empty() {
        return Err(FlightTrackerError(format!(
            "no aircraft with current position found within {radius_km} km"
        )));
    }
    Ok(candidates)
}

/// Add place, route, registry, and operator detail to a single aircraft via
/// reverse geocoding, ADSBDB, and the FAA registry.
fn enrich_aircraft(aircraft: &mut NearestAircraft, client: &ApiClient) {
    let place = if let (Some(latitude), Some(longitude)) =
        (aircraft.state.latitude, aircraft.state.longitude)
    {
        reverse_geocode(latitude, longitude, client)
    } else {
        None
    };
    let metadata = fetch_adsbdb_metadata(&aircraft.state, client);
    let faa = fetch_faa_metadata(&aircraft.state, client);
    let classification = classify_aircraft(&aircraft.state, metadata.as_ref(), faa.as_ref());

    aircraft.nearest_place = place;
    aircraft.metadata = metadata;
    aircraft.faa = faa;
    aircraft.classification = classification;
}

fn enrich_aircraft_table_details(aircraft: &mut NearestAircraft, client: &ApiClient) {
    let metadata = fetch_adsbdb_metadata(&aircraft.state, client);
    let faa = fetch_faa_metadata(&aircraft.state, client);
    let classification = classify_aircraft(&aircraft.state, metadata.as_ref(), faa.as_ref());

    aircraft.metadata = metadata;
    aircraft.faa = faa;
    aircraft.classification = classification;
}

fn reverse_geocode(latitude: f64, longitude: f64, client: &ApiClient) -> Option<Place> {
    let data = client
        .client
        .get(NOMINATIM_REVERSE_URL)
        .query(&[
            ("format", "jsonv2".to_string()),
            ("lat", latitude.to_string()),
            ("lon", longitude.to_string()),
            ("zoom", "10".to_string()),
            ("addressdetails", "1".to_string()),
            ("accept-language", "en".to_string()),
        ])
        .send()
        .ok()
        .and_then(|response| read_json_response(response, NOMINATIM_REVERSE_URL).ok())?;

    let address = data.get("address")?;
    let city = first_present(
        address,
        &[
            "city",
            "town",
            "village",
            "municipality",
            "hamlet",
            "county",
        ],
    );
    let region = first_present(address, &["state", "region", "state_district", "county"]);
    let country = address
        .get("country")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let fallback = data.get("display_name").and_then(Value::as_str);
    let label = readable_place_label(
        city.as_deref(),
        region.as_deref(),
        country.as_deref(),
        fallback,
    )?;

    Some(Place {
        label,
        city,
        region,
        country,
        source: "OpenStreetMap Nominatim".to_string(),
    })
}

fn fetch_adsbdb_metadata(state: &FlightState, client: &ApiClient) -> Option<Value> {
    let icao24 = state.icao24.as_ref()?;
    let url = format!("{ADSBDB_API_URL}/aircraft/{icao24}");
    let mut request = client.client.get(&url);
    if let Some(callsign) = &state.callsign {
        request = request.query(&[("callsign", callsign)]);
    }
    let response = request.send().ok()?;
    let data = read_json_response(response, &url).ok()?;
    data.get("response")
        .filter(|value| value.is_object())
        .cloned()
}

fn fetch_faa_metadata(state: &FlightState, client: &ApiClient) -> Option<BTreeMap<String, String>> {
    let n_number = mode_s_hex_to_n_number(state.icao24.as_deref()?)?;
    let lookup_number = n_number.trim_start_matches('N').to_string();
    let response = client
        .client
        .get(FAA_N_NUMBER_URL)
        .query(&[("nNumberTxt", lookup_number)])
        .send();

    let html = match response {
        Ok(response) => match read_text_response(response, FAA_N_NUMBER_URL) {
            Ok(html) => html,
            Err(_) => {
                let mut fields = BTreeMap::new();
                fields.insert("n_number".to_string(), n_number);
                fields.insert("error".to_string(), "FAA lookup failed".to_string());
                return Some(fields);
            }
        },
        Err(_) => {
            let mut fields = BTreeMap::new();
            fields.insert("n_number".to_string(), n_number);
            fields.insert("error".to_string(), "FAA lookup failed".to_string());
            return Some(fields);
        }
    };

    let mut fields = parse_faa_inquiry_fields(&html);
    if fields.is_empty() {
        fields.insert("n_number".to_string(), n_number);
        fields.insert(
            "error".to_string(),
            "FAA lookup returned no parsed fields".to_string(),
        );
        return Some(fields);
    }
    fields.insert("n_number".to_string(), n_number);
    Some(fields)
}

fn classify_aircraft(
    state: &FlightState,
    metadata: Option<&Value>,
    faa: Option<&BTreeMap<String, String>>,
) -> Classification {
    let origin_country = state.origin_country.clone();
    let airline_name = nested_str(metadata, &["flightroute", "airline", "name"]);
    if let Some(airline_name) = airline_name {
        return Classification {
            kind: "commercial".to_string(),
            operator: Some(airline_name),
            country: nested_str(metadata, &["flightroute", "airline", "country"])
                .or(origin_country),
            confidence: "ADSBDB callsign route airline match".to_string(),
        };
    }

    if let Some(prefix) = callsign_prefix(state.callsign.as_deref()) {
        if let Some(airline) = airline_icao(&prefix) {
            return Classification {
                kind: "commercial".to_string(),
                operator: Some(airline.to_string()),
                country: origin_country,
                confidence: "callsign prefix match".to_string(),
            };
        }

        for (military_prefix, country, operator) in military_callsigns() {
            if state
                .callsign
                .as_deref()
                .unwrap_or_default()
                .to_uppercase()
                .starts_with(military_prefix)
            {
                return Classification {
                    kind: "military".to_string(),
                    operator: Some(operator.to_string()),
                    country: Some(country.to_string()),
                    confidence: "callsign prefix match".to_string(),
                };
            }
        }
    }

    if let Some(registered_owner) = nested_str(metadata, &["aircraft", "registered_owner"]) {
        return Classification {
            kind: "unknown".to_string(),
            operator: Some(registered_owner),
            country: nested_str(metadata, &["aircraft", "registered_owner_country_name"])
                .or(origin_country),
            confidence: "ADSBDB registered owner, not necessarily current operator".to_string(),
        };
    }

    if let Some(faa_owner) = faa.and_then(|fields| fields.get("registered_owner_name")) {
        return Classification {
            kind: "unknown".to_string(),
            operator: Some(faa_owner.clone()),
            country: Some("United States".to_string()),
            confidence: "FAA registered owner, not necessarily current operator".to_string(),
        };
    }

    Classification {
        kind: "unknown".to_string(),
        operator: None,
        country: origin_country,
        confidence: "No operator metadata found in OpenSky, ADSBDB, or FAA".to_string(),
    }
}

fn build_report(
    location: &Location,
    nearest: &NearestAircraft,
    radius_km: f64,
    units: Units,
) -> String {
    let state = &nearest.state;
    let classification = &nearest.classification;
    let aircraft_metadata = nested_object(nearest.metadata.as_ref(), &["aircraft"]);
    let flightroute = nested_object(nearest.metadata.as_ref(), &["flightroute"]);
    let faa = nearest.faa.as_ref();
    let mut report = String::new();

    pushln(&mut report, "Nearest aircraft");
    pushln(&mut report, "================");
    pushln(&mut report, &format!("Search location: {}", location.label));
    pushln(
        &mut report,
        &format!(
            "Coordinates: {:.5}, {:.5}",
            location.latitude, location.longitude
        ),
    );
    pushln(
        &mut report,
        &format!("Location source: {}", location.source),
    );
    pushln(
        &mut report,
        &format!("Search radius: {}", format_distance(radius_km, units)),
    );
    if let Some(api_time) = nearest.api_time {
        pushln(
            &mut report,
            &format!("OpenSky snapshot: {}", format_timestamp(Some(api_time))),
        );
    }
    pushln(&mut report, "");

    pushln(&mut report, "Distance");
    pushln(&mut report, "--------");
    pushln(
        &mut report,
        &format!("Distance: {}", format_distance(nearest.distance_km, units)),
    );
    pushln(
        &mut report,
        &format!("Bearing from you: {:.1} deg", nearest.bearing_degrees),
    );
    pushln(
        &mut report,
        &format!(
            "Nearest city/place to aircraft: {}",
            nearest
                .nearest_place
                .as_ref()
                .map(|place| place.label.as_str())
                .unwrap_or("unknown")
        ),
    );
    pushln(&mut report, "");

    pushln(&mut report, "Plain English summary");
    pushln(&mut report, "---------------------");
    pushln(&mut report, &human_summary(nearest, units));
    pushln(&mut report, "");

    pushln(&mut report, "Operator");
    pushln(&mut report, "--------");
    pushln(
        &mut report,
        &format!("Likely type: {}", classification.kind),
    );
    if let Some(operator) = &classification.operator {
        pushln(&mut report, &format!("Likely operator: {operator}"));
    }
    if let Some(country) = &classification.country {
        pushln(&mut report, &format!("Country: {country}"));
    }
    pushln(
        &mut report,
        &format!("How determined: {}", classification.confidence),
    );
    pushln(&mut report, "");

    pushln(&mut report, "ADSBDB aircraft details");
    pushln(&mut report, "-----------------------");
    if let Some(aircraft) = aircraft_metadata {
        pushln(
            &mut report,
            &format!(
                "Registration: {}",
                value_or_unknown(aircraft.get("registration"))
            ),
        );
        pushln(
            &mut report,
            &format!(
                "Manufacturer: {}",
                value_or_unknown(aircraft.get("manufacturer"))
            ),
        );
        pushln(
            &mut report,
            &format!("Aircraft type: {}", value_or_unknown(aircraft.get("type"))),
        );
        pushln(
            &mut report,
            &format!("ICAO type: {}", value_or_unknown(aircraft.get("icao_type"))),
        );
        pushln(
            &mut report,
            &format!(
                "Registered owner: {}",
                value_or_unknown(aircraft.get("registered_owner"))
            ),
        );
        pushln(
            &mut report,
            &format!(
                "Owner country: {}",
                value_or_unknown(aircraft.get("registered_owner_country_name"))
            ),
        );
        pushln(
            &mut report,
            &format!(
                "Operator flag code: {}",
                value_or_unknown(aircraft.get("registered_owner_operator_flag_code"))
            ),
        );
        pushln(
            &mut report,
            &format!("Photo: {}", value_or_unknown(aircraft.get("url_photo"))),
        );
    } else {
        pushln(&mut report, "No ADSBDB aircraft match found.");
    }
    pushln(&mut report, "");

    pushln(&mut report, "FAA aircraft registry");
    pushln(&mut report, "---------------------");
    if let Some(faa) = faa {
        pushln(
            &mut report,
            &format!("N-number: {}", map_value_or_unknown(faa, "n_number")),
        );
        if let Some(error) = faa.get("error") {
            pushln(&mut report, &format!("Lookup status: {error}"));
        } else {
            pushln(
                &mut report,
                &format!("Status: {}", map_value_or_unknown(faa, "status")),
            );
            pushln(
                &mut report,
                &format!(
                    "Manufacturer: {}",
                    map_value_or_unknown(faa, "manufacturer")
                ),
            );
            pushln(
                &mut report,
                &format!("Model: {}", map_value_or_unknown(faa, "model")),
            );
            pushln(
                &mut report,
                &format!(
                    "Manufacture year: {}",
                    map_value_or_unknown(faa, "manufacture_year")
                ),
            );
            pushln(
                &mut report,
                &format!(
                    "Serial number: {}",
                    map_value_or_unknown(faa, "serial_number")
                ),
            );
            pushln(
                &mut report,
                &format!(
                    "Type aircraft: {}",
                    map_value_or_unknown(faa, "type_aircraft")
                ),
            );
            pushln(
                &mut report,
                &format!("Type engine: {}", map_value_or_unknown(faa, "type_engine")),
            );
            pushln(
                &mut report,
                &format!(
                    "Registered owner: {}",
                    map_value_or_unknown(faa, "registered_owner_name")
                ),
            );
            pushln(
                &mut report,
                &format!(
                    "Owner location: {}",
                    format_codes([
                        faa.get("registered_owner_city").map(String::as_str),
                        faa.get("registered_owner_state").map(String::as_str),
                        faa.get("registered_owner_country").map(String::as_str),
                    ])
                ),
            );
        }
    } else {
        pushln(
            &mut report,
            "Not a U.S. Mode-S address or no FAA registry lookup available.",
        );
    }
    pushln(&mut report, "");

    pushln(&mut report, "Route and airline");
    pushln(&mut report, "-----------------");
    if let Some(flightroute) = flightroute {
        let airline = flightroute.get("airline").and_then(Value::as_object);
        let origin = flightroute.get("origin").and_then(Value::as_object);
        let destination = flightroute.get("destination").and_then(Value::as_object);
        pushln(
            &mut report,
            &format!("Airline: {}", value_or_unknown_obj(airline, "name")),
        );
        pushln(
            &mut report,
            &format!(
                "Airline ICAO/IATA: {}",
                format_codes([
                    object_str(airline, "icao").as_deref(),
                    object_str(airline, "iata").as_deref(),
                    None,
                ])
            ),
        );
        pushln(
            &mut report,
            &format!(
                "Route callsign: {}",
                value_or_unknown(flightroute.get("callsign"))
            ),
        );
        pushln(&mut report, &format!("Origin: {}", format_airport(origin)));
        pushln(
            &mut report,
            &format!("Destination: {}", format_airport(destination)),
        );
    } else {
        pushln(
            &mut report,
            "No ADSBDB route match found for this callsign.",
        );
    }
    pushln(&mut report, "");

    pushln(&mut report, "Flight and aircraft");
    pushln(&mut report, "-------------------");
    pushln(
        &mut report,
        &format!("Callsign: {}", string_or_unknown(state.callsign.as_deref())),
    );
    pushln(
        &mut report,
        &format!("ICAO24: {}", string_or_unknown(state.icao24.as_deref())),
    );
    pushln(
        &mut report,
        &format!(
            "Origin country: {}",
            string_or_unknown(state.origin_country.as_deref())
        ),
    );
    pushln(
        &mut report,
        &format!("Squawk: {}", string_or_unknown(state.squawk.as_deref())),
    );
    pushln(
        &mut report,
        &format!("On ground: {}", yes_no(state.on_ground)),
    );
    pushln(
        &mut report,
        &format!("Special purpose indicator: {}", yes_no(state.spi)),
    );
    pushln(
        &mut report,
        &format!(
            "Category: {}",
            describe_code(state.category, aircraft_category_description)
        ),
    );
    pushln(
        &mut report,
        &format!(
            "Position source: {}",
            describe_code(state.position_source, position_source_description)
        ),
    );
    pushln(&mut report, "");

    pushln(&mut report, "Position and motion");
    pushln(&mut report, "-------------------");
    pushln(
        &mut report,
        &format!("Latitude: {}", format_float(state.latitude, 5)),
    );
    pushln(
        &mut report,
        &format!("Longitude: {}", format_float(state.longitude, 5)),
    );
    pushln(
        &mut report,
        &format!(
            "Barometric altitude: {}",
            format_meters_and_feet(state.baro_altitude)
        ),
    );
    pushln(
        &mut report,
        &format!(
            "Geometric altitude: {}",
            format_meters_and_feet(state.geo_altitude)
        ),
    );
    pushln(
        &mut report,
        &format!("Ground speed: {}", format_speed(state.velocity)),
    );
    pushln(
        &mut report,
        &format!("True track: {}", format_degrees(state.true_track)),
    );
    pushln(
        &mut report,
        &format!(
            "Vertical rate: {}",
            format_vertical_rate(state.vertical_rate)
        ),
    );
    pushln(&mut report, "");

    pushln(&mut report, "Timestamps");
    pushln(&mut report, "----------");
    pushln(
        &mut report,
        &format!(
            "Last position update: {}",
            format_timestamp(state.time_position)
        ),
    );
    pushln(
        &mut report,
        &format!("Last contact: {}", format_timestamp(state.last_contact)),
    );
    pushln(&mut report, "");

    pushln(&mut report, "Raw OpenSky state");
    pushln(&mut report, "-----------------");
    for field in STATE_FIELDS {
        pushln(
            &mut report,
            &format!("{field}: {}", raw_state_value(state, field)),
        );
    }

    report
}

fn read_json_response(mut response: Response, url: &str) -> AppResult<Value> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        let detail = if body.trim().is_empty() {
            status
                .canonical_reason()
                .unwrap_or("HTTP error")
                .to_string()
        } else {
            body.trim().to_string()
        };
        return Err(FlightTrackerError(format!(
            "HTTP {} from {url}: {detail}",
            status.as_u16()
        )));
    }

    let bytes = read_limited(&mut response, MAX_JSON_RESPONSE_BYTES, url)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| FlightTrackerError(format!("invalid JSON from {url}: {error}")))
}

fn read_text_response(mut response: Response, url: &str) -> AppResult<String> {
    let status = response.status();
    if !status.is_success() {
        return Err(FlightTrackerError(format!(
            "HTTP {} from {url}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("HTTP error")
        )));
    }
    let bytes = read_limited(&mut response, MAX_TEXT_RESPONSE_BYTES, url)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn read_limited<R: Read>(reader: &mut R, max_bytes: usize, url: &str) -> AppResult<Vec<u8>> {
    let mut limited = reader.take((max_bytes + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|error| {
        FlightTrackerError(format!("could not read response from {url}: {error}"))
    })?;
    if bytes.len() > max_bytes {
        return Err(FlightTrackerError(format!(
            "response from {url} exceeded {max_bytes} bytes"
        )));
    }
    Ok(bytes)
}

#[derive(Debug)]
struct BoundingBox {
    lamin: f64,
    lomin: f64,
    lamax: f64,
    lomax: f64,
}

fn bounding_box(latitude: f64, longitude: f64, radius_km: f64) -> BoundingBox {
    let lat_delta = (radius_km / EARTH_RADIUS_KM).to_degrees();
    let cos_lat = latitude.to_radians().cos();
    let lon_delta = if cos_lat.abs() < 1e-12 {
        180.0
    } else {
        (radius_km / EARTH_RADIUS_KM / cos_lat).to_degrees()
    };

    BoundingBox {
        lamin: (-90.0_f64).max(latitude - lat_delta),
        lomin: (-180.0_f64).max(longitude - lon_delta),
        lamax: 90.0_f64.min(latitude + lat_delta),
        lomax: 180.0_f64.min(longitude + lon_delta),
    }
}

fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = lat2_rad - lat1_rad;
    let delta_lon = (lon2 - lon1).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    EARTH_RADIUS_KM * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

fn bearing_degrees(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    let y = delta_lon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

fn validate_lat_lon(latitude: f64, longitude: f64) -> AppResult<()> {
    if !(-90.0..=90.0).contains(&latitude) {
        return Err(FlightTrackerError(
            "latitude must be between -90 and 90".to_string(),
        ));
    }
    if !(-180.0..=180.0).contains(&longitude) {
        return Err(FlightTrackerError(
            "longitude must be between -180 and 180".to_string(),
        ));
    }
    Ok(())
}

fn validate_radius_km(radius_km: f64) -> AppResult<()> {
    if !radius_km.is_finite() {
        return Err(FlightTrackerError(
            "--radius-km must be a finite number".to_string(),
        ));
    }
    if radius_km <= 0.0 {
        return Err(FlightTrackerError(
            "--radius-km must be greater than zero".to_string(),
        ));
    }
    if radius_km > MAX_RADIUS_KM {
        return Err(FlightTrackerError(format!(
            "--radius-km must be no greater than {MAX_RADIUS_KM}"
        )));
    }
    Ok(())
}

fn parse_state(raw_state: &[Value]) -> FlightState {
    FlightState {
        icao24: value_string(raw_state.first()),
        callsign: value_string(raw_state.get(1)).and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }),
        origin_country: value_string(raw_state.get(2)),
        time_position: value_i64(raw_state.get(3)),
        last_contact: value_i64(raw_state.get(4)),
        longitude: value_f64(raw_state.get(5)),
        latitude: value_f64(raw_state.get(6)),
        baro_altitude: value_f64(raw_state.get(7)),
        on_ground: value_bool(raw_state.get(8)),
        velocity: value_f64(raw_state.get(9)),
        true_track: value_f64(raw_state.get(10)),
        vertical_rate: value_f64(raw_state.get(11)),
        sensors: raw_state.get(12).filter(|value| !value.is_null()).cloned(),
        geo_altitude: value_f64(raw_state.get(13)),
        squawk: value_string(raw_state.get(14)),
        spi: value_bool(raw_state.get(15)),
        position_source: value_i64(raw_state.get(16)),
        category: value_i64(raw_state.get(17)),
    }
}

fn parse_faa_inquiry_fields(html: &str) -> BTreeMap<String, String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("td, th").expect("static selector parses");
    let cells = document
        .select(&selector)
        .map(|cell| cell.text().collect::<Vec<_>>().join(" "))
        .map(|cell| cell.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>();

    let mut fields = BTreeMap::new();
    for window in cells.windows(2) {
        if let Some(key) = faa_field_key(&window[0]) {
            fields
                .entry(key.to_string())
                .or_insert_with(|| window[1].clone());
        }
    }
    fields
}

fn faa_field_key(label: &str) -> Option<&'static str> {
    match label {
        "Manufacturer Name" => Some("manufacturer"),
        "Model" => Some("model"),
        "MFR Year" => Some("manufacture_year"),
        "Type Aircraft" => Some("type_aircraft"),
        "Type Engine" => Some("type_engine"),
        "Status" => Some("status"),
        "Serial Number" => Some("serial_number"),
        "Mode S Code (Base 16 / Hex)" => Some("mode_s_hex"),
        "Name" => Some("registered_owner_name"),
        "City" => Some("registered_owner_city"),
        "State" => Some("registered_owner_state"),
        "Country" => Some("registered_owner_country"),
        "Type Registration" => Some("type_registration"),
        _ => None,
    }
}

fn mode_s_hex_to_n_number(mode_s: &str) -> Option<String> {
    let address = i64::from_str_radix(mode_s.trim(), 16).ok()?;
    let mut offset = address - US_ICAO24_START;
    if !(0..US_ICAO24_COUNT).contains(&offset) {
        return None;
    }

    let mut n_number = format!("N{}", 1 + offset / 101711);
    offset %= 101711;
    if offset <= 600 {
        return Some(format!("{n_number}{}", n_number_suffix(offset)));
    }

    offset -= 601;
    n_number.push_str(&(offset / 10111).to_string());
    offset %= 10111;
    if offset <= 600 {
        return Some(format!("{n_number}{}", n_number_suffix(offset)));
    }

    offset -= 601;
    n_number.push_str(&(offset / 951).to_string());
    offset %= 951;
    if offset <= 600 {
        return Some(format!("{n_number}{}", n_number_suffix(offset)));
    }

    offset -= 601;
    n_number.push_str(&(offset / 35).to_string());
    offset %= 35;
    if offset <= N_NUMBER_LETTERS.len() as i64 {
        return Some(format!("{n_number}{}", n_number_letter(offset)));
    }

    offset -= N_NUMBER_LETTERS.len() as i64 + 1;
    Some(format!("{n_number}{offset}"))
}

fn n_number_suffix(mut offset: i64) -> String {
    if offset == 0 {
        return String::new();
    }
    offset -= 1;
    let first_letter = N_NUMBER_LETTERS
        .chars()
        .nth((offset / 25) as usize)
        .unwrap_or_default();
    format!("{}{}", first_letter, n_number_letter(offset % 25))
}

fn n_number_letter(offset: i64) -> String {
    if offset == 0 {
        String::new()
    } else {
        N_NUMBER_LETTERS
            .chars()
            .nth((offset - 1) as usize)
            .unwrap_or_default()
            .to_string()
    }
}

fn callsign_prefix(callsign: Option<&str>) -> Option<String> {
    let letters = callsign?
        .trim()
        .chars()
        .map(|character| character.to_ascii_uppercase())
        .take_while(|character| character.is_ascii_alphabetic())
        .collect::<String>();
    if letters.len() < 3 {
        None
    } else {
        Some(letters.chars().take(3).collect())
    }
}

fn first_present(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn readable_place_label(
    city: Option<&str>,
    region: Option<&str>,
    country: Option<&str>,
    fallback: Option<&str>,
) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for value in [city, region, country].into_iter().flatten() {
        if !value.is_empty() && !parts.contains(&value) {
            parts.push(value);
        }
    }
    if parts.is_empty() {
        fallback.map(ToOwned::to_owned)
    } else {
        Some(parts.join(", "))
    }
}

fn human_summary(nearest: &NearestAircraft, units: Units) -> String {
    let state = &nearest.state;
    let callsign = state
        .callsign
        .as_deref()
        .unwrap_or("an aircraft with no callsign");
    let place = nearest
        .nearest_place
        .as_ref()
        .map(|place| place.label.as_str())
        .unwrap_or("an unknown place");
    let operator_text = if nearest.classification.kind == "commercial"
        && nearest.classification.operator.is_some()
    {
        format!(
            "likely a commercial flight operated by {}",
            nearest
                .classification
                .operator
                .as_deref()
                .unwrap_or_default()
        )
    } else if nearest.classification.kind == "military" {
        format!(
            "likely military: {}, {}",
            nearest
                .classification
                .operator
                .as_deref()
                .unwrap_or("a military operator"),
            nearest
                .classification
                .country
                .as_deref()
                .unwrap_or("an unknown country")
        )
    } else {
        "not identifiable as commercial or military from the available data".to_string()
    };

    format!(
        "{callsign} is {} away, bearing {:.0} degrees from you. It is nearest to {place}. It is {operator_text}. Reported altitude is {}, ground speed is {}, and track is {}.",
        format_distance(nearest.distance_km, units),
        nearest.bearing_degrees,
        format_meters_and_feet(state.baro_altitude),
        format_speed(state.velocity),
        format_degrees(state.true_track),
    )
}

fn format_airport(airport: Option<&Map<String, Value>>) -> String {
    let Some(airport) = airport else {
        return "unknown".to_string();
    };
    let code = format_codes([
        object_str(Some(airport), "icao_code").as_deref(),
        object_str(Some(airport), "iata_code").as_deref(),
        None,
    ]);
    let name = value_or_unknown(airport.get("name"));
    let place = [
        object_str(Some(airport), "municipality"),
        object_str(Some(airport), "country_name"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    if place.is_empty() {
        format!("{name} ({code})")
    } else {
        format!("{name} ({code}), {place}")
    }
}

/// Compact "KLAX → KSFO" route for the ROUTE panel, preferring ICAO codes (the
/// K-prefixed style shown in the mock) and falling back to IATA.
fn format_route_codes_for_ui(flightroute: Option<&Map<String, Value>>) -> String {
    let code = |key: &str| -> Option<String> {
        flightroute
            .and_then(|route| route.get(key))
            .and_then(Value::as_object)
            .and_then(|airport| {
                object_str(Some(airport), "icao_code")
                    .or_else(|| object_str(Some(airport), "iata_code"))
            })
    };
    match (code("origin"), code("destination")) {
        (Some(origin), Some(destination)) => format!("{origin} → {destination}"),
        (Some(origin), None) => format!("{origin} → ?"),
        (None, Some(destination)) => format!("? → {destination}"),
        (None, None) => "—".to_string(),
    }
}

/// City pair for the ROUTE panel ("Los Angeles → San Francisco"), shown under
/// the airport codes in place of the (rarely populated) scheduled times.
fn format_route_cities_for_ui(flightroute: Option<&Map<String, Value>>) -> String {
    let city = |key: &str| -> Option<String> {
        flightroute
            .and_then(|route| route.get(key))
            .and_then(Value::as_object)
            .and_then(|airport| object_str(Some(airport), "municipality"))
    };
    match (city("origin"), city("destination")) {
        (Some(origin), Some(destination)) => format!("{origin} → {destination}"),
        (Some(origin), None) => origin,
        (None, Some(destination)) => destination,
        (None, None) => String::new(),
    }
}

fn compact_time(value: &str) -> String {
    value
        .split('T')
        .nth(1)
        .and_then(|time| time.get(0..8))
        .map(|time| format!("{time} LT"))
        .unwrap_or_else(|| value.to_string())
}

fn format_distance(distance_km: f64, units: Units) -> String {
    match units {
        Units::Imperial => format!("{:.1} mi", distance_km * 0.621371),
        Units::Metric => format!("{distance_km:.1} km"),
    }
}

fn format_codes<const N: usize>(codes: [Option<&str>; N]) -> String {
    let present = codes
        .into_iter()
        .flatten()
        .filter(|code| !code.trim().is_empty())
        .map(safe_display)
        .collect::<Vec<_>>();
    if present.is_empty() {
        "unknown".to_string()
    } else {
        present.join(" / ")
    }
}

fn format_timestamp(value: Option<i64>) -> String {
    let Some(value) = value else {
        return "unknown".to_string();
    };
    DateTime::<Utc>::from_timestamp(value, 0)
        .map(|timestamp| timestamp.with_timezone(&Local).to_rfc3339())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_float(value: Option<f64>, decimals: usize) -> String {
    value
        .map(|value| format!("{value:.decimals$}"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_meters_and_feet(value: Option<f64>) -> String {
    value
        .map(|meters| format!("{meters:.0} m / {:.0} ft", meters * M_TO_FEET))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_speed(value: Option<f64>) -> String {
    value
        .map(|mps| format!("{mps:.1} m/s / {:.1} kt", mps * MPS_TO_KNOTS))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_degrees(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1} deg"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_vertical_rate(value: Option<f64>) -> String {
    value
        .map(|mps| format!("{mps:.1} m/s / {:.0} ft/min", mps * M_TO_FEET * 60.0))
        .unwrap_or_else(|| "unknown".to_string())
}

fn value_or_unknown(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => safe_display(value),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(value) if !value.is_null() => safe_display(&value.to_string()),
        _ => "unknown".to_string(),
    }
}

fn value_or_unknown_obj(object: Option<&Map<String, Value>>, key: &str) -> String {
    value_or_unknown(object.and_then(|object| object.get(key)))
}

fn map_value_or_unknown(map: &BTreeMap<String, String>, key: &str) -> String {
    map.get(key)
        .map(|value| safe_display(value))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn string_or_unknown(value: Option<&str>) -> String {
    value
        .map(safe_display)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn safe_display(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            let code = *character as u32;
            !(code <= 0x08
                || code == 0x0b
                || code == 0x0c
                || (0x0e..=0x1f).contains(&code)
                || (0x7f..=0x9f).contains(&code))
        })
        .collect()
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn describe_code(value: Option<i64>, lookup: fn(i64) -> Option<&'static str>) -> String {
    let Some(code) = value else {
        return "unknown".to_string();
    };
    lookup(code)
        .map(|description| format!("{code} ({description})"))
        .unwrap_or_else(|| code.to_string())
}

fn raw_state_value(state: &FlightState, field: &str) -> String {
    match field {
        "icao24" => string_or_unknown(state.icao24.as_deref()),
        "callsign" => string_or_unknown(state.callsign.as_deref()),
        "origin_country" => string_or_unknown(state.origin_country.as_deref()),
        "time_position" => state
            .time_position
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "last_contact" => state
            .last_contact
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "longitude" => state
            .longitude
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "latitude" => state
            .latitude
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "baro_altitude" => state
            .baro_altitude
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "on_ground" => state
            .on_ground
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "velocity" => state
            .velocity
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "true_track" => state
            .true_track
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "vertical_rate" => state
            .vertical_rate
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "sensors" => state
            .sensors
            .as_ref()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "geo_altitude" => state
            .geo_altitude
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "squawk" => string_or_unknown(state.squawk.as_deref()),
        "spi" => state
            .spi
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "position_source" => state
            .position_source
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        "category" => state
            .category
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        _ => "unknown".to_string(),
    }
}

fn pushln(report: &mut String, line: &str) {
    report.push_str(line);
    report.push('\n');
}

fn value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn value_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(value) => value.as_i64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn value_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

fn nested_str(root: Option<&Value>, path: &[&str]) -> Option<String> {
    let mut current = root?;
    for part in path {
        current = current.get(*part)?;
    }
    current
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn nested_object<'a>(root: Option<&'a Value>, path: &[&str]) -> Option<&'a Map<String, Value>> {
    let mut current = root?;
    for part in path {
        current = current.get(*part)?;
    }
    current.as_object().filter(|object| !object.is_empty())
}

fn object_str(object: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    object?
        .get(key)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn airline_icao(prefix: &str) -> Option<&'static str> {
    match prefix {
        "AAL" => Some("American Airlines"),
        "ACA" => Some("Air Canada"),
        "AFR" => Some("Air France"),
        "ASA" => Some("Alaska Airlines"),
        "AUA" => Some("Austrian Airlines"),
        "AVA" => Some("Avianca"),
        "BAW" => Some("British Airways"),
        "CPA" => Some("Cathay Pacific"),
        "DAL" => Some("Delta Air Lines"),
        "DLH" => Some("Lufthansa"),
        "EIN" => Some("Aer Lingus"),
        "EJU" => Some("easyJet Europe"),
        "EZY" => Some("easyJet"),
        "FDX" => Some("FedEx Express"),
        "FFT" => Some("Frontier Airlines"),
        "IBE" => Some("Iberia"),
        "JAL" => Some("Japan Airlines"),
        "JBU" => Some("JetBlue"),
        "KAL" => Some("Korean Air"),
        "KLM" => Some("KLM Royal Dutch Airlines"),
        "NKS" => Some("Spirit Airlines"),
        "QFA" => Some("Qantas"),
        "QTR" => Some("Qatar Airways"),
        "RYR" => Some("Ryanair"),
        "SAS" => Some("Scandinavian Airlines"),
        "SIA" => Some("Singapore Airlines"),
        "SWA" => Some("Southwest Airlines"),
        "TAP" => Some("TAP Air Portugal"),
        "THY" => Some("Turkish Airlines"),
        "UAL" => Some("United Airlines"),
        "UAE" => Some("Emirates"),
        "UPS" => Some("UPS Airlines"),
        "VIR" => Some("Virgin Atlantic"),
        "VOI" => Some("Volaris"),
        "WJA" => Some("WestJet"),
        "WZZ" => Some("Wizz Air"),
        _ => None,
    }
}

fn military_callsigns() -> [(&'static str, &'static str, &'static str); 12] {
    [
        ("ASY", "Australia", "Royal Australian Air Force"),
        ("CFC", "Canada", "Canadian Armed Forces"),
        ("CNV", "United States", "United States Navy"),
        ("DUKE", "United States", "United States Air Force"),
        (
            "EVAC",
            "United States",
            "United States military medical flight",
        ),
        ("FAF", "France", "French Air and Space Force"),
        ("GAF", "Germany", "German Air Force"),
        ("IAM", "Italy", "Italian Air Force"),
        ("NATO", "NATO", "NATO"),
        (
            "RCH",
            "United States",
            "United States Air Force Air Mobility Command",
        ),
        ("RRR", "United Kingdom", "Royal Air Force"),
        (
            "SAM",
            "United States",
            "United States Air Force Special Air Mission",
        ),
    ]
}

fn position_source_description(code: i64) -> Option<&'static str> {
    match code {
        0 => Some("ADS-B"),
        1 => Some("ASTERIX"),
        2 => Some("MLAT"),
        3 => Some("FLARM"),
        _ => None,
    }
}

fn aircraft_category_description(code: i64) -> Option<&'static str> {
    match code {
        0 => Some("No information"),
        1 => Some("No ADS-B emitter category information"),
        2 => Some("Light (< 15500 lbs)"),
        3 => Some("Small (15500 to 75000 lbs)"),
        4 => Some("Large (75000 to 300000 lbs)"),
        5 => Some("High vortex large"),
        6 => Some("Heavy (> 300000 lbs)"),
        7 => Some("High performance"),
        8 => Some("Rotorcraft"),
        9 => Some("Glider / sailplane"),
        10 => Some("Lighter-than-air"),
        11 => Some("Parachutist / skydiver"),
        12 => Some("Ultralight / hang-glider / paraglider"),
        13 => Some("Reserved"),
        14 => Some("Unmanned aerial vehicle"),
        15 => Some("Space / trans-atmospheric vehicle"),
        16 => Some("Surface vehicle - emergency vehicle"),
        17 => Some("Surface vehicle - service vehicle"),
        18 => Some("Point obstacle"),
        19 => Some("Cluster obstacle"),
        20 => Some("Line obstacle"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn haversine_known_distance() {
        let distance = haversine_km(40.7128, -74.0060, 34.0522, -118.2437);
        assert!((distance - 3935.75).abs() / 3935.75 < 0.01);
    }

    #[test]
    fn bearing_is_normalized() {
        let bearing = bearing_degrees(40.7128, -74.0060, 34.0522, -118.2437);
        assert!((0.0..360.0).contains(&bearing));
    }

    #[test]
    fn bounding_box_contains_origin() {
        let box_ = bounding_box(40.0, -70.0, 100.0);
        assert!(box_.lamin < 40.0);
        assert!(box_.lamax > 40.0);
        assert!(box_.lomin < -70.0);
        assert!(box_.lomax > -70.0);
    }

    #[test]
    fn parse_state_trims_callsign() {
        let raw_state = vec![
            json!("abc123"),
            json!(" DAL123 "),
            json!("United States"),
            Value::Null,
            json!(1),
            json!(-70.0),
            json!(40.0),
        ];
        let state = parse_state(&raw_state);
        assert_eq!(state.callsign.as_deref(), Some("DAL123"));
        assert_eq!(state.icao24.as_deref(), Some("abc123"));
        assert_eq!(state.time_position, None);
    }

    #[test]
    fn radius_validation_rejects_unbounded_values() {
        assert!(validate_radius_km(f64::INFINITY).is_err());
        assert!(validate_radius_km(0.0).is_err());
        assert!(validate_radius_km(1000.1).is_err());
        assert!(validate_radius_km(1000.0).is_ok());
    }

    #[test]
    fn safe_display_removes_control_characters() {
        assert_eq!(safe_display("ok\u{1b}[31mred\u{00}"), "ok[31mred");
    }

    #[test]
    fn classifies_known_airline_callsign() {
        let state = FlightState {
            callsign: Some("DAL123".to_string()),
            origin_country: Some("United States".to_string()),
            ..FlightState::default()
        };
        let classification = classify_aircraft(&state, None, None);
        assert_eq!(classification.kind, "commercial");
        assert_eq!(classification.operator.as_deref(), Some("Delta Air Lines"));
    }

    #[test]
    fn classifies_adsbdb_airline_metadata() {
        let state = FlightState {
            callsign: Some("UNKNOWN1".to_string()),
            origin_country: Some("United States".to_string()),
            ..FlightState::default()
        };
        let metadata = json!({
            "flightroute": {
                "airline": {
                    "name": "Example Air",
                    "country": "Canada"
                }
            }
        });
        let classification = classify_aircraft(&state, Some(&metadata), None);
        assert_eq!(classification.kind, "commercial");
        assert_eq!(classification.operator.as_deref(), Some("Example Air"));
        assert_eq!(classification.country.as_deref(), Some("Canada"));
    }

    #[test]
    fn converts_us_mode_s_hex_to_n_number() {
        assert_eq!(mode_s_hex_to_n_number("A0BE8F").as_deref(), Some("N147VC"));
        assert_eq!(mode_s_hex_to_n_number("a00001").as_deref(), Some("N1"));
        assert_eq!(mode_s_hex_to_n_number("400000"), None);
    }

    #[test]
    fn parses_faa_inquiry_table_fields() {
        let html = r#"
        <table>
          <tr><td>Manufacturer Name</td><td>CIRRUS DESIGN CORP</td></tr>
          <tr><td>Model</td><td>SR22</td></tr>
          <tr><td>Status</td><td>Valid</td></tr>
          <tr><td>Name</td><td>0689 HOLDINGS INC TRUSTEE</td></tr>
          <tr><td>City</td><td>WILMINGTON</td></tr>
          <tr><td>State</td><td>DELAWARE</td></tr>
        </table>
        "#;
        let fields = parse_faa_inquiry_fields(html);
        assert_eq!(
            fields.get("manufacturer").map(String::as_str),
            Some("CIRRUS DESIGN CORP")
        );
        assert_eq!(fields.get("model").map(String::as_str), Some("SR22"));
        assert_eq!(
            fields.get("registered_owner_name").map(String::as_str),
            Some("0689 HOLDINGS INC TRUSTEE")
        );
        assert_eq!(
            fields.get("registered_owner_city").map(String::as_str),
            Some("WILMINGTON")
        );
    }

    #[test]
    fn distance_formatting_units() {
        assert_eq!(format_distance(10.0, Units::Metric), "10.0 km");
        assert_eq!(format_distance(10.0, Units::Imperial), "6.2 mi");
    }
}
