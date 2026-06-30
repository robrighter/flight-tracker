use chrono::{DateTime, Utc};
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
use std::sync::OnceLock;
#[cfg(windows)]
use std::sync::Mutex;
use std::time::Duration;
#[cfg(windows)]
use windows::Devices::Geolocation::{Geolocator, PositionAccuracy};
#[cfg(windows)]
use windows::Foundation::TimeSpan;
#[cfg(windows)]
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
    DeleteDC, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE,
    DT_VCENTER, DT_WORDBREAK, DeleteObject, DrawTextW, Ellipse, EndPaint, FF_DONTCARE, FW_BOLD,
    FW_NORMAL, FillRect, HDC, HGDIOBJ, InvalidateRect, LineTo, MoveToEx, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, PS_SOLID, Polygon, RoundRect, SRCCOPY, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, UpdateWindow,
};
#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetClientRect, GetMessageW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW,
    SW_SHOW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY, WM_ERASEBKGND,
    WM_LBUTTONDOWN, WM_PAINT, WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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
    #[arg(
        long,
        default_value_t = 60,
        help = "Desktop UI refresh interval hint (manual refresh button for now)."
    )]
    refresh_seconds: u64,
    #[arg(long, value_enum, default_value_t = Units::Imperial)]
    units: Units,
    #[arg(long, help = "Reserved for detailed future UI errors.")]
    debug: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
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
    operator: String,
    route_detail: String,
    summary: String,
    altitude: String,
    speed: String,
    heading: String,
    registry: String,
    aircraft_type: String,
    squawk: String,
    position: String,
    updated: String,
    location: String,
}

#[cfg(windows)]
impl UiModel {
    fn from_flight(location: &Location, nearest: &NearestAircraft, units: Units) -> Self {
        let state = &nearest.state;
        let metadata = nearest.metadata.as_ref();
        let aircraft = nested_object(metadata, &["aircraft"]);
        let flightroute = nested_object(metadata, &["flightroute"]);
        let route_detail = format_route_detail_for_ui(flightroute);
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
        // `units` currently affects distance/summary phrasing only; motion is
        // shown in standard aviation units (knots, flight level) like the mock.
        let _ = units;

        Self {
            operator,
            route_detail,
            summary: human_summary(nearest, units),
            altitude: ui_flight_level(state.baro_altitude),
            speed: ui_speed(state.velocity),
            heading: ui_heading(state.true_track),
            registry,
            aircraft_type,
            squawk: string_or_unknown(state.squawk.as_deref()),
            position: format!(
                "{}, {}",
                format_float(state.latitude, 4),
                format_float(state.longitude, 4)
            ),
            updated,
            location: location.label.clone(),
        }
    }
}

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
    status: String,
}

impl UiState {
    fn new(
        client: ApiClient,
        location: Location,
        radius_km: f64,
        units: Units,
        aircraft: Vec<NearestAircraft>,
    ) -> Self {
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
            status: "LIVE ADS-B".to_string(),
        };
        if !state.aircraft.is_empty() {
            state.enrich_selected();
        }
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

    fn select(&mut self, index: usize) {
        if index >= self.aircraft.len() || index == self.selected {
            return;
        }
        self.selected = index;
        self.enrich_selected();
        self.rebuild_report();
    }

    fn set_units(&mut self, units: Units) {
        self.units = units;
        self.rebuild_report();
    }

