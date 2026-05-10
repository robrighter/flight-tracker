import math
import unittest
from argparse import Namespace
from contextlib import redirect_stderr
from io import BytesIO, StringIO

from flight_tracker.cli import (
    FlightTrackerError,
    bearing_degrees,
    bounding_box,
    build_parser,
    classify_aircraft,
    format_distance,
    format_primary_route,
    format_route_operator_label,
    haversine_km,
    main,
    mode_s_hex_to_n_number,
    parse_faa_inquiry_fields,
    parse_state,
    read_limited,
    readable_place_label,
    resolve_location,
    safe_display,
    validate_radius_km,
)


class FlightTrackerTests(unittest.TestCase):
    def test_haversine_known_distance(self):
        distance = haversine_km(40.7128, -74.0060, 34.0522, -118.2437)
        self.assertTrue(math.isclose(distance, 3935.75, rel_tol=0.01))

    def test_bearing_is_normalized(self):
        bearing = bearing_degrees(40.7128, -74.0060, 34.0522, -118.2437)
        self.assertGreaterEqual(bearing, 0)
        self.assertLess(bearing, 360)

    def test_bounding_box_contains_origin(self):
        box = bounding_box(40.0, -70.0, 100.0)
        self.assertLess(box["lamin"], 40.0)
        self.assertGreater(box["lamax"], 40.0)
        self.assertLess(box["lomin"], -70.0)
        self.assertGreater(box["lomax"], -70.0)

    def test_parse_state_trims_callsign(self):
        raw_state = ["abc123", " DAL123 ", "United States", None, 1, -70.0, 40.0]
        state = parse_state(raw_state)
        self.assertEqual(state["callsign"], "DAL123")
        self.assertEqual(state["icao24"], "abc123")
        self.assertIsNone(state["time_position"])

    def test_cli_coordinates_must_be_paired(self):
        with self.assertRaises(FlightTrackerError):
            resolve_location(Namespace(lat=40.0, lon=None, timeout=1.0))

    def test_radius_validation_rejects_unbounded_values(self):
        with self.assertRaises(FlightTrackerError):
            validate_radius_km(float("inf"))
        with self.assertRaises(FlightTrackerError):
            validate_radius_km(0)
        with self.assertRaises(FlightTrackerError):
            validate_radius_km(1000.1)
        validate_radius_km(1000.0)

    def test_limited_read_rejects_oversized_responses(self):
        with self.assertRaises(FlightTrackerError):
            read_limited(BytesIO(b"abcdef"), 5, "https://example.test")
        self.assertEqual(read_limited(BytesIO(b"abc"), 5, "https://example.test"), b"abc")

    def test_safe_display_removes_control_characters(self):
        self.assertEqual(safe_display("ok\x1b[31mred\x00"), "ok[31mred")

    def test_ui_flags_parse(self):
        args = build_parser().parse_args(
            ["--ui", "--refresh-seconds", "30", "--units", "imperial"]
        )
        self.assertTrue(args.ui)
        self.assertEqual(args.refresh_seconds, 30)
        self.assertEqual(args.units, "imperial")

    def test_units_default_to_imperial(self):
        args = build_parser().parse_args([])
        self.assertEqual(args.units, "imperial")

    def test_ui_and_json_are_rejected_together(self):
        with redirect_stderr(StringIO()):
            self.assertEqual(main(["--ui", "--json"]), 1)

    def test_classifies_known_airline_callsign(self):
        classification = classify_aircraft(
            {"callsign": "DAL123", "origin_country": "United States"}, None
        )
        self.assertEqual(classification["type"], "commercial")
        self.assertEqual(classification["operator"], "Delta Air Lines")

    def test_classifies_known_military_callsign(self):
        classification = classify_aircraft(
            {"callsign": "RCH456", "origin_country": "United States"}, None
        )
        self.assertEqual(classification["type"], "military")
        self.assertEqual(classification["country"], "United States")

    def test_classifies_adsbdb_airline_metadata(self):
        classification = classify_aircraft(
            {"callsign": "UNKNOWN1", "origin_country": "United States"},
            {"flightroute": {"airline": {"name": "Example Air", "country": "Canada"}}},
        )
        self.assertEqual(classification["type"], "commercial")
        self.assertEqual(classification["operator"], "Example Air")
        self.assertEqual(classification["country"], "Canada")

    def test_uses_adsbdb_registered_owner_when_airline_unknown(self):
        classification = classify_aircraft(
            {"callsign": None, "origin_country": "United States"},
            {
                "aircraft": {
                    "registered_owner": "Example Leasing LLC",
                    "registered_owner_country_name": "United States",
                }
            },
        )
        self.assertEqual(classification["type"], "unknown")
        self.assertEqual(classification["operator"], "Example Leasing LLC")
        self.assertIn("registered owner", classification["confidence"])

    def test_uses_faa_owner_when_other_metadata_unknown(self):
        classification = classify_aircraft(
            {"callsign": None, "origin_country": "United States"},
            None,
            {"registered_owner_name": "EXAMPLE OWNER INC"},
        )
        self.assertEqual(classification["type"], "unknown")
        self.assertEqual(classification["operator"], "EXAMPLE OWNER INC")
        self.assertIn("FAA registered owner", classification["confidence"])

    def test_converts_us_mode_s_hex_to_n_number(self):
        self.assertEqual(mode_s_hex_to_n_number("A0BE8F"), "N147VC")
        self.assertEqual(mode_s_hex_to_n_number("a00001"), "N1")
        self.assertIsNone(mode_s_hex_to_n_number("400000"))

    def test_parses_faa_inquiry_table_fields(self):
        html = """
        <table>
          <tr><td>Manufacturer Name</td><td>CIRRUS DESIGN CORP</td></tr>
          <tr><td>Model</td><td>SR22</td></tr>
          <tr><td>Status</td><td>Valid</td></tr>
          <tr><td>Name</td><td>0689 HOLDINGS INC TRUSTEE</td></tr>
          <tr><td>City</td><td>WILMINGTON</td></tr>
          <tr><td>State</td><td>DELAWARE</td></tr>
        </table>
        """
        fields = parse_faa_inquiry_fields(html)
        self.assertEqual(fields["manufacturer"], "CIRRUS DESIGN CORP")
        self.assertEqual(fields["model"], "SR22")
        self.assertEqual(fields["registered_owner_name"], "0689 HOLDINGS INC TRUSTEE")
        self.assertEqual(fields["registered_owner_city"], "WILMINGTON")

    def test_readable_place_label_deduplicates_parts(self):
        label = readable_place_label("Denver", "Colorado", "United States", None)
        self.assertEqual(label, "Denver, Colorado, United States")

    def test_distance_formatting_units(self):
        self.assertEqual(format_distance(10.0, "metric"), "10.0 km")
        self.assertEqual(format_distance(10.0, "imperial"), "6.2 mi")

    def test_primary_route_formatting(self):
        route = {
            "origin": {"municipality": "New York", "iata_code": "JFK"},
            "destination": {"municipality": "Los Angeles", "iata_code": "LAX"},
        }
        self.assertEqual(format_primary_route(route), "New York (JFK) -> Los Angeles (LAX)")

    def test_route_operator_label_prefers_airline(self):
        label = format_route_operator_label(
            {"type": "commercial", "operator": "Fallback Air"},
            {"airline": {"name": "Example Airlines"}},
        )
        self.assertEqual(label, "Example Airlines")

    def test_route_operator_label_handles_military(self):
        label = format_route_operator_label(
            {
                "type": "military",
                "operator": "United States Air Force",
                "country": "United States",
            },
            {},
        )
        self.assertEqual(label, "Military: United States Air Force, United States")


if __name__ == "__main__":
    unittest.main()
