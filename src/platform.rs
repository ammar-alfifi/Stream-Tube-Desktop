use std::ffi::OsStr;

/// Desktop-only opening of URLs and files. The Android shell replaces this
/// with Android intents; core code never depends on a desktop opener.
pub fn open(path: impl AsRef<OsStr>) -> anyhow::Result<()> {
    open::that(path).map_err(Into::into)
}

pub fn open_detached(path: impl AsRef<OsStr>) -> anyhow::Result<()> {
    open::that_detached(path).map_err(Into::into)
}
