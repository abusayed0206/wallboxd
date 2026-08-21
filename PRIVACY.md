# Privacy Policy for Wallboxd

**Last updated: August 2026**

Wallboxd ("the app") is a Windows desktop application published by abusayed.dev.
This policy explains what data the app accesses and what happens to it. It
was written to accurately describe the app's actual, current behavior - not
boilerplate.

## Summary

Wallboxd does not collect, transmit, sell, or share any personal data with
its developer or any third party. It does not use analytics, telemetry,
crash reporting, or advertising of any kind. The only network requests it
makes are to Letterboxd's own servers, to fetch data you've asked it to
fetch, using the Letterboxd username you provide.

## What data the app uses

- **Letterboxd username**: the username you enter in Settings. It is stored
  only in a local configuration file on your device
  (`%LOCALAPPDATA%\Wallboxd\config.json`) and is sent only to Letterboxd's
  own servers (`letterboxd.com` and its poster image CDN,
  `a.ltrbxd.com`) as part of fetching your public diary and poster images -
  the same requests your web browser would make visiting your own public
  Letterboxd diary page.
- **No account, no sign-in**: Wallboxd does not require or support creating
  an account, and does not access your Letterboxd password, email, or any
  private/non-public information. It only reads what is already publicly
  visible on your Letterboxd profile's diary feed.
- **Local files**: the app caches the generated wallpaper/lock-screen images
  and a diagnostic log file on your device, under
  `%LOCALAPPDATA%\Wallboxd\`. These files never leave your device - the app
  has no server component and does not upload anything anywhere.

## What the app does NOT do

- It does not collect analytics, usage statistics, or telemetry of any kind.
- It does not use advertising, and contains no ad SDKs or tracking pixels.
- It does not create user accounts or require sign-in.
- It does not access, read, or transmit any files on your device other than
  its own configuration/cache folder described above.
- It does not share data with any third party other than Letterboxd itself
  (for the specific requests described above), and Microsoft (as the
  platform the app runs on, standard for any Windows/Microsoft Store app).

## Third-party services

Wallboxd fetches data directly from **Letterboxd** (letterboxd.com and its
CDN) using the username you provide. This is a direct connection from your
device to Letterboxd's servers - Wallboxd's developer never sees or
receives this traffic. Your use of Letterboxd's services through this app
is subject to
[Letterboxd's own privacy policy](https://letterboxd.com/privacy/).
Wallboxd is an independent app and is **not affiliated with, endorsed by,
or operated by Letterboxd Limited**.

## Data retention and deletion

All data Wallboxd stores lives entirely on your device, under
`%LOCALAPPDATA%\Wallboxd\`. To delete everything the app has ever stored,
exit the app via its tray icon and delete that folder - there is nothing to
delete anywhere else, because nothing was ever sent anywhere else.

## Children's privacy

Wallboxd does not knowingly collect any data from anyone, of any age. It has
no mechanism to collect personal data in the first place.

## Changes to this policy

If Wallboxd's data practices ever change (for example, adding optional
crash reporting in a future version), this document will be updated and the
"Last updated" date above will reflect the change. Continued use of the app
after an update constitutes acceptance of the revised policy.

## Contact

Questions about this policy or the app's data handling can be sent to the
developer via the contact details listed on the app's Microsoft Store
listing or its GitHub repository.

---

*This document describes Wallboxd's actual behavior as implemented, based
on its source code.*
