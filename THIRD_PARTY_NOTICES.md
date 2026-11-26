# Third-party native media notices

Stream Tube Android may ship native libraries extracted from the official
[mpv-android `2026-09-17` release](https://github.com/mpv-android/mpv-android/releases/tag/2026-09-17).

## mpv / FFmpeg

The pinned upstream binaries are GPL-licensed builds:

- mpv: GPL-2.0-or-later (upstream default with GPL enabled).
- FFmpeg: GPL-2.0-or-later with version 3 enabled.
- Exact source revision recorded in the binary: `v0.41.0-1049-g0b7ed670f`.

Source and build scripts:

- <https://github.com/mpv-android/mpv-android>
- <https://github.com/mpv-player/mpv>
- <https://github.com/FFmpeg/FFmpeg>

Pinned APK and library SHA-256 values are recorded in
[`docs/evidence/C2/SHA256SUMS.txt`](docs/evidence/C2/SHA256SUMS.txt).

For public distribution, replace these binaries with an LGPL build or provide
the corresponding complete source and build information as required by GPLv3.
The documented LGPL build disables FFmpeg GPL/version3 and configures mpv with
`-Dgpl=false`; see `docs/evidence/C2/REPORT.md` §7.

## yt-dlp EJS solver scripts

`third_party/ejs/yt.solver.core.min.js` and `yt.solver.lib.min.js` are generated
by [yt-dlp/ejs](https://github.com/yt-dlp/ejs) and carry their embedded
`SPDX-License-Identifier: Unlicense` notice.

## QuickJS

The Android solver uses [rquickjs](https://github.com/DelSkayn/rquickjs) and its
bundled QuickJS C engine. rquickjs is MIT-licensed; QuickJS is MIT-licensed.
