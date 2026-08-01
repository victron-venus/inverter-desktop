# Android APK Signing — Required GitHub Secrets

To enable automatic APK signing in the release workflow, add the following **Repository Secrets** in GitHub:

1. **Settings** → **Secrets and variables** → **Actions** → **New repository secret**

| Secret Name                 | Description                                       | Example            |
| --------------------------- | ------------------------------------------------- | ------------------ |
| `ANDROID_KEYSTORE_BASE64`   | Base64-encoded keystore file (*.keystore / *.jks) | `MIID...base64...` |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password                                 | `keystore123`      |
| `ANDROID_KEY_ALIAS`         | Key alias name                                    | `release-key`      |
| `ANDROID_KEY_PASSWORD`      | Key password (may equal keystore password)        | `key123`           |

---

## Generate Keystore & Prepare Secrets

```bash
# 1. Create keystore (run once)
keytool -genkey -v -keystore release.keystore \
  -alias release-key \
  -keyalg RSA -keysize 2048 -validity 10000

# 2. Encode to Base64 (single line, no newlines)
# macOS:
openssl base64 -in release.keystore -out release.keystore.base64

# Linux:
base64 -w 0 release.keystore > release.keystore.base64

# 3. Copy content of release.keystore.base64 → ANDROID_KEYSTORE_BASE64
# 4. Add the other three secrets with the values you chose above
```

---

## How It Works

The workflow step **Android – sign APK** uses [`rkkautsar/sign-sdk-android-action@v1`](https://github.com/rkkautsar/sign-sdk-android-action) which:

- Decodes the base64 keystore
- Runs `apksigner` + `zipalign`
- Outputs `app-universal-release-signed.apk` in the same release directory
- The upload step then picks up the signed APK automatically

> **Note:** Signing only runs for the `victron-venus` org (checks `github.repository_owner`). Forks will skip signing and upload unsigned APKs.

---

## Local Build with Signing

Use the updated `build-android-local.sh` script with `--sign` flag:

```bash
# Option 1: Set env vars directly
export ANDROID_KEYSTORE_PATH=~/release.keystore
export ANDROID_KEYSTORE_PASSWORD=keystore123
export ANDROID_KEY_ALIAS=release-key
export ANDROID_KEY_PASSWORD=key123
./build-android-local.sh --sign

# Option 2: Create .env.local (gitignored) in project root
cat > .env.local <<'EOF'
ANDROID_KEYSTORE_PATH=/Users/you/release.keystore
ANDROID_KEYSTORE_PASSWORD=keystore123
ANDROID_KEY_ALIAS=release-key
ANDROID_KEY_PASSWORD=key123
EOF
./build-android-local.sh --sign
```

The script will:

1. Build release APK via Tauri
2. Run `zipalign` (4-byte alignment)
3. Run `apksigner` with your keystore
4. Output `app-universal-release-signed.apk` to `dist/android/`

> **Tip:** The script auto-loads `.env.local` if present.
