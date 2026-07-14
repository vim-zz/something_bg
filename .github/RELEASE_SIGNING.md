# macOS Release Signing

GitHub tag builds sign the macOS app with a Developer ID Application certificate, submit it to Apple's notary service, staple the accepted ticket to the app, and publish the resulting zip only after all verification checks pass.

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

On macOS, with GitHub CLI authenticated for this repository:

```bash
base64 -i /path/to/developer-id-application.p12 \
  | gh secret set --env release APPLE_DEVELOPER_ID_CERTIFICATE_BASE64
gh secret set --env release APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD
gh secret set --env release APPLE_ID
gh secret set --env release APPLE_APP_SPECIFIC_PASSWORD
gh secret set --env release APPLE_TEAM_ID
```

Configure the environment to allow only tags matching `v*`. When the repository plan supports it, add a required reviewer, prevent self-review, and disable administrator bypass. Environment protections run before GitHub exposes the secrets to the macOS signing job.

## Release flow

Pushing a `v*` tag starts the workflow. The macOS job:

1. Builds and tests the app on runners that have no access to release credentials, then uploads a zip that preserves the bundle's executable permissions.
2. After CI passes, starts a fresh protected-environment runner and downloads the unsigned app.
3. Decodes the certificate into a runner-temporary directory.
4. Imports the certificate and validated notarization credentials into a new random-password keychain.
5. Signs with the hardened runtime and secure timestamp.
6. Notarizes, staples, and verifies the app.
7. Deletes the temporary keychain and certificate file before uploading the zip.

All reusable GitHub Actions in the workflow are pinned to immutable commit SHAs. Update those pins deliberately when upgrading an action.

The GitHub release is not published if signing, notarization, stapling, or Gatekeeper assessment fails.
