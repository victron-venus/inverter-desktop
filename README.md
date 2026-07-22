# Inverter Dashboard

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub stars](https://img.shields.io/github/stars/victron-venus/inverter-desktop)](https://github.com/victron-venus/inverter-desktop/stargazers)
[![GitHub forks](https://img.shields.io/github/forks/victron-venus/inverter-desktop/network/members)](https://github.com/victron-venus/inverter-desktop/network/members)
[![GitHub last commit](https://img.shields.io/github/last-commit/victron-venus/inverter-desktop)](https://github.com/victron-venus/inverter-desktop/commits/main)
[![Maintenance](https://img.shields.io/badge/Maintained%3F-yes-green.svg)](https://github.com/victron-venus/inverter-desktop/graphs/commit-activity)
[![CI](https://github.com/victron-venus/inverter-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/victron-venus/inverter-desktop/actions/workflows/ci.yml)
[![CodeQL](https://github.com/victron-venus/inverter-desktop/actions/workflows/codeql.yml/badge.svg)](https://github.com/victron-venus/inverter-desktop/actions/workflows/codeql.yml)
[![OSS-Fuzz](https://img.shields.io/badge/OSS--Fuzz-integrated-success)](https://oss-fuzz.com/testcases?project=inverter-desktop)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0%2B-blue.svg)](https://tauri.app/)

Desktop and mobile application for monitoring Victron inverter systems via MQTT. Built with Tauri + TypeScript.

| Surface | Recommended project |
|---------|---------------------|
| Cerbo GX (web) | [inverter-dashboard-go](https://github.com/victron-venus/inverter-dashboard-go) |
| Docker / NAS | [inverter-dashboard](https://github.com/victron-venus/inverter-dashboard) (`alvit/inverter-dashboard`) |
| Desktop / mobile (this app) | **inverter-desktop** |

## Features

- Real-time power monitoring (Grid, Solar, Battery, Consumption)
- Interactive controls via MQTT
- Live power charts with ECharts
- EV charging status
- Water system monitoring
- Home automation controls
- Native application for all major platforms

## Supported Platforms

| Platform    | Installation                   | Notes                                         |
| ----------- | ------------------------------ | --------------------------------------------- |
| **macOS**   | `.dmg` installer from Releases | Universal binary (Apple Silicon + Intel)      |
| **Windows** | `.msi` or `.exe` installer     | x64                                           |
| **Linux**   | `.AppImage`, `.deb`, `.rpm`    | Various distributions                         |
| **iOS**     | AltStore, TestFlight, Xcode    | Requires Apple Developer account for AltStore |
| **Android** | APK (direct install or ADB)    | arm64-v8a, armeabi-v7a, x86_64                |

---

## Installation

### macOS

1. Download `.dmg` from [Releases](https://github.com/victron-venus/inverter-desktop/releases/latest)
2. Open the `.dmg` file
3. Drag **Inverter Dashboard.app** to Applications
4. On first run: Right-click → Open → Open (bypasses Gatekeeper)

### Windows

1. Download `.msi` or `.exe` installer from [Releases](https://github.com/victron-venus/inverter-desktop/releases/latest)
2. Run installer, follow prompts
3. Launch from Start Menu or Desktop shortcut

### Linux

**AppImage (recommended):**

```bash
chmod +x Inverter-Dashboard_*.AppImage
./Inverter-Dashboard_*.AppImage
```

** Debian/Ubuntu (.deb):**

```bash
sudo dpkg -i inverter-dashboard_*.deb
sudo apt-get install -f  # install dependencies if needed
```

** RHEL/Fedora (.rpm):**

```bash
sudo rpm -i inverter-dashboard_*.rpm
```

---

### iOS Installation

iOS requires sideloading since the app is not on the App Store. Two options:

#### Option 1: AltStore (Recommended for personal use)

AltStore allows sideloading apps with a free Apple ID (no paid developer account needed).

**Prerequisites:**

- iPhone/iPad running iOS 14 or later
- A free [Apple ID](https://appleid.apple.com/) account
- AltServer installed on your Mac or PC

**Step 1: Install AltServer**

1. Download AltServer for your platform:
   - **macOS**: Download from [AltStore.io](https://altstore.io/) or via Homebrew:
     ```bash
     brew install --cask altstore
     ```
   - **Windows**: Download from [AltStore.io](https://altstore.io/)

2. Start AltServer (it runs in the menu bar/system tray)

**Step 2: Install AltStore on your device**

1. Open AltServer on your Mac/PC
2. Connect your iPhone/iPad via USB
3. On iOS: Go to Settings → General → Device Management → tap your Apple ID
4. Trust the profile if prompted

**Step 3: Sideload the app**

1. Download the `.ipa` file from [Releases](https://github.com/victron-venus/inverter-desktop/releases/latest)
2. Double-click the `.ipa` to open it in AltStore
3. Select your connected device
4. Wait for installation to complete

**Refresh requirement:** AltStore apps expire after 7 days. Keep AltServer running to auto-refresh, or:

- Right-click AltStore icon → Refresh apps
- Or right-click the app in AltStore → Refresh

**Step 4: Trust the app**

1. On iOS: Settings → General → VPN & Device Management
2. Find "Inverter Dashboard" under your Apple ID
3. Tap Trust → Confirm

#### Option 2: TestFlight (If available)

If a TestFlight beta is available:

1. Accept the TestFlight invite
2. Install TestFlight from App Store
3. Open the beta link and tap "Install"

#### Option 3: Xcode (For developers)

1. Download `.ipa` from [Releases](https://github.com/victron-venus/inverter-desktop/releases/latest)
2. Connect your device via USB
3. Open Xcode → Window → Devices and Simulators
4. Select your device → Click "+" → Select the `.ipa`
5. On first install, enable "Trust this app" in device settings

**Troubleshooting iOS:**

- App won't open: Settings → General → Device Management → Trust the app
- AltStore offline: Ensure AltServer is running and device connected
- Refresh failed: Check internet connection, try again

---

### Android Installation

#### Option 1: Direct Install (APK)

1. Download the `.apk` from [Releases](https://github.com/victron-venus/inverter-desktop/releases/latest)
2. Transfer to your Android device
3. Open the APK file
4. If prompted about unknown sources: Settings → Security → Allow unknown sources
5. Tap Install

**Note:** You may need to enable "Install unknown apps" for your browser or file manager.

#### Option 2: ADB Installation (Recommended for developers)

ADB gives you more control and is useful for debugging.

**Prerequisites:**

```bash
# Install ADB (macOS)
brew install android-platform-tools

# Install ADB (Ubuntu/Debian)
sudo apt install adb

# Install ADB (Windows) - download from:
# https://developer.android.com/studio/releases/platform-tools
```

**Step 1: Enable USB Debugging**

1. Go to Settings → About Phone
2. Tap "Build Number" 7 times → Developer mode enabled
3. Go back to Settings → Developer Options
4. Enable "USB Debugging"
5. Connect your device via USB

**Step 2: Verify connection**

```bash
adb devices
# Should show: "xxxxxxxx    device"
```

If you see "unauthorized", check your phone for a pairing confirmation dialog.

**Step 3: Install APK**

```bash
# Download the APK first
wget https://github.com/victron-venus/inverter-desktop/releases/latest/download/inverter-dashboard-android.apk

# Install
adb install inverter-dashboard-android.apk
```

**Step 4: Launch**

```bash
# Option A: From command line
adb shell am start -n com.alvit.inverter_dashboard/.MainActivity

# Option B: Tap the app icon on your device
```

**Useful ADB Commands**

```bash
# View logs (for debugging)
adb logcat -s "Inverter Dashboard"

# Reinstall (keeps app data)
adb install -r inverter-dashboard-android.apk

# Uninstall
adb uninstall com.alvit.inverter_dashboard
```

#### Option 3: Via Local Network (Wireless ADB)

```bash
# Connect via USB first, then enable wireless
adb tcpip 5555

# Disconnect USB, find device IP on phone
# Settings → About Phone → Status → IP address

# Connect wirelessly
adb connect <device-ip>:5555

# Install
adb install inverter-dashboard-android.apk
```

---

## Configuration

Edit `src-tauri/capabilities/default.json` and `src/config.ts` for MQTT settings:

```typescript
const config = {
  mqttHost: '192.168.1.100',
  mqttPort: 1883,
  // ... other settings
}
```

---

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

---

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

---

## Security

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
- ✅ **OpenSSF Best Practices**: Working toward badge certification
- ✅ **Regular Dependency Updates**: Automated vulnerability monitoring
- ✅ **Secure MQTT**: Connection handling and input validation
- ✅ **Security Policy**: Coordinated vulnerability disclosure

### Monitoring

- **OSS-Fuzz Dashboard**: https://oss-fuzz.com/testcases?project=inverter-desktop
- **Coverage Analysis**: https://introspector.oss-fuzz.com/?project=inverter-desktop
- **OpenSSF Best Practices**: https://www.bestpractices.dev/
- **Automated Reports**: Weekly security monitoring via GitHub Actions

---

## Development

```bash
# Install dependencies
npm install

# Run dev server
npm run tauri dev

# Build for production
npm run tauri build
```

### Building for Mobile

**Android:**

```bash
./build-android-local.sh        # Release build
./build-android-local.sh --dev  # Debug build
```

**iOS:**

```bash
./build-ios-local.sh            # Unsigned build (for AltStore)
./build-ios-local.sh --sign     # Signed build (requires Apple Developer)
```

---

## Related Projects

This project is part of the Victron Venus OS integration suite:

| Project                                                                         | Description                                                              |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| [inverter-control](https://github.com/victron-venus/inverter-control)           | Advanced ESS external control system with grid-zero targeting            |
| [inverter-dashboard](https://github.com/victron-venus/inverter-dashboard)       | Real-time web dashboard (Python/FastAPI) via MQTT                        |
| [inverter-dashboard-go](https://github.com/victron-venus/inverter-dashboard-go) | High-performance Go rewrite of the web dashboard                         |
| **inverter-desktop** (this)                                                     | Native desktop and mobile application (Rust/Tauri) for system monitoring |
| [dbus-mqtt-battery](https://github.com/victron-venus/dbus-mqtt-battery)         | MQTT to D-Bus bridge for JBD BMS battery integration                     |
| [dbus-tasmota-pv](https://github.com/victron-venus/dbus-tasmota-pv)             | Tasmota smart plug integration as a PV inverter on D-Bus                 |
| [esphome-jbd-bms-mqtt](https://github.com/victron-venus/esphome-jbd-bms-mqtt)   | ESP32 Bluetooth monitor for JBD BMS batteries                            |
| [inverter-monitoring](https://github.com/victron-venus/inverter-monitoring)     | TIG (Telegraf, InfluxDB, Grafana) monitoring stack                       |
| [terraform-github-victron](https://github.com/4alvit/terraform-github-victron)  | Infrastructure as Code for the GitHub organization                       |

---

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
- **iOS installation**: Check AltStore/AltServer status and device trust settings
- **Android installation**: Verify ADB connection and USB debugging enabled
- **This project**: Open an issue in this repository