    fn rebuild_report(&mut self) {
        self.report = match self.selected_aircraft() {
            Some(aircraft) => {
                build_report(&self.location, aircraft, self.radius_km, self.units)
            }
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

fn main() -> ExitCode {
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
        let location = resolve_location(&args, &client)?;
        let aircraft = find_nearby_aircraft(&location, args.radius_km, &client, UI_LIST_LIMIT)?;
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
    overview_tab: RECT,
    raw_tab: RECT,
    nm_btn: RECT,
    km_btn: RECT,
    refresh_btn: RECT,
    main: RECT,
    board: RECT,
    radar: RECT,
    bottom: RECT,
    board_clip: RECT,
    row_y: i32,
    row_h: i32,
}

#[cfg(windows)]
impl Layout {
    fn row_rect(&self, index: usize) -> RECT {
        let top = self.row_y + self.row_h * index as i32;
        rect_xy(self.board_clip.left, top, self.board_clip.right, top + self.row_h)
    }
}

#[cfg(windows)]
fn compute_layout(width: i32, height: i32) -> Layout {
    let top_h = 72;
    let gutter = 14;
    let main_top = top_h + 10;
    let bottom_h = 180;
    let main_bottom = height - bottom_h - 12;
    let left_w = ((width as f32) * 0.55) as i32;

    let board = rect_xy(gutter, main_top, left_w, main_bottom);
    let board_clip = inset(board, 16, 14);
    let header_y = board_clip.top + 58;
    let row_y = header_y + 34;

    Layout {
        overview_tab: rect_xy(width / 2 - 200, 13, width / 2 - 60, 59),
        raw_tab: rect_xy(width / 2 - 55, 13, width / 2 + 85, 59),
        nm_btn: rect_xy(width - 470, 18, width - 420, 54),
        km_btn: rect_xy(width - 414, 18, width - 364, 54),
        refresh_btn: rect_xy(width - 150, 15, width - 20, 57),
        main: rect_xy(gutter, main_top, width - gutter, main_bottom),
        board,
        radar: rect_xy(left_w + 10, main_top, width - gutter, main_bottom),
        bottom: rect_xy(gutter, main_bottom + 12, width - gutter, height - 14),
        board_clip,
        row_y,
        row_h: 82,
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
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dashboard_window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&window_class);

        let title = to_wide("Flight Tracker");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1160,
            860,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|error| FlightTrackerError(format!("could not create window: {error}")))?;

        let _ = ShowWindow(hwnd, SW_SHOW);

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
            // We repaint the whole client area from a back buffer, so suppress
            // the default background erase to avoid flicker.
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                handle_click(hwnd, x, y);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
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
    let width = (client.right - client.left).max(1040);
    let height = (client.bottom - client.top).max(720);
    let layout = compute_layout(width, height);

    let mut changed = true;
    let mut needs_blocking_refresh = false;
    if point_in(layout.overview_tab, x, y) {
        state.tab = Tab::Overview;
    } else if point_in(layout.raw_tab, x, y) {
        state.tab = Tab::RawReport;
    } else if point_in(layout.nm_btn, x, y) {
        state.set_units(Units::Imperial);
    } else if point_in(layout.km_btn, x, y) {
        state.set_units(Units::Metric);
    } else if point_in(layout.refresh_btn, x, y) {
        needs_blocking_refresh = true;
    } else if state.tab == Tab::Overview {
        let mut hit = false;
        for index in 0..state.aircraft.len().min(UI_LIST_LIMIT) {
            if point_in(layout.row_rect(index), x, y) {
                state.select(index);
                hit = true;
                break;
            }
        }
        changed = hit;
    } else {
        changed = false;
    }

    if needs_blocking_refresh {
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
}

#[cfg(windows)]
fn paint_dashboard(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
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
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

#[cfg(windows)]
fn draw_dashboard(hdc: HDC, rect: RECT, state: &UiState) {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let width = (rect.right - rect.left).max(1040);
        let height = (rect.bottom - rect.top).max(720);
        let layout = compute_layout(width, height);
        fill_rect(hdc, rect_xy(0, 0, width, height), rgb(12, 24, 33));

        let model = state
            .selected_aircraft()
            .map(|aircraft| UiModel::from_flight(&state.location, aircraft, state.units));

        let top_h = 72;
        fill_rect(hdc, rect_xy(0, 0, width, top_h), rgb(15, 27, 37));
        fill_rect(hdc, rect_xy(0, top_h - 3, width, top_h), rgb(153, 111, 44));
        draw_text(
            hdc,
            "\u{2708}  FLIGHT TRACKER",
            rect_xy(34, 16, 360, 58),
            28,
            true,
            rgb(239, 225, 195),
            Align::Left,
        );
        draw_segment(hdc, "OVERVIEW", layout.overview_tab, state.tab == Tab::Overview);
        draw_segment(hdc, "RAW REPORT", layout.raw_tab, state.tab == Tab::RawReport);
        draw_segment(hdc, "NM", layout.nm_btn, matches!(state.units, Units::Imperial));
        draw_segment(hdc, "KM", layout.km_btn, matches!(state.units, Units::Metric));

        let updated = match &model {
            Some(model) => format!("UPDATED  {}", compact_time(&model.updated)),
            None => state.status.clone(),
        };
        draw_text(
            hdc,
            &updated,
            rect_xy(width - 352, 18, layout.refresh_btn.left - 14, 54),
            18,
            true,
            rgb(240, 226, 196),
            Align::Right,
        );
        draw_button(hdc, "\u{21bb} REFRESH", layout.refresh_btn);

        match state.tab {
            Tab::Overview => {
                draw_board(hdc, &layout, state);
                draw_radar(hdc, layout.radar, state);
                draw_bottom_panels(hdc, layout.bottom, model.as_ref(), &state.status);
            }
            Tab::RawReport => {
                draw_raw_report(hdc, layout.main, &state.report);
            }
        }
    }
}

/// One contact rendered into the NEAREST flight-strip board.
#[cfg(windows)]
struct RowView {
    callsign: String,
    sub: String,
    operator: String,
    route: String,
    motion_top: String,
    motion_bot: String,
}

#[cfg(windows)]
fn row_view(aircraft: &NearestAircraft, units: Units) -> RowView {
    let state = &aircraft.state;
    let route = nested_object(aircraft.metadata.as_ref(), &["flightroute"])
        .map(|flightroute| format_primary_route_for_ui(Some(flightroute)))
        .filter(|route| !route.is_empty())
        .unwrap_or_else(|| "\u{2014}".to_string());
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
        motion_top: format!(
            "{}  {}",
            ui_heading(state.true_track),
            ui_speed(state.velocity)
        ),
        motion_bot: ui_flight_level(state.baro_altitude),
    }
}

#[cfg(windows)]
fn draw_board(hdc: HDC, layout: &Layout, state: &UiState) {
    let area = layout.board;
    draw_panel(hdc, area, rgb(228, 211, 176), rgb(97, 69, 39));
    let clip = layout.board_clip;
    draw_text(
        hdc,
        "NEAREST",
        rect_xy(clip.left, clip.top, clip.right, clip.top + 38),
        24,
        true,
        rgb(139, 26, 22),
        Align::Center,
    );
    draw_line(
        hdc,
        clip.left,
        clip.top + 43,
        clip.right,
        clip.top + 43,
        rgb(151, 114, 61),
        1,
    );

    let header_y = clip.top + 58;
    // Columns are proportional to the (narrow) board width so the OPERATOR /
    // ROUTE / MOTION headers never collide regardless of window size.
    let w = clip.right - clip.left;
    let col = [
        clip.left + 72,
        clip.left + w * 34 / 100,
        clip.left + w * 57 / 100,
        clip.left + w * 77 / 100,
    ];
    for (label, x) in [
        ("SUMMARY", col[0]),
        ("OPERATOR", col[1]),
        ("ROUTE", col[2]),
        ("MOTION", col[3]),
    ] {
        draw_text(
            hdc,
            label,
            rect_xy(x, header_y, x + 140, header_y + 24),
            14,
            true,
            rgb(30, 35, 35),
            Align::Left,
        );
    }

    let row_h = layout.row_h;
    let count = state.aircraft.len().min(UI_LIST_LIMIT);
    for idx in 0..UI_LIST_LIMIT {
        let row = layout.row_rect(idx);
        let selected = idx == state.selected && idx < count;
        let fill = if selected {
            rgb(239, 220, 185)
        } else if idx % 2 == 0 {
            rgb(234, 216, 182)
        } else {
            rgb(226, 207, 172)
        };
        fill_rect(hdc, row, fill);

        if idx >= count {
            continue;
        }
        let view = row_view(&state.aircraft[idx], state.units);

        // Oxblood selection rail with a marker arrow, matching the mock's
        // highlighted flight strip.
        if selected {
            fill_rect(
                hdc,
                rect_xy(row.left, row.top, row.left + 48, row.bottom),
                rgb(127, 22, 20),
            );
            draw_text(
                hdc,
                "\u{25B6}",
                rect_xy(row.left + 14, row.top + 28, row.left + 40, row.top + 56),
                18,
                true,
                rgb(238, 225, 195),
                Align::Center,
            );
        }

        let text_dark = rgb(21, 31, 35);
        draw_text(
            hdc,
            &view.callsign,
            rect_xy(clip.left + 72, row.top + 14, col[1] - 10, row.top + 45),
            22,
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.sub,
            rect_xy(clip.left + 72, row.top + 46, col[1] - 12, row.top + 70),
            14,
            true,
            rgb(141, 26, 22),
            Align::Left,
        );
        draw_text(
            hdc,
            &view.operator,
            rect_xy(col[1], row.top + 24, col[2] - 12, row.top + 58),
            18,
            false,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.route,
            rect_xy(col[2], row.top + 24, col[3] - 12, row.top + 58),
            18,
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.motion_top,
            rect_xy(col[3], row.top + 16, clip.right - 16, row.top + 44),
            16,
            true,
            text_dark,
            Align::Left,
        );
        draw_text(
            hdc,
            &view.motion_bot,
            rect_xy(col[3], row.top + 46, clip.right - 16, row.top + 70),
            15,
            false,
            rgb(80, 60, 40),
            Align::Left,
        );
    }

    for x in [col[1] - 18, col[2] - 14, col[3] - 16] {
        draw_line(hdc, x, header_y + 6, x, clip.bottom - 12, rgb(185, 145, 83), 1);
    }
    for idx in 0..=UI_LIST_LIMIT {
        let y = layout.row_y + row_h * idx as i32;
        draw_line(hdc, clip.left, y, clip.right, y, rgb(183, 141, 82), 1);
    }
}

#[cfg(windows)]
fn draw_radar(hdc: HDC, area: RECT, state: &UiState) {
    draw_panel(hdc, area, rgb(44, 47, 45), rgb(108, 91, 66));
    let size = (area.right - area.left).min(area.bottom - area.top) - 54;
    let cx = (area.left + area.right) / 2;
    let cy = (area.top + area.bottom) / 2;
    let radius = size / 2;
    fill_ellipse(
        hdc,
        cx - radius,
        cy - radius,
        cx + radius,
        cy + radius,
        rgb(9, 39, 55),
        rgb(185, 155, 93),
        5,
    );
    fill_ellipse(
        hdc,
        cx - radius + 18,
        cy - radius + 18,
        cx + radius - 18,
        cy + radius - 18,
        rgb(10, 48, 68),
        rgb(88, 104, 102),
        1,
    );

    for ring in [1, 2, 3] {
        let r = radius * ring / 4;
        ellipse_outline(hdc, cx - r, cy - r, cx + r, cy + r, rgb(100, 128, 126), 1);
    }
    draw_line(
        hdc,
        cx - radius + 24,
        cy,
        cx + radius - 24,
        cy,
        rgb(100, 128, 126),
        1,
    );
    draw_line(
        hdc,
        cx,
        cy - radius + 24,
        cx,
        cy + radius - 24,
        rgb(100, 128, 126),
        1,
    );
    for (label, x, y) in [
        ("N", cx - 12, cy - radius + 12),
        ("E", cx + radius - 34, cy - 12),
        ("S", cx - 10, cy + radius - 40),
        ("W", cx - radius + 16, cy - 12),
    ] {
        draw_text(
            hdc,
            label,
            rect_xy(x, y, x + 32, y + 34),
            28,
            true,
            rgb(236, 211, 161),
            Align::Center,
        );
    }

    // Centre marker for the observer's own position.
    fill_ellipse(hdc, cx - 4, cy - 4, cx + 4, cy + 4, rgb(236, 211, 161), rgb(236, 211, 161), 1);

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
    let count = state.aircraft.len().min(UI_LIST_LIMIT);
    // Plot non-selected contacts first so the selected one paints on top.
    for idx in 0..count {
        if idx == state.selected {
            continue;
        }
        let aircraft = &state.aircraft[idx];
        let (bx, by) = radar_point(cx, cy, usable, span, aircraft);
        draw_plane_marker(
            hdc,
            bx,
            by,
            8,
            aircraft.state.true_track,
            rgb(120, 196, 224),
            rgb(40, 92, 112),
        );
    }

    if let Some(aircraft) = state.aircraft.get(state.selected) {
        let (tx, ty) = radar_point(cx, cy, usable, span, aircraft);
        draw_line(hdc, cx, cy, tx, ty, rgb(240, 190, 36), 2);
        fill_ellipse(
            hdc,
            tx - 16,
            ty - 16,
            tx + 16,
            ty + 16,
            rgb(99, 35, 26),
            rgb(240, 190, 36),
            2,
        );
        draw_plane_marker(hdc, tx, ty, 9, aircraft.state.true_track, rgb(255, 225, 138), rgb(120, 70, 20));
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
                ui_distance(aircraft.distance_km, state.units),
                compass16(aircraft.bearing_degrees),
                ui_flight_level(aircraft.state.baro_altitude)
            ),
            rect_xy(tx + 22, ty - 14, tx + 168, ty + 70),
            15,
            true,
            rgb(242, 218, 171),
            Align::Left,
        );
    }

