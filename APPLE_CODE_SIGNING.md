# macOS Code Signing and Notarization

## Overview

macOS requires apps distributed outside the App Store to be signed and notarized to avoid Gatekeeper warnings. Without proper signing, Apple Silicon (ARM64) builds may fail to launch with "app is damaged" error. This guide explains how to set up code signing for this project.

**Note:** You must **fork this repository** to add your own Apple Developer credentials. Do not commit real secrets to the main repo.

---

## Prerequisites

- Active **Apple Developer Program** membership ($99/year)
- Access to **Certificates, Identifiers & Profiles** in Apple Developer portal
- `xcode-select` installed (`xcode-select --install`)
- macOS runner with Xcode command-line tools

---

## Step 1: Create a Developer ID Certificate

1. Log in to [Apple Developer Portal](https://developer.apple.com/account/)
2. Navigate to **Certificates, Identifiers & Profiles** → **Certificates**
3. Click **+** → Select **Developer ID Application** → Continue
4. Follow instructions to generate a CSR using **Keychain Access**
5. Upload CSR → Download the generated certificate (`Developer ID Application.cer`)
6. Double-click the `.cer` file to install it in Keychain
7. In Keychain Access, find the certificate under **My Certificates**
8. Right-click → **Export** → choose `.p12` format
9. Set a password for the `.p12` file (remember it)

---

## Step 2: Prepare GitHub Secrets

In your **forked** repository:

1. Convert the `.p12` file to base64:

```bash
base64 -i DeveloperIDApplication.p12 -o developer_id.p12.base64
```

2. Copy the contents of `developer_id.p12.base64` to clipboard.

3. Go to your fork's **Settings** → **Secrets and variables** → **Actions**
4. Add these repository secrets:

| Name                      | Value                                                                 |
|---------------------------|-----------------------------------------------------------------------|
| `APPLE_CERTIFICATE`       | (paste base64 string from step above)                                |
| `APPLE_CERTIFICATE_PASSWORD` | (password you set when exporting .p12)                              |
| `APPLE_SIGNING_IDENTITY`  | (full name of certificate, e.g., `Developer ID Application: My Name (TEAMID)`) |
| `APPLE_TEAM_ID`           | (your Apple Developer Team ID, e.g., `XXXXXXXXXX`)                  |

**Find `APPLE_SIGNING_IDENTITY`:**

```bash
security find-identity -v -p codesigning
# Output example: 1) 1234567890ABCDEF1234567890ABCDEF12345678 "Developer ID Application: My Name (TEAMID)"
```

Use the entire string after the number and space.

---

## Step 3: Enable Signing in GitHub Workflow

Edit `.github/workflows/release.yml` in your fork:

Locate the `Build app` step and add environment variables:

```yaml
- name: Build app
  uses: tauri-apps/tauri-action@84b9d35b5fc46c1e45415bdb6144030364f7ebc5
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    TAURI_APP_VERSION: ${{ needs.check-version-change.outputs.new_version }}
    APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
    APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
    APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
    APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
  with:
    release: true
    tagName: v${{ needs.check-version-change.outputs.new_version }}
    releaseName: Release v${{ needs.check-version-change.outputs.new_version }}
    args: ${{ matrix.args }}
```

Commit and push changes.

---

## Step 4 (Optional): Automatic Notarization

To avoid Gatekeeper warnings, notarize the app. Add additional secrets:

- `APPLE_KEYCHAIN_PASSWORD` – lock screen password for the runner's keychain (your macOS password)
- `APPLE_NOTARIZE` – set to `true`

Then update the `Build app` step to include notarization:

```yaml
with:
  release: true
  notarize: true
```

`tauri-action` will submit the app to Apple for notarization and staple the ticket.

**Important:** Notarization requires that the app is signed with a valid Developer ID and that the `APPLE_TEAM_ID` is correct.

---

## Step 5: Verify Build Artifacts

After push creates a new release:

1. Download the `aarch64.dmg` and `x64.dmg` from GitHub Releases
2. Mount each DMG and inspect signature:

```bash
codesign -dv --verbose=4 /Volumes/Inverter\ Dashboard/Inverter\ Dashboard.app
spctl -a -t exec -vv /Volumes/Inverter\ Dashboard/Inverter\ Dashboard.app
```

Expected output includes `source=Developer ID Application` and `accepted`.

3. If notarized, also check:

```bash
xcrun stapler validate /Volumes/Inverter\ Dashboard/Inverter\ Dashboard.app
```

---

## Troubleshooting

| Symptom                                      | Likely Cause                               |
|-----------------------------------------------|--------------------------------------------|
| "app is damaged" on launch (ARM64 only)      | App unsigned → Gatekeeper blocks           |
| `codesign` reports "invalid identity"        | Wrong `APPLE_SIGNING_IDENTITY` or expired certificate |
| Notarization fails with "invalid bundle"    | Missing required entitlements or corrupted bundle |
| Build fails with `error: failed to run custom build command` | Missing Xcode command-line tools on runner |

Check GitHub Actions logs for detailed error messages.

---

## Security Notes

- Never commit `.p12` files or secrets to version control.
- Keep your Apple Developer certificate secure; rotate if exposed.
- Use fine-grained repository permissions for forks.
- Notarization helps users avoid security warnings and is recommended for public distribution.

---

## Further Reading

- [Apple - Notarize your software](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [Tauri - Code Signing](https://tauri.app/v1/guides/distribution/signing)
- [GitHub - tauri-action](https://github.com/tauri-apps/tauri-action)
