# Electricity tariff editor

Open **Set tariff** in the daily statistics strip. The Univer spreadsheet is
loaded only when the editor opens. Select a schedule, fill its week with an off-peak price, then
paste or edit peak prices. Rows are 30-minute periods, Monday through Sunday,
in the tariff's IANA time zone (including DST). Prices are currency units per
kWh, not cents. Explicit zero and negative prices are supported; blank, text,
non-finite and formula cells are rejected. Keep day and time labels unchanged.

In the dashboard editor, Save applies a validated local override to this dashboard on this device. Cancel discards
the draft. Clear local tariff removes the saved copy. JSON export/import lets
another browser or installation reuse the plan. This feature does not update
Emporia or control the inverter. Public dashboards do not expose editing.

For a flat tariff, daily grid kWh produces an explicitly approximate energy cost.
For time-of-use tariffs, the strip shows the current rate; a daily cost needs
interval consumption and is deliberately unavailable from a daily total alone.
The former hard-coded USD 0.31/kWh estimate has been removed. Taxes, fixed fees,
demand charges and consumption tiers are outside this energy-rate model. The editor does not invent any initial rate.

## Seasons and billing periods

A version 2 tariff contains a default weekly `rates` grid and a `seasons` array.
Each season has a `name`, distinct calendar `months` (1–12) and its own complete
48×7 `rates` grid. Months may not overlap. Months without an override use the
default grid. The editor opens the schedule active now; switching schedules
commits any cell being edited before showing the other grid. Import and export
retain all schedules, including those not currently visible. Use **Add season** to create a schedule, enter its name and select calendar
months. Edit its weekly prices or fill the selected week. **Remove this season**
removes only that draft override; the default schedule then covers those months.
All seasons must have nonoverlapping months before saving.

An optional `billingDay` (integer 1–31) starts the billing period in the tariff's
time zone. For example, day 17 on September 24 displays September 17–October 16.
Short months clamp days 29–31 to the last calendar day without shifting later
months. This is the period start, not the payment due date. Season prices still
switch at local midnight on the first of their months, even mid-billing-period.
The displayed period is not a calculated invoice: time-of-use totals require
interval consumption. Billing dates alone cannot reconstruct that consumption.

Legacy version 1 weekly files and stored plans remain readable and migrate to
version 2 without assuming a billing date. The storage key remains unchanged.
Old app versions reject version 2 instead of silently treating seasonal prices
as year-round prices. Update the receiving app before importing a new export.

## Installation defaults and configuration backups

The editor also consumes a tariff provisioned through inverter-control's
SetupHelper or deployment configuration. A local dashboard override takes
priority. **Use installation tariff** removes that override and resumes the
installation default, including future controller updates. Invalid installation
data is shown as an error and is never silently priced as zero.

Desktop/mobile first-run setup and **Configuration → Electricity tariff** can
store a plan in the application configuration. **Apply tariff** changes only the
draft; **Save & Continue** or Configuration **Save** persists it. This copy is
included in normal portable configuration backups. Priority is local dashboard
override, application configuration, then controller tariff. **Use controller
tariff** clears the application-level plan. Local browser overrides still need
their separate tariff export when moving devices.

See the English [installation, manual entry and deployment guide](https://github.com/victron-venus/inverter-control/blob/main/docs/electricity-tariffs.md)
for the interactive SetupHelper wizard, compact period files, `TARIFF_FILE`,
validation, persistent paths and managed desktop configuration fragments.

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
The browser stores local overrides per origin; desktop additionally scopes them by
portal ID (falling back to gateway or MQTT host). No cloud synchronization runs.

Validation covers rates, missing data, source metadata, separate scopes, storage
failure, season changes, pending-cell preservation, short-month billing boundaries
and local-time/DST selection. Run the existing frontend build and test
commands after changing the mirrored files.
