# Inverter Desktop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub stars](https://img.shields.io/github/stars/victron-venus/inverter-desktop)](https://github.com/victron-venus/inverter-desktop/stargazers)
[![GitHub forks](https://img.shields.io/github/forks/victron-venus/inverter-desktop)](https://github.com/victron-venus/inverter-desktop/network/members)
[![GitHub last commit](https://img.shields.io/github/last-commit/victron-venus/inverter-desktop)](https://github.com/victron-venus/inverter-desktop/commits/main)
[![Maintenance](https://img.shields.io/badge/Maintained%3F-yes-green.svg)](https://github.com/victron-venus/inverter-desktop/graphs/commit-activity)
[![CI](https://github.com/victron-venus/inverter-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/victron-venus/inverter-desktop/actions/workflows/ci.yml)
[![CodeQL](https://github.com/victron-venus/inverter-desktop/actions/workflows/codeql.yml/badge.svg)](https://github.com/victron-venus/inverter-desktop/actions/workflows/codeql.yml)
[![Trivy](https://github.com/victron-venus/inverter-desktop/actions/workflows/trivy-fs.yml/badge.svg)](https://github.com/victron-venus/inverter-desktop/actions/workflows/trivy-fs.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/victron-venus/inverter-desktop/badge?svg)](https://scorecard.dev/viewer/?github.com/victron-venus/inverter-desktop)
[![OSS-Fuzz](https://img.shields.io/badge/OSS--Fuzz-integrated-success)](https://oss-fuzz.com/testcases?project=inverter-desktop)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0%2B-blue.svg)](https://tauri.app/)

Desktop application for monitoring Victron inverter systems via MQTT. Built with Tauri + TypeScript.

For the **web dashboard** (Python/FastAPI), see **[inverter-dashboard](https://github.com/victron-venus/inverter-dashboard)** — Docker image **`alvit/inverter-dashboard`**.

## Features

- Real-time power monitoring (Grid, Solar, Battery, Consumption)
- Interactive controls via MQTT
- Live power charts with uPlot
- EV charging status
- Water system monitoring
- Home automation controls
- Native desktop application (Windows, macOS, Linux)

## Quick Start

### Prerequisites

- Node.js 18+
- Rust toolchain
- MQTT broker

### Security

This project includes comprehensive security measures:

- **Security Policy**: See [SECURITY.md](SECURITY.md) for vulnerability reporting
- **Fuzzing**: Automated fuzz testing via [FUZZING.md](FUZZING.md)
- **OSS-Fuzz Integration**: Continuous fuzzing with [OSS_FUZZ_GUIDE.md](OSS_FUZZ_GUIDE.md)
- **OpenSSF Best Practices**: Badge effort documented in [OPENSSF_BADGE_GUIDE.md](OPENSSF_BADGE_GUIDE.md)
- **Dependency Auditing**: Regular security scans with `cargo audit`
- **Status Tracking**: Current security status in [SECURITY_STATUS.md](SECURITY_STATUS.md)

### Security Features

- ✅ **OSS-Fuzz Integration**: Continuous automated fuzzing
- ✅ **3 Fuzz Targets**: JSON parsing, MQTT handling, command parsing
- ✅ **ClusterFuzzLite**: CI/CD fuzzing integration
- ✅ **OpenSSF Best Practices**: Working toward badge certification
- ✅ **Regular Dependency Updates**: Automated vulnerability monitoring
- ✅ **Secure MQTT**: Connection handling and input validation
- ✅ **Security Policy**: Coordinated vulnerability disclosure

### Monitoring

- **OSS-Fuzz Dashboard**: https://oss-fuzz.com/testcases?project=inverter-desktop
- **Coverage Analysis**: https://introspector.oss-fuzz.com/?project=inverter-desktop
- **OpenSSF Best Practices**: https://www.bestpractices.dev/
- **Automated Reports**: Weekly security monitoring via GitHub Actions

## Development

```bash
# Install dependencies
npm install

# Run dev server
npm run tauri dev

# Build for production
npm run tauri build
```

### Configuration

Edit `src-tauri/capabilities/default.json` and `src/config.ts` for MQTT settings:

```typescript
const config = {
  mqttHost: "192.168.1.100",
  mqttPort: 1883,
  // ... other settings
};
```

## MQTT Topics

### Subscribed (incoming data)
- `inverter/state` - JSON with current system state
- `inverter/console` - Console log messages

### Published (commands)
- `inverter/cmd/toggle` - Toggle boolean entities
- `inverter/cmd/press` - Press button entities
- `inverter/cmd/setpoint` - Set power setpoint
- `inverter/cmd/dry_run` - Toggle dry run mode
- `inverter/cmd/limits` - Set power limits
- `inverter/cmd/ess_mode` - Toggle ESS mode
- `inverter/cmd/loop_interval` - Set control loop interval

## Expected State Format

```json
{
  "gt": 150,
  "g1": 100,
  "g2": 50,
  "tt": 2500,
  "t1": 1500,
  "t2": 1000,
  "solar_total": 3500,
  "battery_soc": 85,
  "battery_power": -500,
  "battery_voltage": 52.4,
  "setpoint": 0,
  "inverter_state": "Inverting",
  "dry_run": false,
  "ess_mode": {
    "mode_name": "Optimized (with BatteryLife)",
    "is_external": false
  },
  "booleans": {
    "auto_mode": true,
    "ev_boost": false
  },
  "daily_stats": {
    "produced_today": 25.5,
    "produced_dollars": 7.65,
    "grid_kwh": 2.3
  }
}
```

## Related Projects

This project is part of a Victron Venus OS integration suite:

| Project | Description |
|---------|-------------|
| [inverter-control](https://github.com/victron-venus/inverter-control) | ESS external control with web dashboard |
| [inverter-dashboard](https://github.com/victron-venus/inverter-dashboard) | Python/FastAPI dashboard via MQTT (`alvit/inverter-dashboard` on Docker Hub) |
| **inverter-desktop** (this) | Tauri desktop application |
| [inverter-dashboard-go](https://github.com/victron-venus/inverter-dashboard-go) | Go rewrite: same UI and MQTT workflow; binaries + Docker image `alvit/inverter-dashboard-go` |
| [dbus-mqtt-battery](https://github.com/victron-venus/dbus-mqtt-battery) | MQTT to D-Bus bridge for BMS integration |
| [dbus-tasmota-pv](https://github.com/victron-venus/dbus-tasmota-pv) | Tasmota smart plug as PV inverter on D-Bus |
| [esphome-jbd-bms-mqtt](https://github.com/victron-venus/esphome-jbd-bms-mqtt) | ESP32 Bluetooth monitor for JBD BMS |
| [inverter-monitoring](https://github.com/victron-venus/inverter-monitoring) | Telegraf + InfluxDB + Grafana monitoring stack |

## Author

Created by [@4alvit](https://github.com/4alvit)

## License

MIT License - see [LICENSE](LICENSE).

---

**Note:** This is a community project and is not affiliated with Victron Energy.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature-name`)
3. Commit your changes
4. Push to the branch (`git push origin feature-name`)
5. Create a Pull Request

## Support

For issues specific to:
- **MQTT connectivity**: Check broker reachability and topic subscriptions
- **Desktop app crashes**: Review Tauri logs and system requirements
- **This project**: Open an issue in this repository
