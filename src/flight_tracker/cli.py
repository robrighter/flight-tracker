from __future__ import annotations

import argparse
import io
import json
import math
import os
import re
import sys
import threading
from contextlib import redirect_stdout
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from html.parser import HTMLParser
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

IPAPI_URL = "https://ipapi.co/json/"
NOMINATIM_REVERSE_URL = "https://nominatim.openstreetmap.org/reverse"
ADSBDB_API_URL = "https://api.adsbdb.com/v0"
FAA_N_NUMBER_URL = "https://registry.faa.gov/AircraftInquiry/Search/NNumberResult"
OPENSKY_API_URL = "https://opensky-network.org/api"
OPENSKY_TOKEN_URL = (
    "https://auth.opensky-network.org/auth/realms/opensky-network/"
    "protocol/openid-connect/token"
)
USER_AGENT = "flight-tracker-cli/0.1"
EARTH_RADIUS_KM = 6371.0088
MPS_TO_KNOTS = 1.9438444924406
M_TO_FEET = 3.2808398950131
US_ICAO24_START = 0xA00001
US_ICAO24_COUNT = 915399
N_NUMBER_LETTERS = "ABCDEFGHJKLMNPQRSTUVWXYZ"
MAX_RADIUS_KM = 1000.0
MAX_JSON_RESPONSE_BYTES = 5 * 1024 * 1024
MAX_TEXT_RESPONSE_BYTES = 2 * 1024 * 1024
CONTROL_CHARS = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]")


STATE_FIELDS = [
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
    "category",
]

POSITION_SOURCE = {
    0: "ADS-B",
    1: "ASTERIX",
    2: "MLAT",
    3: "FLARM",
}

AIRCRAFT_CATEGORY = {
    0: "No information",
    1: "No ADS-B emitter category information",
    2: "Light (< 15500 lbs)",
    3: "Small (15500 to 75000 lbs)",
    4: "Large (75000 to 300000 lbs)",
    5: "High vortex large",
    6: "Heavy (> 300000 lbs)",
    7: "High performance",
    8: "Rotorcraft",
    9: "Glider / sailplane",
    10: "Lighter-than-air",
    11: "Parachutist / skydiver",
    12: "Ultralight / hang-glider / paraglider",
    13: "Reserved",
    14: "Unmanned aerial vehicle",
    15: "Space / trans-atmospheric vehicle",
    16: "Surface vehicle - emergency vehicle",
    17: "Surface vehicle - service vehicle",
    18: "Point obstacle",
    19: "Cluster obstacle",
    20: "Line obstacle",
}

AIRLINE_ICAO = {
    "AAL": "American Airlines",
    "ACA": "Air Canada",
    "AFR": "Air France",
    "ASA": "Alaska Airlines",
    "AUA": "Austrian Airlines",
    "AVA": "Avianca",
    "BAW": "British Airways",
    "CPA": "Cathay Pacific",
    "DAL": "Delta Air Lines",
    "DLH": "Lufthansa",
    "EIN": "Aer Lingus",
    "EJU": "easyJet Europe",
    "EZY": "easyJet",
    "FDX": "FedEx Express",
    "FFT": "Frontier Airlines",
    "IBE": "Iberia",
    "JAL": "Japan Airlines",
    "JBU": "JetBlue",
    "KAL": "Korean Air",
    "KLM": "KLM Royal Dutch Airlines",
    "NKS": "Spirit Airlines",
    "QFA": "Qantas",
    "QTR": "Qatar Airways",
    "RYR": "Ryanair",
    "SAS": "Scandinavian Airlines",
    "SIA": "Singapore Airlines",
    "SWA": "Southwest Airlines",
    "TAP": "TAP Air Portugal",
    "THY": "Turkish Airlines",
    "UAL": "United Airlines",
    "UAE": "Emirates",
    "UPS": "UPS Airlines",
    "VIR": "Virgin Atlantic",
    "VOI": "Volaris",
    "WJA": "WestJet",
    "WZZ": "Wizz Air",
}

MILITARY_CALLSIGNS = {
    "ASY": ("Australia", "Royal Australian Air Force"),
    "CFC": ("Canada", "Canadian Armed Forces"),
    "CNV": ("United States", "United States Navy"),
    "DUKE": ("United States", "United States Air Force"),
    "EVAC": ("United States", "United States military medical flight"),
    "FAF": ("France", "French Air and Space Force"),
    "GAF": ("Germany", "German Air Force"),
    "IAM": ("Italy", "Italian Air Force"),
    "NATO": ("NATO", "NATO"),
    "RCH": ("United States", "United States Air Force Air Mobility Command"),
    "RRR": ("United Kingdom", "Royal Air Force"),
    "SAM": ("United States", "United States Air Force Special Air Mission"),
    "VV": ("United States", "United States Navy"),
}


class FlightTrackerError(RuntimeError):
    """Raised when the CLI cannot retrieve or process flight data."""


@dataclass(frozen=True)
class Location:
    latitude: float
    longitude: float
    label: str
    source: str


@dataclass(frozen=True)
class Place:
    label: str
    city: str | None
    region: str | None
    country: str | None
    source: str