    draw_text(
        hdc,
        &state.status,
        rect_xy(
            area.left + 24,
            area.bottom - 46,
            area.right - 28,
            area.bottom - 20,
        ),
        15,
        true,
        rgb(126, 220, 118),
        Align::Right,
    );
}

/// Project an aircraft's bearing/distance onto the radar scope (North up).
#[cfg(windows)]
fn radar_point(cx: i32, cy: i32, usable: f64, span_km: f64, aircraft: &NearestAircraft) -> (i32, i32) {
    let r = usable * (aircraft.distance_km / span_km).clamp(0.0, 1.0);
    let angle = (aircraft.bearing_degrees - 90.0).to_radians();
    (
        cx + (angle.cos() * r) as i32,
        cy + (angle.sin() * r) as i32,
    )
}

#[cfg(windows)]
fn draw_bottom_panels(hdc: HDC, area: RECT, model: Option<&UiModel>, status: &str) {
    let gap = 8;
    let total_w = area.right - area.left;
    let Some(model) = model else {
        let panel = rect_xy(area.left, area.top, area.right, area.bottom);
        draw_panel(hdc, panel, rgb(226, 207, 174), rgb(99, 72, 43));
        draw_text(
            hdc,
            status,
            inset(panel, 20, 16),
            18,
            true,
            rgb(113, 25, 23),
            Align::Left,
        );
        return;
    };
    let widths = [
        total_w * 24 / 100,
        total_w * 15 / 100,
        total_w * 15 / 100,
        total_w * 13 / 100,
        total_w * 18 / 100,
        total_w * 15 / 100,
    ];
    let mut x = area.left;
    let registry_body = format!("{}\n{}", model.registry, model.aircraft_type);
    let motion_body = format!(
        "HDG {}\nSPD {}\nALT {}\nSQK {}",
        model.heading, model.speed, model.altitude, model.squawk
    );
    let updated_body = format!(
        "{}\n{}\n{}",
        compact_time(&model.updated),
        model.location,
        model.position
    );
    let panels = [
        ("SUMMARY", model.summary.as_str()),
        ("OPERATOR", model.operator.as_str()),
        ("ROUTE", model.route_detail.as_str()),
        ("REGISTRY", registry_body.as_str()),
        ("MOTION", motion_body.as_str()),
        ("UPDATED", updated_body.as_str()),
    ];
    for (idx, (title, body)) in panels.iter().enumerate() {
        let right = if idx == panels.len() - 1 {
            area.right
        } else {
            x + widths[idx]
        };
        let panel = rect_xy(x, area.top, right - gap, area.bottom);
        draw_panel(hdc, panel, rgb(226, 207, 174), rgb(99, 72, 43));
        draw_text(
            hdc,
            title,
            rect_xy(
                panel.left + 16,
                panel.top + 10,
                panel.right - 14,
                panel.top + 34,
            ),
            15,
            true,
            rgb(32, 34, 32),
            Align::Left,
        );
        draw_line(
            hdc,
            panel.left + 16,
            panel.top + 38,
            panel.right - 16,
            panel.top + 38,
            rgb(126, 94, 55),
            1,
        );
        draw_text(
            hdc,
            body,
            rect_xy(
                panel.left + 18,
                panel.top + 48,
                panel.right - 18,
                panel.bottom - 14,
            ),
            if idx == 0 { 15 } else { 18 },
            idx != 0,
            if idx == 1 {
                rgb(113, 25, 23)
            } else {
                rgb(22, 33, 38)
            },
            Align::Left,
        );
        x = right;
    }
}

