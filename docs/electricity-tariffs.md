# Electricity tariff editor

Open **Set tariff** in the daily statistics strip. The Univer spreadsheet is
loaded only when the editor opens. Fill the week with an off-peak price, then
paste or edit peak prices. Rows are 30-minute periods, Monday through Sunday,
in the tariff's IANA time zone (including DST). Prices are currency units per
kWh, not cents. Explicit zero and negative prices are supported; blank, text,
non-finite and formula cells are rejected. Keep day and time labels unchanged.

Save applies a validated copy to this dashboard on this device. Cancel discards
the draft. Clear local tariff removes the saved copy. JSON export/import lets
another browser or installation reuse the plan. This feature does not update
Emporia or control the inverter. Public dashboards do not expose editing.

For a flat tariff, daily grid kWh produces an explicitly approximate energy cost.
For time-of-use tariffs, the strip shows the current rate; a daily cost needs
interval consumption and is deliberately unavailable from a daily total alone.
The former hard-coded USD 0.31/kWh estimate has been removed. Taxes, fixed fees,
demand charges, seasonal schedules and consumption tiers are outside this weekly
energy-rate model. The editor does not invent any initial rate.

## Emporia import

The companion `dbus-emporia-vue/scripts/export_tariff.py` reads the existing
configured device's `locationProperties` endpoint and emits only tariff fields.
Run it on the machine that already holds the driver's configuration and tokens:

```sh
python3 scripts/export_tariff.py --config /path/to/config.json \
  --device-gid 12345 --currency USD --output emporia-tariff.json
```

Use the actual configured device ID and the currency shown in the Emporia app.
Import the resulting JSON with **Import tariff**. The exporter refuses to
replace an existing file and never includes credentials or address fields.
The output must be a JSON filename without directory components; the file is
created in the current working directory with owner-only permissions.

A nonempty `utilityRateGid` identifies a selected utility plan; the available
PyEmVue device-properties contract does not provide the plan's time-of-use
schedule. Such an import retains the plan reference and leaves price cells
blank. Copy the actual schedule from the Emporia app, review it, and save.
It never treats `usageCentPerKwHour` as a complete YOU plan. For a legacy flat
plan without a utility plan ID, cents are converted to currency/kWh explicitly.

## Implementation

`src/tariffs/` is kept identical in inverter-dashboard-vue and inverter-desktop,
which currently own separate frontend builds. Univer OSS packages are pinned to
1.0.0; no paid import/export or server plugin is required. The heavy editor is a
separate lazy chunk. JSON is the exchange format; XLSX is not part of this feature.
The browser stores a plan per origin; desktop additionally scopes storage by
portal ID (falling back to gateway or MQTT host). No cloud synchronization runs.

Validation covers rates, missing data, source metadata, separate scopes, storage
failure and local-time/DST selection. Run the existing frontend build and test
commands after changing the mirrored files.