@dataclass(frozen=True)
class NearestAircraft:
    distance_km: float
    bearing_degrees: float
    state: dict[str, Any]
    nearest_place: Place | None
    metadata: dict[str, Any] | None
    faa: dict[str, Any] | None
    classification: dict[str, Any]
    api_time: int | None


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.ui:
        if args.json:
            print("error: --ui and --json cannot be used together", file=sys.stderr)
            return 1
        return run_ui(args)

    try:
        location = resolve_location(args)
        nearest = find_nearest_aircraft(location, args.radius_km, args.timeout)
    except FlightTrackerError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.json:
        print(
            json.dumps(
                {
                    "location": asdict(location),
                    "nearest_aircraft": asdict(nearest),
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        print_report(location, nearest, args.radius_km, args.units)

    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Find the nearest aircraft to your current location."
    )
    parser.add_argument("--lat", type=float, help="Latitude in decimal degrees.")
    parser.add_argument("--lon", type=float, help="Longitude in decimal degrees.")
    parser.add_argument(
        "--radius-km",
        type=float,
        default=150.0,
        help=f"Search radius around the location. Default: 150. Max: {MAX_RADIUS_KM:g}.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=15.0,
        help="HTTP request timeout in seconds. Default: 15.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON instead of a text report.",
    )
    parser.add_argument(
        "--ui",
        action="store_true",
        help="Open a window that refreshes the nearest-aircraft report.",
    )
    parser.add_argument(
        "--refresh-seconds",
        type=int,
        default=60,
        help="UI refresh interval in seconds. Default: 60.",
    )
    parser.add_argument(
        "--units",
        choices=["metric", "imperial"],
        default="imperial",
        help="Distance units for the report. Default: imperial.",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Show detailed unexpected UI errors.",
    )
    return parser


def run_ui(args: argparse.Namespace) -> int:
    try:
        import tkinter as tk
        from tkinter import ttk
    except ImportError as exc:
        print("error: Tkinter is not available in this Python installation", file=sys.stderr)
        return 1

    if args.refresh_seconds <= 0:
        print("error: --refresh-seconds must be greater than zero", file=sys.stderr)
        return 1

    root = tk.Tk()
    root.title("Flight Tracker")
    root.geometry("1160x860")
    root.minsize(980, 700)
    root.configure(bg="#eef3f8")

    style = ttk.Style(root)
    try:
        style.theme_use("clam")
    except tk.TclError:
        pass
    style.configure("Toolbar.TFrame", background="#dbe8f3")
    style.configure("Footer.TFrame", background="#eef3f8")
    style.configure("Overview.TFrame", background="#eef3f8")
    style.configure("Panel.TFrame", background="#ffffff", relief="ridge", borderwidth=1)
    style.configure("PanelTitle.TLabel", background="#ffffff", foreground="#24415c")
    style.configure("PanelValue.TLabel", background="#ffffff", foreground="#1f2933")
    style.configure("Metric.TFrame", background="#17324d", relief="flat")
    style.configure("MetricTitle.TLabel", background="#17324d", foreground="#bcd2e8")
    style.configure("MetricValue.TLabel", background="#17324d", foreground="#ffffff")
    style.configure("Route.TFrame", background="#e7f0d8", relief="ridge", borderwidth=1)
    style.configure("RouteTitle.TLabel", background="#e7f0d8", foreground="#2f4f1f")
    style.configure("RouteValue.TLabel", background="#e7f0d8", foreground="#172b11")
    style.configure("Header.TLabel", background="#eef3f8", foreground="#142638")
    style.configure("Summary.TLabel", background="#eef3f8", foreground="#334e68")
    style.configure("Status.TLabel", background="#dbe8f3", foreground="#24415c")
    style.configure("Footer.TLabel", background="#eef3f8", foreground="#52606d")

    status_text = tk.StringVar(value="Starting...")
    last_refresh_text = tk.StringVar(value="Last refreshed: never")
    units_text = tk.StringVar(value=args.units)
    current_location: Location | None = None
    current_nearest: NearestAircraft | None = None
    busy = False
    closed = False

    root.columnconfigure(0, weight=1)
    root.rowconfigure(1, weight=1)

    toolbar = ttk.Frame(root, padding=(10, 8), style="Toolbar.TFrame")
    toolbar.grid(row=0, column=0, sticky="ew")
    toolbar.columnconfigure(0, weight=1)

    status = ttk.Label(toolbar, textvariable=status_text, style="Status.TLabel")
    status.grid(row=0, column=0, sticky="w")
    ttk.Label(toolbar, text="Units", style="Status.TLabel").grid(
        row=0, column=1, padx=(8, 4)
    )
    units_selector = ttk.Combobox(
        toolbar,
        textvariable=units_text,
        values=("metric", "imperial"),
        width=9,
        state="readonly",
    )
    units_selector.grid(row=0, column=2, padx=(0, 8))
    refresh_button = ttk.Button(toolbar, text="Refresh")
    refresh_button.grid(row=0, column=3, padx=(8, 0))

    notebook = ttk.Notebook(root)
    notebook.grid(row=1, column=0, sticky="nsew", padx=10, pady=(0, 8))
    overview_tab = ttk.Frame(notebook, style="Overview.TFrame")
    raw_tab = ttk.Frame(notebook, padding=8)
    notebook.add(overview_tab, text="Overview")
    notebook.add(raw_tab, text="Raw report")

    overview_tab.columnconfigure(0, weight=1)
    overview_tab.rowconfigure(0, weight=1)
    overview_canvas = tk.Canvas(
        overview_tab,
        bg="#eef3f8",
        highlightthickness=0,
        borderwidth=0,
    )
    overview_scrollbar = ttk.Scrollbar(
        overview_tab, orient="vertical", command=overview_canvas.yview
    )
    overview_canvas.configure(yscrollcommand=overview_scrollbar.set)
    overview_canvas.grid(row=0, column=0, sticky="nsew")
    overview_scrollbar.grid(row=0, column=1, sticky="ns")

    overview = ttk.Frame(overview_canvas, padding=12, style="Overview.TFrame")
    overview_window = overview_canvas.create_window((0, 0), window=overview, anchor="nw")

    def configure_overview_scroll(_event: Any = None) -> None:
        overview_canvas.configure(scrollregion=overview_canvas.bbox("all"))

    def configure_overview_width(event: Any) -> None:
        overview_canvas.itemconfigure(overview_window, width=event.width)

    overview.bind("<Configure>", configure_overview_scroll)
    overview_canvas.bind("<Configure>", configure_overview_width)

    overview.columnconfigure(0, weight=1)
    overview.columnconfigure(1, weight=1)
    overview.rowconfigure(4, weight=1)

    callsign_text = tk.StringVar(value="Nearest aircraft")
    route_label_text = tk.StringVar(value="Operator unknown")
    route_primary_text = tk.StringVar(value="Origin and destination unknown")
    summary_text = tk.StringVar(value="Waiting for first refresh...")
    distance_text = tk.StringVar(value="--")
    bearing_text = tk.StringVar(value="--")
    altitude_text = tk.StringVar(value="--")
    speed_text = tk.StringVar(value="--")
    city_text = tk.StringVar(value="--")
    operator_text = tk.StringVar(value="--")
    route_text = tk.StringVar(value="--")
    registry_text = tk.StringVar(value="--")
    motion_text = tk.StringVar(value="--")
    timestamps_text = tk.StringVar(value="--")

    title = ttk.Label(
        overview,
        textvariable=callsign_text,
        font=("TkDefaultFont", 22, "bold"),
        style="Header.TLabel",
    )
    title.grid(row=0, column=0, columnspan=2, sticky="w")
    summary = ttk.Label(
        overview,
        textvariable=summary_text,
        wraplength=980,
        justify="left",
        font=("TkDefaultFont", 11),
        style="Summary.TLabel",
    )
    summary.grid(row=1, column=0, columnspan=2, sticky="ew", pady=(4, 14))

    route_banner = ttk.Frame(overview, padding=12, style="Route.TFrame")
    route_banner.grid(row=2, column=0, columnspan=2, sticky="ew", pady=(0, 14))
    route_banner.columnconfigure(0, weight=1)
    ttk.Label(
        route_banner,
        textvariable=route_label_text,
        font=("TkDefaultFont", 9, "bold"),
        style="RouteTitle.TLabel",
    ).grid(row=0, column=0, sticky="w")
    ttk.Label(
        route_banner,
        textvariable=route_primary_text,
        font=("TkDefaultFont", 15, "bold"),
        wraplength=960,
        justify="left",
        style="RouteValue.TLabel",
    ).grid(row=1, column=0, sticky="ew", pady=(4, 0))

    metrics = ttk.Frame(overview)
    metrics.grid(row=3, column=0, columnspan=2, sticky="new")
    for index in range(4):
        metrics.columnconfigure(index, weight=1, uniform="metric")

    metric_card(metrics, "Distance", distance_text).grid(row=0, column=0, sticky="ew", padx=(0, 8))
    metric_card(metrics, "Bearing", bearing_text).grid(row=0, column=1, sticky="ew", padx=8)
    metric_card(metrics, "Altitude", altitude_text).grid(row=0, column=2, sticky="ew", padx=8)
    metric_card(metrics, "Speed", speed_text).grid(row=0, column=3, sticky="ew", padx=(8, 0))

    panels = ttk.Frame(overview)
    panels.grid(row=4, column=0, columnspan=2, sticky="nsew", pady=(14, 0))
    panels.columnconfigure(0, weight=1)
    panels.columnconfigure(1, weight=1)
    for index in range(3):
        panels.rowconfigure(index, weight=1)

    info_panel(panels, "Location", city_text).grid(row=0, column=0, sticky="nsew", padx=(0, 8), pady=(0, 10))
    info_panel(panels, "Operator", operator_text).grid(row=0, column=1, sticky="nsew", padx=(8, 0), pady=(0, 10))
    info_panel(panels, "Route and Airline", route_text).grid(row=1, column=0, sticky="nsew", padx=(0, 8), pady=(0, 10))
    info_panel(panels, "Registry", registry_text).grid(row=1, column=1, sticky="nsew", padx=(8, 0), pady=(0, 10))
    info_panel(panels, "Motion", motion_text).grid(row=2, column=0, sticky="nsew", padx=(0, 8))
    info_panel(panels, "Timestamps", timestamps_text).grid(row=2, column=1, sticky="nsew", padx=(8, 0))

    raw_tab.columnconfigure(0, weight=1)
    raw_tab.rowconfigure(0, weight=1)
    report = tk.Text(
        raw_tab,
        wrap="word",
        font=("TkFixedFont", 10),
        padx=12,
        pady=12,
        state="disabled",
    )
    report.grid(row=0, column=0, sticky="nsew")
    scrollbar = ttk.Scrollbar(raw_tab, orient="vertical", command=report.yview)
    scrollbar.grid(row=0, column=1, sticky="ns")
    report.configure(yscrollcommand=scrollbar.set)

    footer = ttk.Frame(root, padding=(10, 0, 10, 8), style="Footer.TFrame")
    footer.grid(row=2, column=0, sticky="ew")
    footer.columnconfigure(0, weight=1)
    ttk.Label(footer, textvariable=last_refresh_text, style="Footer.TLabel").grid(row=0, column=0, sticky="w")
    ttk.Label(footer, text=f"Auto-refresh: every {args.refresh_seconds} seconds", style="Footer.TLabel").grid(
        row=0, column=1, sticky="e"
    )

    def set_report(content: str) -> None:
        report.configure(state="normal")
        report.delete("1.0", "end")
        report.insert("1.0", content)
        report.configure(state="disabled")

    def render_current() -> None:
        if current_location is None or current_nearest is None:
            return
        set_report(
            build_report(
                current_location,
                current_nearest,
                args.radius_km,
                units_text.get(),
            )
        )
        set_overview(current_location, current_nearest)

    def set_overview(location: Location, nearest: NearestAircraft) -> None:
        state = nearest.state
        classification = nearest.classification
        aircraft = (nearest.metadata or {}).get("aircraft") or {}
        flightroute = (nearest.metadata or {}).get("flightroute") or {}
        faa = nearest.faa or {}

        callsign_text.set(str(state.get("callsign") or "Aircraft with no callsign"))
        route_label_text.set(format_route_operator_label(classification, flightroute))
        route_primary_text.set(format_primary_route(flightroute))
        summary_text.set(human_summary(nearest, units_text.get()))
        distance_text.set(format_distance(nearest.distance_km, units_text.get(), multiline=True))
        bearing_text.set(f"{nearest.bearing_degrees:.0f} deg")
        altitude_text.set(format_meters_and_feet(state.get("baro_altitude")))
        speed_text.set(format_speed(state.get("velocity")))
        city_text.set(
            "\n".join(
                [
                    f"Aircraft near: {nearest.nearest_place.label if nearest.nearest_place else 'unknown'}",
                    f"You: {location.label}",
                    f"Aircraft coordinates: {format_float(state.get('latitude'), 5)}, {format_float(state.get('longitude'), 5)}",
                ]
            )
        )
        operator_lines = [
            f"Likely type: {classification.get('type', 'unknown')}",
            f"Operator: {classification.get('operator') or 'unknown'}",
            f"Country: {classification.get('country') or 'unknown'}",
            f"Source: {classification.get('confidence', 'unknown')}",
        ]
        operator_text.set("\n".join(operator_lines))
        route_text.set(format_route_panel(flightroute))
        registry_text.set(format_registry_panel(aircraft, faa))
        motion_text.set(
            "\n".join(
                [
                    f"Track: {format_degrees(state.get('true_track'))}",
                    f"Vertical rate: {format_vertical_rate(state.get('vertical_rate'))}",
                    f"On ground: {yes_no(state.get('on_ground'))}",
                    f"Squawk: {value_or_unknown(state.get('squawk'))}",
                ]
            )
        )
        timestamps_text.set(
            "\n".join(
                [
                    f"OpenSky snapshot: {format_timestamp(nearest.api_time)}",
                    f"Last position: {format_timestamp(state.get('time_position'))}",
                    f"Last contact: {format_timestamp(state.get('last_contact'))}",
                ]
            )
        )

    def set_error(content: str) -> None:
        callsign_text.set("Refresh failed")
        route_label_text.set("Operator unknown")
        route_primary_text.set("Origin and destination unknown")
        summary_text.set(content.strip())
        for variable in [
            distance_text,
            bearing_text,
            altitude_text,
            speed_text,
            city_text,
            operator_text,
            route_text,
            registry_text,
            motion_text,
            timestamps_text,
        ]:
            variable.set("--")

    def refresh() -> None:
        nonlocal busy, current_location, current_nearest
        if busy or closed:
            return

        busy = True
        refresh_button.configure(state="disabled")
        status_text.set("Refreshing nearest aircraft...")

        def worker() -> None:
            try:
                location = resolve_location(args)
                nearest = find_nearest_aircraft(location, args.radius_km, args.timeout)
                content = build_report(location, nearest, args.radius_km, units_text.get())
                error = None
            except FlightTrackerError as exc:
                content = f"error: {exc}\n"
                error = str(exc)
            except Exception as exc:
                if args.debug:
                    content = f"unexpected error: {exc}\n"
                else:
                    content = "unexpected error: run again with --debug for details\n"
                error = str(exc)

            def finish() -> None:
                nonlocal busy, current_location, current_nearest
                if closed:
                    return
                if error:
                    set_report(content)
                    set_error(content)
                else:
                    current_location = location
                    current_nearest = nearest
                    render_current()
                timestamp = datetime.now(timezone.utc).astimezone().strftime(
                    "%Y-%m-%d %H:%M:%S %Z"
                )
                last_refresh_text.set(f"Last refreshed: {timestamp}")
                status_text.set("Refresh failed" if error else "Current nearest aircraft")
                refresh_button.configure(state="normal")
                busy = False
                root.after(args.refresh_seconds * 1000, refresh)

            root.after(0, finish)

        threading.Thread(target=worker, daemon=True).start()

    def close() -> None:
        nonlocal closed
        closed = True
        root.destroy()

    refresh_button.configure(command=refresh)
    units_selector.bind("<<ComboboxSelected>>", lambda _event: render_current())
    root.protocol("WM_DELETE_WINDOW", close)
    root.after(100, refresh)
    root.mainloop()
    return 0


def metric_card(parent: Any, title: str, value: Any) -> Any:
    frame = ttk_frame(parent, padding=12, style="Metric.TFrame")
    frame.columnconfigure(0, weight=1)
    ttk_label(frame, text=title, font=("TkDefaultFont", 9, "bold"), style="MetricTitle.TLabel").grid(
        row=0, column=0, sticky="w"
    )
    ttk_label(frame, textvariable=value, font=("TkDefaultFont", 14, "bold"), style="MetricValue.TLabel").grid(
        row=1, column=0, sticky="w", pady=(6, 0)
    )
    return frame


def info_panel(parent: Any, title: str, value: Any) -> Any:
    frame = ttk_frame(parent, padding=12, style="Panel.TFrame")
    frame.columnconfigure(0, weight=1)
    ttk_label(frame, text=title, font=("TkDefaultFont", 10, "bold"), style="PanelTitle.TLabel").grid(
        row=0, column=0, sticky="w"
    )
    ttk_label(frame, textvariable=value, wraplength=455, justify="left", style="PanelValue.TLabel").grid(
        row=1, column=0, sticky="new", pady=(6, 0)
    )
    return frame


def ttk_frame(parent: Any, **kwargs: Any) -> Any:
    import tkinter.ttk as ttk

    return ttk.Frame(parent, **kwargs)


def ttk_label(parent: Any, *args: Any, **kwargs: Any) -> Any:
    import tkinter.ttk as ttk

    return ttk.Label(parent, *args, **kwargs)


def resolve_location(args: argparse.Namespace) -> Location:
    has_lat = args.lat is not None
    has_lon = args.lon is not None
    if has_lat != has_lon:
        raise FlightTrackerError("--lat and --lon must be provided together")

    if has_lat and has_lon:
        validate_lat_lon(args.lat, args.lon)
        return Location(args.lat, args.lon, "provided coordinates", "cli")

    data = get_json(IPAPI_URL, args.timeout)
    if data.get("error"):
        reason = data.get("reason") or data.get("message") or "unknown geolocation error"
        raise FlightTrackerError(f"could not determine location from IP: {reason}")

    latitude = data.get("latitude")
    longitude = data.get("longitude")
    if latitude is None or longitude is None:
        raise FlightTrackerError("IP geolocation did not return latitude and longitude")

    latitude = float(latitude)
    longitude = float(longitude)
    validate_lat_lon(latitude, longitude)

    place = ", ".join(
        str(value)
        for value in [data.get("city"), data.get("region"), data.get("country_name")]
        if value
    )
    return Location(latitude, longitude, place or "IP geolocation", "ipapi.co")


def find_nearest_aircraft(
    location: Location, radius_km: float, timeout: float
) -> NearestAircraft:
    validate_radius_km(radius_km)

    params = bounding_box(location.latitude, location.longitude, radius_km)
    params["extended"] = 1
    data = opensky_get_json("/states/all", params, timeout)
    states = data.get("states") or []

    nearest: NearestAircraft | None = None
    for raw_state in states:
        state = parse_state(raw_state)
        aircraft_lat = state.get("latitude")
        aircraft_lon = state.get("longitude")
        if aircraft_lat is None or aircraft_lon is None:
            continue

        distance_km = haversine_km(
            location.latitude, location.longitude, aircraft_lat, aircraft_lon
        )
        if distance_km > radius_km:
            continue

        candidate = NearestAircraft(
            distance_km=distance_km,
            bearing_degrees=bearing_degrees(
                location.latitude, location.longitude, aircraft_lat, aircraft_lon
            ),
            state=state,
            nearest_place=None,
            metadata=None,
            faa=None,
            classification={},
            api_time=data.get("time"),
        )
        if nearest is None or candidate.distance_km < nearest.distance_km:
            nearest = candidate

    if nearest is None:
        raise FlightTrackerError(
            f"no aircraft with current position found within {radius_km:g} km"
        )

    place = reverse_geocode(nearest.state["latitude"], nearest.state["longitude"], timeout)
    metadata = fetch_adsbdb_metadata(nearest.state, timeout)
    faa = fetch_faa_metadata(nearest.state, timeout)
    return NearestAircraft(
        distance_km=nearest.distance_km,
        bearing_degrees=nearest.bearing_degrees,
        state=nearest.state,
        nearest_place=place,
        metadata=metadata,
        faa=faa,
        classification=classify_aircraft(nearest.state, metadata, faa),
        api_time=nearest.api_time,
    )


def validate_radius_km(radius_km: float) -> None:
    if not math.isfinite(radius_km):
        raise FlightTrackerError("--radius-km must be a finite number")
    if radius_km <= 0:
        raise FlightTrackerError("--radius-km must be greater than zero")
    if radius_km > MAX_RADIUS_KM:
        raise FlightTrackerError(f"--radius-km must be no greater than {MAX_RADIUS_KM:g}")


def opensky_get_json(path: str, params: dict[str, Any], timeout: float) -> Any:
    url = f"{OPENSKY_API_URL}{path}?{urlencode(params)}"
    headers = {}

    client_id = os.getenv("OPENSKY_CLIENT_ID")
    client_secret = os.getenv("OPENSKY_CLIENT_SECRET")
    if client_id and client_secret:
        headers["Authorization"] = f"Bearer {fetch_opensky_token(client_id, client_secret, timeout)}"

    return get_json(url, timeout, headers=headers)


def fetch_opensky_token(client_id: str, client_secret: str, timeout: float) -> str:
    body = urlencode(
        {
            "grant_type": "client_credentials",
            "client_id": client_id,
            "client_secret": client_secret,
        }
    ).encode("ascii")
    request = Request(
        OPENSKY_TOKEN_URL,
        data=body,
        method="POST",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    data = request_json(request, timeout)
    token = data.get("access_token")
    if not token:
        raise FlightTrackerError("OpenSky authentication did not return an access token")
    return str(token)


def reverse_geocode(latitude: float, longitude: float, timeout: float) -> Place | None:
    params = {
        "format": "jsonv2",
        "lat": latitude,
        "lon": longitude,
        "zoom": 10,
        "addressdetails": 1,
        "accept-language": "en",
    }
    try:
        data = get_json(f"{NOMINATIM_REVERSE_URL}?{urlencode(params)}", timeout)
    except FlightTrackerError:
        return None

    address = data.get("address") or {}
    city = first_present(
        address,
        ["city", "town", "village", "municipality", "hamlet", "county"],
    )
    region = first_present(address, ["state", "region", "state_district", "county"])
    country = address.get("country")
    label = readable_place_label(city, region, country, data.get("display_name"))
    if not label:
        return None

    return Place(
        label=label,
        city=city,
        region=region,
        country=country,
        source="OpenStreetMap Nominatim",
    )


def fetch_adsbdb_metadata(state: dict[str, Any], timeout: float) -> dict[str, Any] | None:
    icao24 = state.get("icao24")
    if not icao24:
        return None

    url = f"{ADSBDB_API_URL}/aircraft/{icao24}"
    callsign = state.get("callsign")
    if callsign:
        url = f"{url}?{urlencode({'callsign': callsign})}"

    try:
        data = get_json(url, timeout)
    except FlightTrackerError:
        return None

    response = data.get("response")
    if not isinstance(response, dict):
        return None
    return response


def fetch_faa_metadata(state: dict[str, Any], timeout: float) -> dict[str, Any] | None:
    n_number = mode_s_hex_to_n_number(state.get("icao24"))
    if not n_number:
        return None

    url = f"{FAA_N_NUMBER_URL}?{urlencode({'nNumberTxt': n_number.removeprefix('N')})}"
    try:
        html = get_text(url, timeout)
    except FlightTrackerError:
        return {"n_number": n_number, "error": "FAA lookup failed"}

    fields = parse_faa_inquiry_fields(html)
    if not fields:
        return {"n_number": n_number, "error": "FAA lookup returned no parsed fields"}

    fields["n_number"] = n_number
    return fields


def classify_aircraft(
    state: dict[str, Any],
    metadata: dict[str, Any] | None = None,
    faa: dict[str, Any] | None = None,
) -> dict[str, Any]:
    callsign = state.get("callsign")
    prefix = callsign_prefix(callsign)
    origin_country = state.get("origin_country")
    flightroute = (metadata or {}).get("flightroute") or {}
    airline = flightroute.get("airline") or {}
    aircraft = (metadata or {}).get("aircraft") or {}
    faa_owner = (faa or {}).get("registered_owner_name")

    if airline.get("name"):
        return {
            "type": "commercial",
            "operator": airline["name"],
            "country": airline.get("country") or origin_country,
            "confidence": "ADSBDB callsign route airline match",
        }

    if prefix:
        airline = AIRLINE_ICAO.get(prefix)
        if airline:
            return {
                "type": "commercial",
                "operator": airline,
                "country": origin_country,
                "confidence": "callsign prefix match",
            }

        for military_prefix, (country, operator) in MILITARY_CALLSIGNS.items():
            if str(callsign).upper().startswith(military_prefix):
                return {
                    "type": "military",
                    "operator": operator,
                    "country": country,
                    "confidence": "callsign prefix match",
                }

    registered_owner = aircraft.get("registered_owner")
    if registered_owner:
        return {
            "type": "unknown",
            "operator": registered_owner,
            "country": aircraft.get("registered_owner_country_name") or origin_country,
            "confidence": "ADSBDB registered owner, not necessarily current operator",
        }

    if faa_owner:
        return {
            "type": "unknown",
            "operator": faa_owner,
            "country": "United States",
            "confidence": "FAA registered owner, not necessarily current operator",
        }

    return {
        "type": "unknown",
        "operator": None,
        "country": origin_country,
        "confidence": "No operator metadata found in OpenSky, ADSBDB, or FAA",
    }


def callsign_prefix(callsign: Any) -> str | None:
    if not isinstance(callsign, str):
        return None
    letters = []
    for character in callsign.strip().upper():
        if character.isalpha():
            letters.append(character)
        else:
            break
    if len(letters) < 3:
        return None
    return "".join(letters[:3])


def get_json(url: str, timeout: float, headers: dict[str, str] | None = None) -> Any:
    request = Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    return request_json(request, timeout)


def get_text(url: str, timeout: float, headers: dict[str, str] | None = None) -> str:
    request = Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    try:
        with urlopen(request, timeout=timeout) as response:
            charset = response.headers.get_content_charset() or "utf-8"
            return read_limited(response, MAX_TEXT_RESPONSE_BYTES, request.full_url).decode(
                charset, errors="replace"
            )
    except HTTPError as exc:
        raise FlightTrackerError(f"HTTP {exc.code} from {request.full_url}: {exc.reason}") from exc
    except URLError as exc:
        raise FlightTrackerError(f"could not reach {request.full_url}: {exc.reason}") from exc
    except TimeoutError as exc:
        raise FlightTrackerError(f"request timed out: {request.full_url}") from exc


def request_json(request: Request, timeout: float) -> Any:
    try:
        with urlopen(request, timeout=timeout) as response:
            charset = response.headers.get_content_charset() or "utf-8"
            payload = read_limited(response, MAX_JSON_RESPONSE_BYTES, request.full_url)
            return json.loads(payload.decode(charset))
    except HTTPError as exc:
        detail = exc.reason
        try:
            body = exc.read().decode("utf-8", errors="replace").strip()
            if body:
                detail = body
        except OSError:
            pass
        raise FlightTrackerError(f"HTTP {exc.code} from {request.full_url}: {detail}") from exc
    except URLError as exc:
        raise FlightTrackerError(f"could not reach {request.full_url}: {exc.reason}") from exc
    except TimeoutError as exc:
        raise FlightTrackerError(f"request timed out: {request.full_url}") from exc
    except json.JSONDecodeError as exc:
        raise FlightTrackerError(f"invalid JSON from {request.full_url}") from exc


def read_limited(response: Any, max_bytes: int, url: str) -> bytes:
    payload = response.read(max_bytes + 1)
    if len(payload) > max_bytes:
        raise FlightTrackerError(f"response from {url} exceeded {max_bytes} bytes")
    return payload


def parse_state(raw_state: list[Any]) -> dict[str, Any]:
    state = {
        field: raw_state[index] if index < len(raw_state) else None
        for index, field in enumerate(STATE_FIELDS)
    }
    callsign = state.get("callsign")
    if isinstance(callsign, str):
        state["callsign"] = callsign.strip() or None
    return state


def first_present(values: dict[str, Any], keys: list[str]) -> str | None:
    for key in keys:
        value = values.get(key)
        if value:
            return str(value)
    return None


def readable_place_label(
    city: str | None, region: str | None, country: str | None, fallback: Any
) -> str | None:
    parts = []
    for value in [city, region, country]:
        if value and value not in parts:
            parts.append(value)
    if parts:
        return ", ".join(parts)
    if fallback:
        return str(fallback)
    return None


def mode_s_hex_to_n_number(mode_s: Any) -> str | None:
    if not isinstance(mode_s, str):
        return None
    try:
        address = int(mode_s.strip(), 16)
    except ValueError:
        return None

    offset = address - US_ICAO24_START
    if offset < 0 or offset >= US_ICAO24_COUNT:
        return None

    n_number = "N" + str(1 + offset // 101711)
    offset %= 101711
    if offset <= 600:
        return n_number + n_number_suffix(offset)

    offset -= 601
    n_number += str(offset // 10111)
    offset %= 10111
    if offset <= 600:
        return n_number + n_number_suffix(offset)

    offset -= 601
    n_number += str(offset // 951)
    offset %= 951
    if offset <= 600:
        return n_number + n_number_suffix(offset)

    offset -= 601
    n_number += str(offset // 35)
    offset %= 35
    if offset <= len(N_NUMBER_LETTERS):
        return n_number + n_number_letter(offset)

    offset -= len(N_NUMBER_LETTERS) + 1
    return n_number + str(offset)


def n_number_suffix(offset: int) -> str:
    if offset == 0:
        return ""
    offset -= 1
    return N_NUMBER_LETTERS[offset // 25] + n_number_letter(offset % 25)


def n_number_letter(offset: int) -> str:
    if offset == 0:
        return ""
    return N_NUMBER_LETTERS[offset - 1]


class FaaInquiryParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.cells: list[str] = []
        self._capture_cell = False
        self._current: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() in {"td", "th"}:
            self._capture_cell = True
            self._current = []

    def handle_data(self, data: str) -> None:
        if self._capture_cell:
            stripped = " ".join(data.split())
            if stripped:
                self._current.append(stripped)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() in {"td", "th"} and self._capture_cell:
            value = " ".join(self._current).strip()
            if value:
                self.cells.append(value)
            self._capture_cell = False
            self._current = []


FAA_FIELD_LABELS = {
    "Manufacturer Name": "manufacturer",
    "Model": "model",
    "MFR Year": "manufacture_year",
    "Type Aircraft": "type_aircraft",
    "Type Engine": "type_engine",
    "Status": "status",
    "Serial Number": "serial_number",
    "Mode S Code (Base 16 / Hex)": "mode_s_hex",
    "Name": "registered_owner_name",
    "City": "registered_owner_city",
    "State": "registered_owner_state",
    "Country": "registered_owner_country",
    "Type Registration": "type_registration",
}


def parse_faa_inquiry_fields(html: str) -> dict[str, str]:
    parser = FaaInquiryParser()
    parser.feed(html)
    fields: dict[str, str] = {}
    cells = parser.cells

    for index, cell in enumerate(cells[:-1]):
        key = FAA_FIELD_LABELS.get(cell)
        if key and key not in fields:
            fields[key] = cells[index + 1]

    return fields


def bounding_box(latitude: float, longitude: float, radius_km: float) -> dict[str, float]:
    lat_delta = math.degrees(radius_km / EARTH_RADIUS_KM)
    cos_lat = math.cos(math.radians(latitude))
    lon_delta = 180.0 if abs(cos_lat) < 1e-12 else math.degrees(radius_km / EARTH_RADIUS_KM / cos_lat)

    return {
        "lamin": max(-90.0, latitude - lat_delta),
        "lomin": max(-180.0, longitude - lon_delta),
        "lamax": min(90.0, latitude + lat_delta),
        "lomax": min(180.0, longitude + lon_delta),
    }


def haversine_km(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    lat1_rad = math.radians(lat1)
    lat2_rad = math.radians(lat2)
    delta_lat = lat2_rad - lat1_rad
    delta_lon = math.radians(lon2 - lon1)

    a = (
        math.sin(delta_lat / 2) ** 2
        + math.cos(lat1_rad) * math.cos(lat2_rad) * math.sin(delta_lon / 2) ** 2
    )
    return EARTH_RADIUS_KM * 2 * math.atan2(math.sqrt(a), math.sqrt(1 - a))


def bearing_degrees(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    lat1_rad = math.radians(lat1)
    lat2_rad = math.radians(lat2)
    delta_lon = math.radians(lon2 - lon1)
    y = math.sin(delta_lon) * math.cos(lat2_rad)
    x = math.cos(lat1_rad) * math.sin(lat2_rad) - math.sin(lat1_rad) * math.cos(
        lat2_rad
    ) * math.cos(delta_lon)
    return (math.degrees(math.atan2(y, x)) + 360) % 360


def validate_lat_lon(latitude: float, longitude: float) -> None:
    if not -90 <= latitude <= 90:
        raise FlightTrackerError("latitude must be between -90 and 90")
    if not -180 <= longitude <= 180:
        raise FlightTrackerError("longitude must be between -180 and 180")


def print_report(
    location: Location, nearest: NearestAircraft, radius_km: float, units: str = "metric"
) -> None:
    print(build_report(location, nearest, radius_km, units), end="")


def build_report(
    location: Location, nearest: NearestAircraft, radius_km: float, units: str = "metric"
) -> str:
    buffer = io.StringIO()
    with redirect_stdout(buffer):
        print_report_body(location, nearest, radius_km, units)
    return buffer.getvalue()


def print_report_body(
    location: Location, nearest: NearestAircraft, radius_km: float, units: str = "metric"
) -> None:
    state = nearest.state
    classification = nearest.classification
    aircraft_metadata = (nearest.metadata or {}).get("aircraft") or {}
    flightroute = (nearest.metadata or {}).get("flightroute") or {}
    faa = nearest.faa or {}
    print("Nearest aircraft")
    print("================")
    print(f"Search location: {location.label}")
    print(f"Coordinates: {location.latitude:.5f}, {location.longitude:.5f}")
    print(f"Location source: {location.source}")
    print(f"Search radius: {format_distance(radius_km, units)}")
    if nearest.api_time:
        print(f"OpenSky snapshot: {format_timestamp(nearest.api_time)}")
    print()

    print("Distance")
    print("--------")
    print(f"Distance: {format_distance(nearest.distance_km, units)}")
    print(f"Bearing from you: {nearest.bearing_degrees:.1f} deg")
    if nearest.nearest_place:
        print(f"Nearest city/place to aircraft: {nearest.nearest_place.label}")
    else:
        print("Nearest city/place to aircraft: unknown")
    print()

    print("Plain English summary")
    print("---------------------")
    print(human_summary(nearest, units))
    print()

    print("Operator")
    print("--------")
    print(f"Likely type: {classification.get('type', 'unknown')}")
    if classification.get("operator"):
        print(f"Likely operator: {classification['operator']}")
    if classification.get("country"):
        print(f"Country: {classification['country']}")
    print(f"How determined: {classification.get('confidence', 'unknown')}")
    print()

    print("ADSBDB aircraft details")
    print("-----------------------")
    if aircraft_metadata:
        print(f"Registration: {value_or_unknown(aircraft_metadata.get('registration'))}")
        print(f"Manufacturer: {value_or_unknown(aircraft_metadata.get('manufacturer'))}")
        print(f"Aircraft type: {value_or_unknown(aircraft_metadata.get('type'))}")
        print(f"ICAO type: {value_or_unknown(aircraft_metadata.get('icao_type'))}")
        print(f"Registered owner: {value_or_unknown(aircraft_metadata.get('registered_owner'))}")
        print(
            "Owner country: "
            f"{value_or_unknown(aircraft_metadata.get('registered_owner_country_name'))}"
        )
        print(
            "Operator flag code: "
            f"{value_or_unknown(aircraft_metadata.get('registered_owner_operator_flag_code'))}"
        )
        print(f"Photo: {value_or_unknown(aircraft_metadata.get('url_photo'))}")
    else:
        print("No ADSBDB aircraft match found.")
    print()

    print("FAA aircraft registry")
    print("---------------------")
    if faa:
        print(f"N-number: {value_or_unknown(faa.get('n_number'))}")
        if faa.get("error"):
            print(f"Lookup status: {faa['error']}")
        else:
            print(f"Status: {value_or_unknown(faa.get('status'))}")
            print(f"Manufacturer: {value_or_unknown(faa.get('manufacturer'))}")
            print(f"Model: {value_or_unknown(faa.get('model'))}")
            print(f"Manufacture year: {value_or_unknown(faa.get('manufacture_year'))}")
            print(f"Serial number: {value_or_unknown(faa.get('serial_number'))}")
            print(f"Type aircraft: {value_or_unknown(faa.get('type_aircraft'))}")
            print(f"Type engine: {value_or_unknown(faa.get('type_engine'))}")
            print(f"Registered owner: {value_or_unknown(faa.get('registered_owner_name'))}")
            print(
                "Owner location: "
                f"{format_codes(faa.get('registered_owner_city'), faa.get('registered_owner_state'), faa.get('registered_owner_country'))}"
            )
    else:
        print("Not a U.S. Mode-S address or no FAA registry lookup available.")
    print()

    print("Route and airline")
    print("-----------------")
    if flightroute:
        airline = flightroute.get("airline") or {}
        origin = flightroute.get("origin") or {}
        destination = flightroute.get("destination") or {}
        print(f"Airline: {value_or_unknown(airline.get('name'))}")
        print(f"Airline ICAO/IATA: {format_codes(airline.get('icao'), airline.get('iata'))}")
        print(f"Route callsign: {value_or_unknown(flightroute.get('callsign'))}")
        print(f"Origin: {format_airport(origin)}")
        print(f"Destination: {format_airport(destination)}")
    else:
        print("No ADSBDB route match found for this callsign.")
    print()

    print("Flight and aircraft")
    print("-------------------")
    print(f"Callsign: {value_or_unknown(state.get('callsign'))}")
    print(f"ICAO24: {value_or_unknown(state.get('icao24'))}")
    print(f"Origin country: {value_or_unknown(state.get('origin_country'))}")
    print(f"Squawk: {value_or_unknown(state.get('squawk'))}")
    print(f"On ground: {yes_no(state.get('on_ground'))}")
    print(f"Special purpose indicator: {yes_no(state.get('spi'))}")
    print(f"Category: {describe_code(state.get('category'), AIRCRAFT_CATEGORY)}")
    print(f"Position source: {describe_code(state.get('position_source'), POSITION_SOURCE)}")
    print()

    print("Position and motion")
    print("-------------------")
    print(f"Latitude: {format_float(state.get('latitude'), 5)}")
    print(f"Longitude: {format_float(state.get('longitude'), 5)}")
    print(f"Barometric altitude: {format_meters_and_feet(state.get('baro_altitude'))}")
    print(f"Geometric altitude: {format_meters_and_feet(state.get('geo_altitude'))}")
    print(f"Ground speed: {format_speed(state.get('velocity'))}")
    print(f"True track: {format_degrees(state.get('true_track'))}")
    print(f"Vertical rate: {format_vertical_rate(state.get('vertical_rate'))}")
    print()

    print("Timestamps")
    print("----------")
    print(f"Last position update: {format_timestamp(state.get('time_position'))}")
    print(f"Last contact: {format_timestamp(state.get('last_contact'))}")
    print()

    print("Raw OpenSky state")
    print("-----------------")
    for field in STATE_FIELDS:
        print(f"{field}: {value_or_unknown(state.get(field))}")


def human_summary(nearest: NearestAircraft, units: str = "metric") -> str:
    state = nearest.state
    callsign = state.get("callsign") or "an aircraft with no callsign"
    classification = nearest.classification
    place = nearest.nearest_place.label if nearest.nearest_place else "an unknown place"
    altitude = format_meters_and_feet(state.get("baro_altitude"))
    speed = format_speed(state.get("velocity"))
    heading = format_degrees(state.get("true_track"))

    if classification.get("type") == "commercial" and classification.get("operator"):
        operator_text = f"likely a commercial flight operated by {classification['operator']}"
    elif classification.get("type") == "military":
        operator = classification.get("operator") or "a military operator"
        country = classification.get("country") or "an unknown country"
        operator_text = f"likely military: {operator}, {country}"
    else:
        operator_text = "not identifiable as commercial or military from the available data"

    return (
        f"{callsign} is {format_distance(nearest.distance_km, units)} away, bearing "
        f"{nearest.bearing_degrees:.0f} degrees from you. It is nearest to "
        f"{place}. It is {operator_text}. Reported altitude is {altitude}, "
        f"ground speed is {speed}, and track is {heading}."
    )


def format_airport(airport: dict[str, Any]) -> str:
    if not airport:
        return "unknown"
    code = format_codes(airport.get("icao_code"), airport.get("iata_code"))
    name = value_or_unknown(airport.get("name"))
    municipality = airport.get("municipality")
    country = airport.get("country_name")
    place = ", ".join(str(value) for value in [municipality, country] if value)
    return f"{name} ({code})" + (f", {place}" if place else "")


def format_distance(distance_km: float, units: str, multiline: bool = False) -> str:
    if units == "imperial":
        value = distance_km * 0.621371
        return f"{value:.1f}\nmi" if multiline else f"{value:.1f} mi"
    return f"{distance_km:.1f}\nkm" if multiline else f"{distance_km:.1f} km"


def format_primary_route(flightroute: dict[str, Any]) -> str:
    if not flightroute:
        return "Origin unknown -> destination unknown"

    origin = flightroute.get("origin") or {}
    destination = flightroute.get("destination") or {}
    return f"{short_airport(origin)} -> {short_airport(destination)}"


def format_route_operator_label(
    classification: dict[str, Any], flightroute: dict[str, Any]
) -> str:
    airline = (flightroute.get("airline") or {}).get("name")
    if airline:
        return airline

    operator = classification.get("operator")
    aircraft_type = classification.get("type")
    country = classification.get("country")
    if aircraft_type == "military":
        if operator and country:
            return f"Military: {operator}, {country}"
        if country:
            return f"Military: {country}"
        return "Military"
    if operator:
        return str(operator)
    return "Operator unknown"


def short_airport(airport: dict[str, Any]) -> str:
    if not airport:
        return "unknown"
    code = airport.get("iata_code") or airport.get("icao_code")
    name = airport.get("municipality") or airport.get("name")
    if code and name:
        return f"{name} ({code})"
    return str(name or code or "unknown")


def format_route_panel(flightroute: dict[str, Any]) -> str:
    if not flightroute:
        return "No ADSBDB route match found."

    airline = flightroute.get("airline") or {}
    origin = flightroute.get("origin") or {}
    destination = flightroute.get("destination") or {}
    return "\n".join(
        [
            f"Airline: {value_or_unknown(airline.get('name'))}",
            f"Codes: {format_codes(airline.get('icao'), airline.get('iata'))}",
            f"Callsign: {value_or_unknown(flightroute.get('callsign'))}",
            f"From: {format_airport(origin)}",
            f"To: {format_airport(destination)}",
        ]
    )


def format_registry_panel(aircraft: dict[str, Any], faa: dict[str, Any]) -> str:
    lines = []
    if aircraft:
        lines.extend(
            [
                f"Registration: {value_or_unknown(aircraft.get('registration'))}",
                f"Type: {value_or_unknown(aircraft.get('type'))}",
                f"Owner: {value_or_unknown(aircraft.get('registered_owner'))}",
            ]
        )

    if faa:
        lines.extend(
            [
                f"FAA N-number: {value_or_unknown(faa.get('n_number'))}",
                f"FAA status: {value_or_unknown(faa.get('status') or faa.get('error'))}",
                f"FAA owner: {value_or_unknown(faa.get('registered_owner_name'))}",
            ]
        )

    return "\n".join(lines) if lines else "No registry metadata found."


def format_codes(*codes: Any) -> str:
    present = [safe_display(code) for code in codes if code]
    return " / ".join(present) if present else "unknown"


def format_timestamp(value: Any) -> str:
    if value is None:
        return "unknown"
    return datetime.fromtimestamp(int(value), timezone.utc).isoformat()


def format_float(value: Any, decimals: int) -> str:
    if value is None:
        return "unknown"
    return f"{float(value):.{decimals}f}"


def format_meters_and_feet(value: Any) -> str:
    if value is None:
        return "unknown"
    meters = float(value)
    return f"{meters:.0f} m / {meters * M_TO_FEET:.0f} ft"


def format_speed(value: Any) -> str:
    if value is None:
        return "unknown"
    mps = float(value)
    return f"{mps:.1f} m/s / {mps * MPS_TO_KNOTS:.1f} kt"


def format_degrees(value: Any) -> str:
    if value is None:
        return "unknown"
    return f"{float(value):.1f} deg"


def format_vertical_rate(value: Any) -> str:
    if value is None:
        return "unknown"
    mps = float(value)
    return f"{mps:.1f} m/s / {mps * M_TO_FEET * 60:.0f} ft/min"


def value_or_unknown(value: Any) -> str:
    if value is None:
        return "unknown"
    display = safe_display(value)
    if not display.strip():
        return "unknown"
    return display


def safe_display(value: Any) -> str:
    return CONTROL_CHARS.sub("", str(value))


def yes_no(value: Any) -> str:
    if value is None:
        return "unknown"
    return "yes" if bool(value) else "no"


def describe_code(value: Any, descriptions: dict[int, str]) -> str:
    if value is None:
        return "unknown"
    code = int(value)
    description = descriptions.get(code)
    return f"{code} ({description})" if description else str(code)
