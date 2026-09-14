# Privacy Policy — Inverter Desktop

Last updated: 2026-09-12

Published by **alvit**. For privacy or support questions, contact [alvit.work@gmail.com](mailto:alvit.work@gmail.com).

## About this app

Inverter Desktop displays and controls information from compatible energy and home-automation systems that you configure. It connects to an MQTT broker, an Inverter Gateway, and optional Home Assistant services. The app does not provide an inverter, broker or Home Assistant account.

Project source and support: [Inverter Desktop](https://github.com/victron-venus/inverter-desktop), [support issues](https://github.com/victron-venus/inverter-desktop/issues).

Public Android builds do not use developer-operated servers by default. Connections described below use the services you configure.

## Information used by the app

**Connection settings and authentication.** You can enter server addresses, ports, usernames, passwords and other authentication settings needed to connect to your services. The app saves configuration on your device and uses the applicable credentials when contacting the selected service.

**Energy and home information.** The app receives inverter, solar, battery, grid and consumption measurements, together with the device states needed for enabled integrations. Depending on your configuration, these can include charging, pump, valve, Home Assistant entity and camera information. It displays this information and sends control requests when you operate supported controls.

**Camera information.** When you enable a compatible Home Assistant camera integration, the app accesses the configured camera URLs or media through the associated services. Accessing camera content does not mean the app is using your phone's camera.

**Diagnostic information.** The app maintains diagnostic messages in a limited in-memory frontend buffer and uses native application logging. Messages can describe connection failures, service addresses and technical errors. If you choose to share logs or screenshots for support, review them first and remove credentials, private addresses, camera images and other information you do not want to disclose. Public GitHub issues are publicly visible.

## Network destinations and security

Connections go to the MQTT broker, gateway, Home Assistant and media endpoints selected by your configuration. Those operators receive information needed to process requests, including connection metadata and any credentials or commands required by their service. Their handling of information is governed by their own policies and your arrangements with them.

Encryption depends on the transport and endpoints used. The app does not guarantee that every configured MQTT, HTTP or media connection is encrypted. Use trusted networks and services, avoid exposing unencrypted connections to untrusted networks, and enable secure transport where supported. Do not provide credentials to an endpoint you do not trust.

If you open a project or support link, your browser contacts that website, whose privacy practices apply to that visit.

## Local information and your choices

You can change connection settings and disable integrations in the app. Removing inverter connection settings stops those connections after saving; separately configured Home Assistant or camera services must also be disabled if you want to stop them.

You can remove local application information through your operating system's app-data controls. Changing a setting, clearing app data or uninstalling the app does not delete information already held by a broker, gateway, Home Assistant server, media service or support website. Contact the applicable service operator about its records and retention practices. Platform backups, if enabled, are governed by your platform settings.

## Local retention

Saved settings remain on your device until you change or remove them or clear the app's local data. Temporary display state and diagnostic buffers can be replaced as the app runs. Native diagnostic files, exported files and operating-system backups may remain separately; manage those through the app or operating system and remove any copies you shared when no longer needed. This policy does not set retention periods for services operated by others.

## Accounts and third-party services

The app uses credentials for services that you configure. Those service accounts remain with their operators. Account closure or deletion requests for a third-party service must be directed to that operator. Do not include account passwords or private access tokens in a support issue.
