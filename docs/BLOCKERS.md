# Blockers

## Physical arm64 device unavailable

- Reproduced: `adb devices` shows only `emulator-5554`; no physical arm64 device is connected.
- Impact: final real-device performance/crash/hardware-decoder gates cannot be completed. x86_64 emulator and arm64 translation are useful smoke tests only.
- Attempts: existing wireless/USB device setup was checked; the phone is not currently available to adb.
- Next viable route: connect the physical arm64 device, or deliver the signed APK to the user for installation and device logs.

No other external blocker is currently confirmed. Update this file only with a reproduced failure, attempts, and the next viable route.
