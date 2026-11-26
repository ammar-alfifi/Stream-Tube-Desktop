# Arabic core locale evidence

Date: 2026-11-26

- `ar.json`: 837/837 keys, same order as the reference catalog.
- Placeholder multisets: 0 mismatches.
- Empty translated values: 0.
- `AppLanguage::Arabic` serializes as `ar`; `is_rtl()` returns true only for Arabic.
- Focused i18n suite: 7 passed, 0 failed (`i18n-tests.log`).
- UI layout, Arabic IME, shaping, and dynamic plural audit remain Track E work; this evidence covers the shared core catalog only.
