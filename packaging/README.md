# Packaging Wallboxd as MSIX

This packages the existing Win32 tray app (`Wallboxd.exe`) with **package
identity** via Desktop Bridge (`EntryPoint="Windows.FullTrustApplication"` +
`rescap:runFullTrust` in `AppxManifest.xml`) — same executable, same code,
just wrapped so Windows treats it as an identified app instead of a bare
`.exe`.

This isn't cosmetic: Windows' lock screen APIs require package identity —
confirmed by testing the unpackaged build directly (`GetCurrentPackageFullName`
returns `APPMODEL_ERROR_NO_PACKAGE`), which is why the plain `.exe` build
can't set the lock screen at all. Package identity alone wasn't quite enough,
though — the newer `UserProfilePersonalizationSettings.TrySetLockScreenImageAsync`
API still silently declined even with identity present; the app uses the
older `LockScreen.SetImageFileAsync` API instead, which is confirmed working.
See the main `AGENTS.md`'s "Lock screen" section for the full investigation.

## Product identity

Assigned by the Microsoft Store (Partner Center) for this app. These values
live in `AppxManifest.xml` and **must match exactly** — don't regenerate or
edit them:

| Field | Value |
|---|---|
| Package/Identity/Name | `abusayed.dev.Wallboxd` |
| Package/Identity/Publisher | `CN=8EA73F0A-0DC1-449F-A896-493A34649C49` |
| Package/Properties/PublisherDisplayName | `abusayed.dev` |
| Package Family Name (PFN) | `abusayed.dev.Wallboxd_a7ccwk4vg4vf2` (derived, not in the manifest) |
| Store ID | `9N8JQGG8R5MQ` |
| Store URL | https://apps.microsoft.com/detail/9N8JQGG8R5MQ |
| MSA App Id | `ad18f3ac-70a9-44d4-bbf8-fd9f9596f08b` — not currently wired into anything (no Microsoft Account/notification features exist yet); recorded here so it isn't lost if that changes later |

`AppxManifest.xml`'s `Version` (`0.1.0.0`) should track `Cargo.toml`'s
`version` — bump both together, same as the existing tag-vs-Cargo.toml check
in `release.yml` for the plain-exe release.

## Assets

`Assets/*.png` are **placeholders** — solid color + "W", generated with a
PowerShell/System.Drawing one-liner, not real artwork. Replace them with
actual Wallboxd branding before Store submission:

| File | Size | Manifest use |
|---|---|---|
| `Square44x44Logo.png` | 44x44 | Taskbar/Start tile icon (required) |
| `Square150x150Logo.png` | 150x150 | Start medium tile (required) |
| `Wide310x150Logo.png` | 310x150 | Start wide tile |
| `StoreLogo.png` | 50x50 | `Properties/Logo` (required) |

This is the minimal valid set (one scale each) — fine for local sideload
testing. Partner Center will separately ask for a fuller icon/screenshot set
during Store listing setup regardless of what's baked into the package.

## Local build + test-sideload

```powershell
# Build + package (unsigned)
.\packaging\build-msix.ps1

# Build + package + sign with a local test certificate + trust it for this user
.\packaging\build-msix.ps1 -TestSign
```

Requires the Windows SDK (`makeappx.exe`, `signtool.exe` — the script finds
them automatically under `Windows Kits\10\bin`).

`-TestSign` generates a self-signed certificate whose Subject matches the
manifest's `Publisher` exactly (required — MSIX installation checks that the
signing cert's subject equals the manifest identity's Publisher field), signs
the `.msix` with it, and trusts it for the current user
(`Cert:\CurrentUser\TrustedPeople`). **This certificate is local-only, never
committed** (see `.gitignore`) — it has nothing to do with Store
distribution; Partner Center strips it and re-signs with the Store's own
certificate on submission.

`CurrentUser\TrustedPeople` alone is **not enough** for `Add-AppxPackage` to
accept a self-signed cert — confirmed directly: it fails with `0x800B0109`
("the root certificate ... must be trusted") even after trusting it there.
Two more things are required, both needing an elevated (Administrator)
PowerShell — this script deliberately doesn't attempt them itself:

```powershell
# Run as Administrator, from the repo root

# 1. Enable sideloading (equivalent to Settings > Privacy & security >
#    For developers > Developer Mode)
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" /v AllowDevelopmentWithoutDevLicense /t REG_DWORD /d 1 /f

# 2. Trust the test cert's root — self-signed certs must chain to a trusted
#    Root CA for AppX deployment, not just TrustedPeople
Import-Certificate -FilePath .\packaging\build\WallboxdTestCert.cer -CertStoreLocation Cert:\LocalMachine\Root
```

Then, from either an elevated or a normal PowerShell, install and verify:

```powershell
Add-AppxPackage -Path .\packaging\build\Wallboxd.msix
Get-AppxPackage -Name abusayed.dev.Wallboxd
```

Launch it from the Start menu (search "Wallboxd") rather than running the
staged `.exe` directly — package identity only applies when Windows launches
the app through its package (AUMID
`abusayed.dev.Wallboxd_a7ccwk4vg4vf2!Wallboxd`), not when the exe is run
standalone.

## Store submission

Upload the package Partner Center asks for during submission (it will want
you to build via `makeappx`/`MSBuild`/Visual Studio the same way — this
script's output is a valid input). Partner Center:
- Re-signs the package with the Store's certificate (the local test
  certificate above is irrelevant to this).
- Handles the icon/screenshot/description Store listing separately from what
  `AppxManifest.xml` declares.
- Requires `Version`'s 4th component to be `0` for submission (already true
  here — bump the first three to match `Cargo.toml` and leave the 4th at
  `0`).
