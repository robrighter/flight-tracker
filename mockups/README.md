# Flight Tracker UI Mockups

These concepts explored a polished desktop instrument with a nod to early commercial aviation, airport operations rooms, analog avionics, route maps, brass, enamel, and flight-strip boards. The `aviation-ui-ops-board.png` direction was selected and is what the native Rust dashboard (`--ui`) now implements.

## Directions

- `aviation-ui-art-deco-radar.png`: Art Deco aircraft dossier plus a large circular radar/map instrument. Premium single-aircraft focus screen; not implemented.
- `aviation-ui-ops-board.png`: Mid-century operations board with a nearest-aircraft list, tabs, units toggle, radar scope, and compact detail panels. **This is the implemented dashboard.**
- `aviation-ui-route-map-cockpit.png`: Vintage route-map cockpit with a broad map band, side gauges, and logbook table. Direction for a possible future surrounding-traffic exploration view; not implemented.

## Implementation Notes

- Keep the app as an operational tool first: dense, readable panels; obvious refresh and units controls; no landing-page hero.
- Use the existing app sections as the information model: summary, route, registry, operator, motion, timestamps, raw report.
- Borrow visual language from instrument faces and flight strips, but keep data text modern enough to scan quickly.
- Prefer warm ivory, brass, deep green, navy ink, muted sky blue, and restrained oxblood/coral accents.
