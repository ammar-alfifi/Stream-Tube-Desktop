# Environment setup evidence

Date: 2026-11-26

## Fork and Actions

- `POST /repos/Avaneesh-Inamdar/Stream-Tube-Desktop/forks` -> `202 ammar-alfifi/Stream-Tube-Desktop`
- `GET /repos/ammar-alfifi/Stream-Tube-Desktop` -> `200`
- `PUT /repos/ammar-alfifi/Stream-Tube-Desktop/actions/permissions` -> `204`
- `git push -u fork chore/android-core-split` -> new remote branch created.

## Toolchain

- JDK: `/home/ammar/.local/share/jdk/current` -> Temurin 21.0.13.
- Android SDK: `/home/ammar/Android/Sdk`.
- NDK: `/home/ammar/Android/Sdk/ndk/27.2.12479018`.
- Rust targets installed: `aarch64-linux-android`, `x86_64-linux-android`.

## Emulator

Command:

```bash
$HOME/Android/Sdk/emulator/emulator -avd tvt35 -no-window \
  -gpu swiftshader_indirect -no-audio -no-boot-anim \
  -memory 3072 -cores 4 -no-snapshot-save
```

Observed:

```text
List of devices attached
emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64xa transport_id:3
ro.build.version.release=15
ro.product.cpu.abi=x86_64
sys.boot_completed=1
```

This is emulator-only evidence; no physical arm64 device is attached.

## Release signing key

- Keystore: `/home/ammar/.android/streamtube-release.jks`
- Properties: `/home/ammar/.android/streamtube-release.properties` (mode `0600`)
- Alias: `streamtube`
- Generated locally; not committed.
