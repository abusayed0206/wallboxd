# Packaging Wallboxd as MSIX

This packages the existing Win32 tray app (`Wallboxd.exe`) with **package
identity** via Desktop Bridge (`EntryPoint="Windows.FullTrustApplication"` +
`rescap:runFullTrust` in `AppxManifest.xml`) - same executable, same code,
just wrapped so Windows treats it as an identified app instead of a bare
`.exe`.

This isn't cosmetic: Windows' lock screen APIs require package identity -
confirmed by testing the unpackaged build directly (`GetCurrentPackageFullName`
returns `APPMODEL_ERROR_NO_PACKAGE`), which is why the plain `.exe` build
can't set the lock screen at all. Package identity alone wasn't quite enough,
though - the newer `UserProfilePersonalizationSettings.TrySetLockScreenImageAsync`
API still silently declined even with identity present; the app uses the
older `LockScreen.SetImageFileAsync` API instead, which is confirmed working.
See the main `AGENTS.md`'s "Lock screen" section for the full investigation.

## Product identity

Assigned by the Microsoft Store (Partner Center) for this app. These values
live in `AppxManifest.xml` and **must match exactly** - don't regenerate or
edit them:

| Field | Value |
|---|---|
| Package/Identity/Name | `abusayed.dev.Wallboxd` |
| Package/Identity/Publisher | `CN=8EA73F0A-0DC1-449F-A896-493A34649C49` |
| Package/Properties/PublisherDisplayName | `abusayed.dev` |
| Package Family Name (PFN) | `abusayed.dev.Wallboxd_a7ccwk4vg4vf2` (derived, not in the manifest) |
| Store ID | `9N8JQGG8R5MQ` |
| Store URL | https://apps.microsoft.com/detail/9N8JQGG8R5MQ |
| MSA App Id | `ad18f3ac-70a9-44d4-bbf8-fd9f9596f08b` - not currently wired into anything (no Microsoft Account/notification features exist yet); recorded here so it isn't lost if that changes later |

`AppxManifest.xml`'s `Version` (`0.1.0.0`) should track `Cargo.toml`'s
`version` - bump both together, same as the existing tag-vs-Cargo.toml check
in `release.yml` for the plain-exe release.

## Assets

`Assets/*.png` are **placeholders** - solid color + "W", generated with a
PowerShell/System.Drawing one-liner, not real artwork. Replace them with
actual Wallboxd branding before Store submission:

| File | Size | Manifest use |
|---|---|---|
| `Square44x44Logo.png` | 44x44 | Taskbar/Start tile icon (required) |
| `Square150x150Logo.png` | 150x150 | Start medium tile (required) |
| `Wide310x150Logo.png` | 310x150 | Start wide tile |
| `StoreLogo.png` | 50x50 | `Properties/Logo` (required) |

This is the minimal valid set (one scale each) - fine for local sideload
testing. Partner Center will separately ask for a fuller icon/screenshot set
during Store listing setup regardless of what's baked into the package.

The **exe itself** also has an embedded icon now - `assets/icon.ico` at the
repo root (not under `packaging/`), embedded via `build.rs`, same placeholder
"W" design as the `Assets/*.png` tiles above for visual consistency. This is
what Explorer, Task Manager, and Alt-Tab show for `Wallboxd.exe` regardless
of MSIX packaging, and what the tray icon uses too (`tray.rs`). Replace it
alongside the MSIX tile assets before Store submission - see the main
`AGENTS.md`'s "Icon" section for how it was built and how to verify it.

## Local build + test-sideload

```powershell
# Build + package (unsigned)
.\packaging\build-msix.ps1

# Build + package + sign with a local test certificate + trust it for this user
.\packaging\build-msix.ps1 -TestSign
```

Requires the Windows SDK (`makeappx.exe`, `signtool.exe` - the script finds
them automatically under `Windows Kits\10\bin`).

`-TestSign` generates a self-signed certificate whose Subject matches the
manifest's `Publisher` exactly (required - MSIX installation checks that the
signing cert's subject equals the manifest identity's Publisher field), signs
the `.msix` with it, and trusts it for the current user
(`Cert:\CurrentUser\TrustedPeople`). **This certificate is local-only, never
committed** (see `.gitignore`) - it has nothing to do with Store
distribution; Partner Center strips it and re-signs with the Store's own
certificate on submission.