/// Full text report panel shown on the RAW REPORT tab.
#[cfg(windows)]
fn draw_raw_report(hdc: HDC, area: RECT, report: &str) {
    draw_panel(hdc, area, rgb(228, 211, 176), rgb(97, 69, 39));
    let clip = inset(area, 24, 18);
    draw_text(
        hdc,
        "RAW REPORT",
        rect_xy(clip.left, clip.top, clip.right, clip.top + 30),
        20,
        true,
        rgb(139, 26, 22),
        Align::Left,
    );
    draw_line(
        hdc,
        clip.left,
        clip.top + 36,
        clip.right,
        clip.top + 36,
        rgb(151, 114, 61),
        1,
    );
    draw_mono_text(
        hdc,
        report,
        rect_xy(clip.left, clip.top + 46, clip.right, clip.bottom),
        15,
        rgb(28, 36, 38),
    );
}

/// A brass push-button used for the REFRESH control.
#[cfg(windows)]
fn draw_button(hdc: HDC, label: &str, rect: RECT) {
    draw_panel(hdc, rect, rgb(118, 18, 17), rgb(176, 132, 66));
    draw_text(hdc, label, inset(rect, 8, 4), 16, true, rgb(244, 228, 196), Align::Center);
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
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW",
        "NNW",
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
fn draw_panel(hdc: HDC, rect: RECT, fill: COLORREF, border: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, 2, border);
        let old_brush = SelectObject(hdc, HGDIOBJ::from(brush));
        let old_pen = SelectObject(hdc, HGDIOBJ::from(pen));
        let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 10, 10);
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ::from(pen));
        let _ = DeleteObject(HGDIOBJ::from(brush));
    }
}

