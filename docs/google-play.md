# Google Play upload bundles

The **Google Play upload bundle** workflow builds an AAB from `main`, signs it with a separate upload key, verifies every payload signature against that key's certificate, and retains the AAB plus its SHA-256 checksum as a GitHub Actions artifact for seven days. It does not upload to Google Play or create a GitHub release. A missing or invalid signing configuration fails the job; the workflow never publishes an unsigned fallback.

Run the workflow manually on `main` after configuring these repository secrets:

- `PLAY_UPLOAD_KEYSTORE_BASE64`: the upload keystore encoded as one base64 line.
- `PLAY_UPLOAD_STORE_PASSWORD`: the keystore password.
- `PLAY_UPLOAD_KEY_ALIAS`: the exact upload key alias.
- `PLAY_UPLOAD_KEY_PASSWORD`: the private key password.

Create and back up this upload key through your own secure key-management process. Never commit the keystore, passwords, or base64 value. The workflow decodes the key into a private temporary directory only during signing, removes it afterward, and uploads only the verified AAB and checksum. Register the upload certificate in Play Console before submitting its first bundle. No service-account credentials are required because submission remains manual.

## Preserve existing installations

The existing release APK signing workflow and its `ANDROID_KEYSTORE_*` / `ANDROID_KEY_*` secrets are unchanged. **Do not replace that app signing key with the Play upload key.** An upload key authenticates submissions; Play App Signing signs the APKs delivered to users with a separate app signing key.

For upgrades between existing GitHub APK installations and Google Play, enroll using the **existing app signing key** rather than allowing Play to generate an unrelated key. Follow Play Console's official encrypted key-import procedure, using the original keystore under your control. A certificate alone cannot recreate its private key. This repository does not contain a documented location for the original key; `.gitignore` excludes `release.keystore` but does not prove where it is stored.

The public `Inverter.Desktop_2.5.40_signed.apk` from [release v2.5.40](https://github.com/victron-venus/inverter-desktop/releases/tag/v2.5.40) was verified with Android `apksigner`. Its signer certificate fingerprints are:

- SHA-256: `4b027a0ef2158d4264cd095a9b5bc05078c8e42dc7944373d3a31ed1bc9be047`
- SHA-1: `6648bbdd0b6fd122e38a49f5da6f4031dff37b75`

These identify the actual published signer, not a verified company affiliation. Check that the app signing certificate selected in Play Console matches the existing signer before publishing. The new upload certificate is expected to be different.

## Package and compatibility checks

The existing application ID is `com.alvit.inverter_dashboard`, with target/compile SDK 36. Tauri supplies release versions from the application metadata; the published 2.5.40 AAB has version code `2005040`. Before each Play upload, increase the application version through the normal release process and check the generated version code against all previously uploaded Play bundles. Rebuilding the same version does not produce a new version code, and the workflow cannot inspect Play's upload history.

The Play workflow uses AGP 8.11.0 and explicitly selects NDK r28. It verifies 16 KB ELF load-segment alignment for every packaged arm64/x86_64 library before signing. The current 2.5.40 AAB passes that check. This static check does not replace running the application on a 16 KB Android device/emulator, checking generated APK ZIP alignment with bundletool, or Play Console validation. It also does not certify store listing, privacy declarations, account eligibility, or policy acceptance.

Run the hardware-free signing contracts with a JDK and Python:

```sh
python3 -m unittest discover -s tests -p 'test_*signing.py' -v
python3 -m unittest discover -s tests -p 'test_native_alignment.py' -v
```

Tests generate disposable local keys and exercise real JDK signing and verification, including wrong signers, tampered/unsigned entries, missing secrets, and invalid credentials. Their ZIP fixtures are not Android release packages.

Official references: [Play App Signing and existing keys](https://support.google.com/googleplay/android-developer/answer/9842756), [sign your app](https://developer.android.com/studio/publish/app-signing), and [16 KB page-size support](https://developer.android.com/guide/practices/page-sizes).
