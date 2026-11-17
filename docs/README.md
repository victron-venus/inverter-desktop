# Documentation index

These documents describe the Inverter Desktop application: its plugin host,
settings model, connection paths, hardening contracts and release evidence.

## Get started

| Document | Description |
| --- | --- |
| [Desktop plugins and the mobile core](desktop-features-and-mobile-core.md) | What ships on Android and iOS versus the generic desktop package host. |
| [Gateway connection](gateway-connection.md) | Connecting through native HTTPS inverter-gateway endpoints. |
| [Electricity tariff editor](electricity-tariffs.md) | Editing prices when inverter-control is the tariff source. |
| [Grid submeter display](grid-submeter.md) | Why the Grid heading shows only the selected backup submeter. |

## Configure plugins and settings

| Document | Description |
| --- | --- |
| [Desktop plugin packages](plugin-packages.md) | Declaring and installing packages from application configuration. |
| [Desktop plugin settings](plugin-settings.md) | Isolated settings for verified installed packages. |
| [Module configuration and portable settings](module-configuration.md) | Versioned module namespaces in the shared configuration. |
| [Desktop plugin worker protocol](plugin-worker-protocol.md) | The first worker contract for optional desktop features. |
| [Ring-MQTT camera motion](ring-mqtt.md) | Confirming the real Ring-MQTT topics instead of inventing them. |
| [Inverter controls: MQTT ownership and the optional HA adapter](mqtt-control-ownership.md) | Which component owns the seven operating flags and how aliases work. |

## Operate and verify

| Document | Description |
| --- | --- |
| [Desktop session, transport and storage contracts](desktop-hardening.md) | Session, IPC, transport and credential-storage behaviour after the audit. |
| [Desktop browser compatibility](browser-compatibility.md) | The frontend entry points and the browsers it is accepted on. |
| [Native plugin media smoke test](native-plugin-media-smoke.md) | Smoke test across the media service, route, player and native window. |
| [Privacy Policy](privacy-policy.md) | Local settings, configured services and user choices. |
| [Build and quality evidence](quality-and-build.md) | Reproducing the isolated MQTT acceptance fixtures and quality checks. |
| [Desktop plugin extraction delivery](desktop-plugin-delivery.md) | Separating reviewed source, release bytes and observed behaviour. |

## Release and maintain

| Document | Description |
| --- | --- |
| [Release build environment](RELEASE_ENVIRONMENT.md) | Exact tool versions, hosted-runner limits and recovery steps. |
| [Application release identity](release-versioning.md) | The base version files that identify a release. |
| [CI orchestration](CI-ORCHESTRATION.md) | How the quality gate and release pipeline divide validation. |
| [Google Play upload bundles](google-play.md) | Building, signing and verifying the separately keyed AAB. |
| [RustSec retirement notices and the zbus 6 migration](RUSTSEC.md) | The advisories behind the Cargo Deny failure and their resolution. |
| [Security architecture and assurance](security-assurance.md) | A reasoned account of current controls and their limits. |
| [OpenSSF Best Practices evidence](openssf-evidence.md) | Evidence index for the public assessment. |
| [Screenshot artwork](images/README.md) | Source screenshots and how the README artwork is regenerated. |
| [Data resilience tracking](archive/data-resilience-todo.md) | Archived task list for transient dashboard flicker and metric zeroing. |

Shared repository policy: [release-workflow.md](release-workflow.md), [runner-preflight.md](runner-preflight.md), [VERSIONING.md](VERSIONING.md) and [rc-fuzzing.md](rc-fuzzing.md) are generated or shared boilerplate maintained in [victron-venus/venus-os-ci-toolkit](https://github.com/victron-venus/venus-os-ci-toolkit).
