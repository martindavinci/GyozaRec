# winget

GyozaRec is published to the Windows Package Manager as `MartinDaVinci.GyozaRec`.

## First submission (once, by hand)

The catalog only accepts automatic updates for packages it already has, so the first version goes in manually:

1. Publish a GitHub release with the MSI attached.
2. Install the manifest tool: `winget install Microsoft.WingetCreate`.
3. Run it with the MSI's download URL:

   ```
   wingetcreate new https://github.com/martindavinci/GyozaRec/releases/download/v0.2.0/GyozaRec_0.2.0_x64.msi
   ```

4. Use the identifier `MartinDaVinci.GyozaRec`, check the generated fields (name, publisher, license MIT, short description), and let `wingetcreate` open the pull request against `microsoft/winget-pkgs`.

## Later versions (automatic)

`.github/workflows/winget.yml` runs when a release is published and opens the update pull request for you. It needs a repository secret named `WINGET_TOKEN`: a classic GitHub personal access token with the `public_repo` scope, from an account that has forked `microsoft/winget-pkgs`. Without the secret the workflow skips itself.
