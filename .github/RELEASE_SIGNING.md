# macOS Release Signing

GitHub tag builds sign the macOS app with a Developer ID Application certificate, submit it to Apple's notary service, staple the accepted ticket to the app, and publish the resulting zip only after all verification checks pass.

Starting with v1.11.0, the same protected release also signs the Sparkle update archive and appcast. Release bundles require signed feeds and verify the update archive before extraction.

## Apple credentials

1. In Keychain Access, export the **Developer ID Application** certificate together with its private key as a password-protected `.p12` file. Use a unique, strong export password.
2. Sign in at [account.apple.com](https://account.apple.com/), open **Sign-In and Security** → **App-Specific Passwords**, and generate a password named `something-bg-github-notary`. Two-factor authentication must be enabled on the Apple Account.
3. Record the Apple Account email and Developer Program Team ID associated with the Developer ID certificate.

Keep the certificate outside the repository and store its export password and the app-specific password separately. Revoke and replace either credential immediately if it may have been exposed. Resetting the primary Apple Account password revokes its app-specific passwords.

## GitHub environment

In the repository settings, create an environment named `release` and add these environment secrets:

| Secret | Value |
| --- | --- |
| `APPLE_DEVELOPER_ID_CERTIFICATE_BASE64` | Base64-encoded `.p12` file |
| `APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `APPLE_ID` | Apple Account email used for the Developer Program |
| `APPLE_APP_SPECIFIC_PASSWORD` | App-specific password generated at account.apple.com |
| `APPLE_TEAM_ID` | Developer Program Team ID shown in the signing identity |
| `SPARKLE_PRIVATE_ED_KEY` | Private Ed25519 key exported by Sparkle's `generate_keys`; preserve newlines exactly |

Add this repository variable outside the protected secret store because it is intentionally embedded in the application:

| Variable | Value |
| --- | --- |
| `SPARKLE_PUBLIC_ED_KEY` | Public key printed by Sparkle's `generate_keys` |

On macOS, with GitHub CLI authenticated for this repository:

```bash
base64 -i /path/to/developer-id-application.p12 \
  | gh secret set --env release APPLE_DEVELOPER_ID_CERTIFICATE_BASE64
gh secret set --env release APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD
gh secret set --env release APPLE_ID
gh secret set --env release APPLE_APP_SPECIFIC_PASSWORD
gh secret set --env release APPLE_TEAM_ID
```

Fetch the pinned Sparkle distribution and generate the updater keys once on a controlled Mac:

```bash
./scripts/fetch-sparkle.sh
target/sparkle/2.9.4/bin/generate_keys --account com.vim-zz.something-bg
target/sparkle/2.9.4/bin/generate_keys \
  --account com.vim-zz.something-bg \
  -x /secure/path/something-bg-sparkle-private-key
gh variable set SPARKLE_PUBLIC_ED_KEY --body "the_public_key_printed_above"
gh secret set --env release SPARKLE_PRIVATE_ED_KEY < /secure/path/something-bg-sparkle-private-key
```

The named account keeps this application's updater key separate from Sparkle keys used by other applications. Keep encrypted backups of the private Sparkle key. Do not commit it, pass it as a command-line argument, or expose it in logs. CI passes the secret to `sign_update` through standard input.

Configure the environment to allow only tags matching `v*`. When the repository plan supports it, add a required reviewer, prevent self-review, and disable administrator bypass. Environment protections run before GitHub exposes the secrets to the macOS signing job.

## Release flow

Pushing a `v*` tag starts the workflow. The macOS job:

1. Builds and tests the app on runners that have no access to release credentials, then uploads a zip that preserves the bundle's executable permissions.
2. After CI passes, starts a fresh protected-environment runner and downloads the unsigned app.
3. Decodes the certificate into a runner-temporary directory.
4. Imports the certificate and validated notarization credentials into a new random-password keychain.
5. Signs with the hardened runtime and secure timestamp.
6. Explicitly signs Sparkle's nested XPC services, helpers, framework, and then the outer app.
7. Notarizes, staples, and verifies the app.
8. Archives and EdDSA-signs the final zip, generates and signs `appcast.xml`, and validates their matching versions and sizes.
9. Deletes the temporary keychain, certificate, and private updater-key material before publishing.

All reusable GitHub Actions in the workflow are pinned to immutable commit SHAs. Update those pins deliberately when upgrading an action.

The GitHub release is not published if signing, notarization, stapling, Gatekeeper assessment, archive signing, appcast validation, or tag/version alignment fails. The appcast enclosure uses the immutable versioned release URL, while installed apps read the feed through `releases/latest/download/appcast.xml`.
