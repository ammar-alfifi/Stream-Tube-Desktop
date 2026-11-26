# Blockers

## Physical arm64 device unavailable

- Reproduced: `adb devices` shows only `emulator-5554`; no physical arm64 device is connected.
- Impact: final real-device performance/crash/hardware-decoder gates cannot be completed. x86_64 emulator and arm64 translation are useful smoke tests only.
- Attempts: existing wireless/USB device setup was checked; the phone is not currently available to adb.
- Next viable route: connect the physical arm64 device, or deliver the signed APK to the user for installation and device logs.

## Debug Home exceeds RAM budget

- Reproduced: `dumpsys meminfo dev.streamtube.android` on the API 35 emulator reports **435,390 KB TOTAL PSS** (~425 MiB), above the 250 MB browsing budget.
- Evidence: `docs/evidence/B6/meminfo-home-debug.log`.
- Impact: B6 cannot pass on the current debug shell even before playback.
- Next viable route: measure a release build, then bound thumbnail/image caching and Home feed size if the release profile remains over budget.

No other external blocker is currently confirmed. Update this file only with a reproduced failure, attempts, and the next viable route.