#[cfg(windows)]
fn draw_segment(hdc: HDC, label: &str, rect: RECT, selected: bool) {
    draw_panel(
        hdc,
        rect,
        if selected {
            rgb(118, 18, 17)
        } else {
            rgb(18, 31, 42)
        },
        rgb(88, 72, 53),
    );
    draw_text(
        hdc,
        label,
        inset(rect, 6, 4),
        16,
        true,
        rgb(238, 222, 188),
        Align::Center,
    );
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
        let face = to_wide("Segoe UI");
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
        let mut format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
            format.0 | DT_WORDBREAK.0 | DT_END_ELLIPSIS.0,
        );
        if !text.contains('\n') {
            format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
                format.0 | DT_SINGLELINE.0 | DT_VCENTER.0,
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
fn ellipse_outline(
    hdc: HDC,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color: COLORREF,
    width: i32,
) {
    fill_ellipse(hdc, left, top, right, bottom, rgb(10, 48, 68), color, width);
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

fn format_primary_route_for_ui(flightroute: Option<&Map<String, Value>>) -> String {
    let Some(flightroute) = flightroute else {
        return "Origin unknown -> destination unknown".to_string();
    };
    let origin = flightroute.get("origin").and_then(Value::as_object);
    let destination = flightroute.get("destination").and_then(Value::as_object);
    format!(
        "{} -> {}",
        short_airport_for_ui(origin),
        short_airport_for_ui(destination)
    )
}

fn format_route_detail_for_ui(flightroute: Option<&Map<String, Value>>) -> String {
    let Some(flightroute) = flightroute else {
        return "No route match found.".to_string();
    };
    let airline = flightroute.get("airline").and_then(Value::as_object);
    format!(
        "{}\n{}",
        value_or_unknown_obj(airline, "name"),
        format_primary_route_for_ui(Some(flightroute))
    )
}

fn short_airport_for_ui(airport: Option<&Map<String, Value>>) -> String {
    let Some(airport) = airport else {
        return "unknown".to_string();
    };
    let code =
        object_str(Some(airport), "iata_code").or_else(|| object_str(Some(airport), "icao_code"));
    let name =
        object_str(Some(airport), "municipality").or_else(|| object_str(Some(airport), "name"));
    match (name, code) {
        (Some(name), Some(code)) => format!("{name} ({code})"),
        (Some(name), None) => name,
        (None, Some(code)) => code,
        (None, None) => "unknown".to_string(),
    }
}

fn compact_time(value: &str) -> String {
    value
        .split('T')
        .nth(1)
        .and_then(|time| time.get(0..8))
        .map(|time| format!("{time} Z"))
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
        .map(|timestamp| timestamp.to_rfc3339())
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
