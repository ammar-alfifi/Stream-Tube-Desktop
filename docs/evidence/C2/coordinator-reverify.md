# Independent coordinator re-verification — 2026-11-26

## SHA-256

`sha256sum -c docs/evidence/C2/SHA256SUMS.txt` -> all 29 entries OK.

## ELF

- arm64-v8a `libmpv.so`: SONAME `libmpv.so`; LOAD alignment includes `0x4000`.
- x86_64 `libmpv.so`: SONAME `libmpv.so`; LOAD alignment includes `0x4000`.

## Runtime (emulator-5554, API 35, x86_64 native)

```text
preloaded 8 deps OK
dlopen("libmpv.so", RTLD_NOW) = 0xc433a6d2835050f OK
mpv_client_api_version = 0x20005 (client API 2.0)
mpv_create() = 0x71f5779b4120
mpv_initialize() = 0 (success)
mpv_terminate_destroy() OK
RUNTIME VERIFICATION PASSED
```

Physical arm64 device remains pending.
