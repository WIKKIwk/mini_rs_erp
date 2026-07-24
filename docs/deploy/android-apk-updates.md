# Self-hosted Android APK updates

This runbook explains how to distribute Accord Mobile updates from your own
`mini_rs_erp` server without Google Play. The APK can be built on a separate
developer or CI machine, uploaded over SSH, and then downloaded by every
installed mobile client through the backend.

## How it works

The update flow has three parts:

1. A trusted build machine produces a signed APK with a higher Android
   `versionCode`.
2. An operator uploads that APK to the server and runs the release publisher.
3. Accord Mobile reads the release manifest, downloads the immutable APK,
   verifies it, and opens Android's system package installer.

The backend exposes two public read-only endpoints:

| Endpoint | Purpose |
| --- | --- |
| `GET /v1/mobile/app-update/android` | Returns release metadata or `204 No Content` when no release is published. |
| `GET /v1/mobile/app-update/android/apk/{file}` | Streams one immutable APK and supports byte ranges. |

There is intentionally no public APK upload endpoint. Publishing is an
operator action performed through SSH or another authenticated deployment
channel.

## Requirements

- `mini_rs_erp` with the app-update routes deployed;
- a public HTTPS domain that reaches the backend;
- SSH access to the server;
- a persistent server directory writable only by the service operator;
- an APK built for the same Android application ID and signing certificate as
  the installed app;
- a strictly increasing Android `versionCode`.

No paid certificate is required for self-hosted Android distribution. However,
the signing key is the permanent identity of the app. Back it up securely and
never commit it to a public repository.

## One-time server setup

Create a persistent release directory. Adjust the user and group to match the
account that runs `mini_rs_erp`:

```bash
sudo install -d \
  -o mini-rs-erp \
  -g mini-rs-erp \
  -m 0750 \
  /var/lib/mini-rs-erp/mobile-releases
```

Configure the backend:

```env
MOBILE_APP_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases
```

Deploy or restart the backend once so that the code and environment setting
take effect. Publishing later APK versions does not require another restart;
the manifest is read on every request.

Before the first release, the metadata endpoint should return `204`:

```bash
curl -i https://erp.example.com/v1/mobile/app-update/android
```

## Build on another computer

In the `accord_mobile_v2` repository, increase the version in `pubspec.yaml`.
For example:

```yaml
version: 0.2.1+6
```

Then build the release APK against the public backend:

```bash
make apk \
  API_URL=https://erp.example.com \
  APK_NAME=accord.apk
```

The result is:

```text
build/app/outputs/flutter-apk/accord.apk
```

Before publishing, confirm that the APK uses the expected application ID,
version code, ABI, and signing certificate.

## Upload and publish over SSH

Upload the APK to a temporary server path:

```bash
scp \
  build/app/outputs/flutter-apk/accord.apk \
  deploy@erp.example.com:/tmp/accord-0.2.1-6.apk
```

Then publish it on the server:

```bash
ssh deploy@erp.example.com
cd /opt/mini_rs_erp

make publish-mobile-apk \
  APK=/tmp/accord-0.2.1-6.apk \
  VERSION_CODE=6 \
  VERSION_NAME=0.2.1 \
  MOBILE_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases \
  RELEASE_NOTES="Bug fixes and performance improvements"
```

The publisher:

- copies the APK under a content-addressed filename;
- calculates its SHA-256 and size;
- writes a temporary manifest;
- atomically replaces `android.json` only after the APK is ready.

Existing clients that already fetched older metadata continue to use the
immutable APK URL from that metadata.

## Optional and mandatory updates

The default release is optional. Users can install it immediately or postpone
it:

```bash
make publish-mobile-apk \
  APK=/tmp/accord-0.2.1-6.apk \
  VERSION_CODE=6 \
  VERSION_NAME=0.2.1 \
  MOBILE_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases
```

Require only clients older than a specific version:

```bash
make publish-mobile-apk \
  APK=/tmp/accord-0.2.1-6.apk \
  VERSION_CODE=6 \
  VERSION_NAME=0.2.1 \
  MINIMUM_VERSION_CODE=5 \
  MOBILE_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases
```

Require every client with an older version to update:

```bash
make publish-mobile-apk \
  APK=/tmp/accord-0.2.1-6.apk \
  VERSION_CODE=6 \
  VERSION_NAME=0.2.1 \
  MANDATORY_UPDATE=1 \
  MOBILE_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases
```

For long release notes, use a UTF-8 file:

```bash
make publish-mobile-apk \
  APK=/tmp/accord-0.2.1-6.apk \
  VERSION_CODE=6 \
  VERSION_NAME=0.2.1 \
  RELEASE_NOTES_FILE=/tmp/release-notes.txt \
  MOBILE_RELEASE_DIR=/var/lib/mini-rs-erp/mobile-releases
```

## Verify the published release

Read the live manifest:

```bash
curl -fsS \
  https://erp.example.com/v1/mobile/app-update/android
```

Expected fields include:

```json
{
  "version_code": 6,
  "version_name": "0.2.1",
  "minimum_supported_version_code": 0,
  "mandatory": false,
  "apk_url": "/v1/mobile/app-update/android/apk/accord-mobile-android-0.2.1-6-....apk",
  "sha256": "...",
  "size_bytes": 12345678,
  "release_notes": "Bug fixes and performance improvements",
  "published_at": "..."
}
```

Download the URL returned in `apk_url` and compare its SHA-256 with the
manifest before announcing the release.

Also test on at least one Android device that already has the previous version:

1. Open the app or use **Profile → Settings → App update**.
2. Confirm that the new version is shown.
3. Download the APK.
4. Grant **Install unknown apps** to Accord Mobile if Android asks.
5. Complete the system installer.
6. Reopen the app and verify the displayed version and normal login flow.

## First updater-enabled release

An app version that does not contain the updater cannot discover this system.
The first updater-enabled APK must therefore be distributed once through an
existing channel or installed directly. Every later version can be distributed
through the in-app updater.

The first updater-enabled APK must still use the same signing key as the
previously installed APK. If the signing key differs, Android requires a
one-time uninstall and fresh install.

## Rollback and recovery

Android does not allow a normal downgrade over an installed higher
`versionCode`. If a published release is broken:

1. fix or revert the application code;
2. assign a new, higher `versionCode`;
3. build and publish the corrected APK as a hotfix.

If only the release metadata is wrong and no user has installed it, restore the
previous `android.json`. Do not delete old content-addressed APK files
immediately; clients may still hold metadata that points to them.

## Security checklist

- Serve the API and APK only over HTTPS.
- Restrict write access to `MOBILE_APP_RELEASE_DIR`.
- Keep SSH credentials and Android signing keys outside both repositories.
- Never publish an APK with an unexpected package name or certificate.
- Increase `versionCode` for every release.
- Review release notes and mandatory-update settings before publishing.
- Back up the signing key separately from the build machine.
- Retain at least the currently published and previous APK during a release
  transition.

The mobile client independently checks download size, SHA-256, application ID,
newer version code, and signing certificate before opening Android's installer.
Android still asks the user for final installation approval.