`CurrentUser\TrustedPeople` alone is **not enough** for `Add-AppxPackage` to
accept a self-signed cert - confirmed directly: it fails with `0x800B0109`
("the root certificate ... must be trusted") even after trusting it there.
Two more things are required, both needing an elevated (Administrator)
PowerShell - this script deliberately doesn't attempt them itself:

```powershell
# Run as Administrator, from the repo root

# 1. Enable sideloading (equivalent to Settings > Privacy & security >
#    For developers > Developer Mode)
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" /v AllowDevelopmentWithoutDevLicense /t REG_DWORD /d 1 /f

# 2. Trust the test cert's root - self-signed certs must chain to a trusted
#    Root CA for AppX deployment, not just TrustedPeople
Import-Certificate -FilePath .\packaging\build\WallboxdTestCert.cer -CertStoreLocation Cert:\LocalMachine\Root
```

Then, from either an elevated or a normal PowerShell, install and verify:

```powershell
Add-AppxPackage -Path .\packaging\build\Wallboxd.msix
Get-AppxPackage -Name abusayed.dev.Wallboxd
```

Launch it from the Start menu (search "Wallboxd") rather than running the
staged `.exe` directly - package identity only applies when Windows launches
the app through its package (AUMID
`abusayed.dev.Wallboxd_a7ccwk4vg4vf2!Wallboxd`), not when the exe is run
standalone.

## Store submission

### Build the submission package

```powershell
.\packaging\build-msix.ps1
```

**No `-TestSign`.** For Partner Center, upload the plain, unsigned
`packaging\build\Wallboxd.msix` that this produces - Partner Center re-signs
with the Store's own certificate during certification, so the local test
certificate (only needed for the sideload testing above) is irrelevant to
submission and shouldn't be part of what you upload.

### Pre-submission checklist

- [ ] **Replace the placeholder artwork.** `assets/icon.ico`, `assets/logo.png`,
      and `packaging/Assets/*.png` are all currently a plain "W" mark
      generated as a placeholder - not real branding. Swap them for real
      artwork before submitting; a placeholder icon on a live listing looks
      unfinished.
- [ ] **Bump the version** in both `Cargo.toml` and `packaging/AppxManifest.xml`
      (`Version="X.Y.Z.0"`) if this isn't the version you want as your first
      public release - `0.1.0.0` is fine for testing but many teams start
      real Store releases at `1.0.0.0`. The 4th component must stay `0` -
      Partner Center requires that for submission.
- [ ] **Take the two screenshots** - see [`docs/SCREENSHOTS.md`](../docs/SCREENSHOTS.md).
- [ ] **Have Store listing text ready** - see [`docs/STORE_LISTING.md`](../docs/STORE_LISTING.md)
      for title/description/keywords copy-paste text.
- [ ] **Host the privacy policy somewhere public** - [`PRIVACY.md`](../PRIVACY.md)
      needs a real URL for Partner Center, not just a repo file. See
      `docs/STORE_LISTING.md`'s "Privacy policy URL" section for hosting
      options (GitHub Pages, raw GitHub link, your own domain).
- [ ] **Decide on license terms** - accept Microsoft's Standard Store License
      Terms (simplest, no extra file needed) unless you specifically want
      custom terms.
- [ ] **Run the age rating questionnaire** in Partner Center - see
      `docs/STORE_LISTING.md`'s "Age rating" note.

### Submitting in Partner Center

1. Sign in to [Partner Center](https://partner.microsoft.com/dashboard) and
   open this app's listing (already exists - Store ID `9N8JQGG8R5MQ`).
2. **Packages**: upload `packaging\build\Wallboxd.msix`. Partner Center
   validates it automatically (checks the manifest, capabilities,
   identity match) - fix anything it flags before continuing.
3. **Store listing**: fill in the fields from `docs/STORE_LISTING.md`,
   upload the screenshots from `docs/SCREENSHOTS.md`, and set the privacy
   policy URL.
4. **Properties**: set category (Personalization), age rating, and
   pricing (free).
5. **Submission**: review everything, submit for certification. Microsoft's
   automated + manual review typically takes anywhere from a few hours to a
   few days.

What Partner Center handles for you (don't try to do these yourself):
- Re-signing the package with the Store's own certificate.
- The icon/screenshot/description Store listing display - separate from
  whatever's declared in `AppxManifest.xml`.
- Distribution, updates, and the install experience for end users.
