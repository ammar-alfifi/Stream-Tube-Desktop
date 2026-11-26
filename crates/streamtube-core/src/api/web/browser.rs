use super::*;
use anyhow::Context;

const SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&uilel=3&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26app%3Ddesktop%26next%3Dhttps%253A%252F%252Fwww.youtube.com%252F";

/// Chromium-based browsers the sign-in can use, most likely first.
fn find_browser() -> Option<(String, PathBuf)> {
    #[cfg(windows)]
    {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        let pf86 =
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let candidates = [
            (
                "Google Chrome",
                format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
            ),
            (
                "Google Chrome",
                format!(r"{pf86}\Google\Chrome\Application\chrome.exe"),
            ),
            (
                "Google Chrome",
                format!(r"{local}\Google\Chrome\Application\chrome.exe"),
            ),
            (
                "Microsoft Edge",
                format!(r"{pf86}\Microsoft\Edge\Application\msedge.exe"),
            ),
            (
                "Microsoft Edge",
                format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
            ),
            (
                "Brave",
                format!(r"{pf}\BraveSoftware\Brave-Browser\Application\brave.exe"),
            ),
            (
                "Brave",
                format!(r"{local}\BraveSoftware\Brave-Browser\Application\brave.exe"),
            ),
            (
                "Chromium",
                format!(r"{local}\Chromium\Application\chrome.exe"),
            ),
        ];
        candidates
            .into_iter()
            .map(|(n, p)| (n.to_string(), PathBuf::from(p)))
            .find(|(_, p)| p.is_file())
    }
    #[cfg(not(windows))]
    {
        let names = [
            ("Google Chrome", "google-chrome"),
            ("Google Chrome", "google-chrome-stable"),
            ("Chromium", "chromium"),
            ("Chromium", "chromium-browser"),
            ("Brave", "brave-browser"),
            ("Brave", "brave"),
            ("Microsoft Edge", "microsoft-edge"),
            ("Microsoft Edge", "microsoft-edge-stable"),
            ("Vivaldi", "vivaldi"),
        ];
        let path = std::env::var_os("PATH")?;
        for (n, bin) in names {
            for dir in std::env::split_paths(&path) {
                let p = dir.join(bin);
                if p.is_file() {
                    return Some((n.to_string(), p));
                }
            }
        }
        None
    }
}

/// Name of the browser the sign-in will open, if there is one.
pub fn browser_name() -> Option<String> {
    find_browser().map(|(n, _)| n)
}

/// The sign-in browser's DevTools connection. It runs over a pair of pipes that
/// only this process holds (`--remote-debugging-pipe`), not a loopback port that
/// any local process could connect to and read the fresh Google cookies from.
struct Cdp {
    to_browser: std::io::PipeWriter,
    replies: mpsc::Receiver<Value>,
    next_id: i64,
}

impl Cdp {
    /// Adds the pipe flags to the browser's command line. Call `spawn` on the
    /// command, then `Cdp::started` with what this returns.
    fn attach(cmd: &mut std::process::Command) -> Result<(Self, ChildEnds)> {
        // the browser reads commands from the first pipe and writes replies to the second
        let (browser_in, to_browser) = std::io::pipe()?;
        let (from_browser, browser_out) = std::io::pipe()?;
        cmd.arg("--remote-debugging-pipe");
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::process::CommandExt;
            let (r, w) = (browser_in.as_raw_fd(), browser_out.as_raw_fd());
            unsafe extern "C" {
                fn dup(fd: i32) -> i32;
                fn dup2(from: i32, to: i32) -> i32;
                fn fcntl(fd: i32, cmd: i32, ...) -> i32;
            }
            const F_SETFD: i32 = 2;
            // SAFETY: only async-signal-safe calls between fork and exec. Chromium
            // expects its input on fd 3 and its output on fd 4.
            unsafe {
                cmd.pre_exec(move || {
                    let (mut r, mut w) = (r, w);
                    if w == 3 {
                        w = dup(w);
                    }
                    if r == 4 {
                        r = dup(r);
                    }
                    if r < 0 || w < 0 || dup2(r, 3) < 0 || dup2(w, 4) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // dup2 onto the same number keeps close-on-exec; clear it
                    fcntl(3, F_SETFD, 0);
                    fcntl(4, F_SETFD, 0);
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn SetHandleInformation(
                    handle: *mut std::ffi::c_void,
                    mask: u32,
                    flags: u32,
                ) -> i32;
            }
            const HANDLE_FLAG_INHERIT: u32 = 1;
            let (r, w) = (browser_in.as_raw_handle(), browser_out.as_raw_handle());
            // SAFETY: plain Win32 calls on handles this process owns. Only the
            // browser's ends become inheritable; ours stay private.
            unsafe {
                if SetHandleInformation(r, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0
                    || SetHandleInformation(w, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            cmd.arg(format!(
                "--remote-debugging-io-pipes={},{}",
                r as usize, w as usize
            ));
        }
        let (tx, replies) = mpsc::channel();
        std::thread::Builder::new()
            .name("signin-devtools".into())
            .spawn(move || {
                use std::io::BufRead;
                let mut reader = std::io::BufReader::new(from_browser);
                let mut buf = Vec::new();
                // messages are JSON, each ended by a NUL byte
                while reader.read_until(0, &mut buf).is_ok_and(|n| n > 0) {
                    if buf.last() == Some(&0) {
                        buf.pop();
                    }
                    if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                        if tx.send(v).is_err() {
                            break;
                        }
                    }
                    buf.clear();
                }
            })?;
        Ok((
            Self {
                to_browser,
                replies,
                next_id: 0,
            },
            ChildEnds(browser_in, browser_out),
        ))
    }

    fn call(&mut self, method: &str) -> Result<Value> {
        use std::io::Write;
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({ "id": id, "method": method })
            .to_string()
            .into_bytes();
        msg.push(0);
        self.to_browser.write_all(&msg)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| anyhow!("devtools: no reply"))?;
            let v = self
                .replies
                .recv_timeout(left)
                .map_err(|_| anyhow!("devtools: no reply"))?;
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(v);
            }
        }
    }

    /// Asks the browser for its cookies.
    fn cookies(&mut self) -> Result<Vec<Value>> {
        let v = self.call("Storage.getCookies")?;
        Ok(v.pointer("/result/cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn close_browser(&mut self) {
        let _ = self.call("Browser.close");
    }
}

/// The browser's ends of the pipes. Dropped once it has started, so that only the
/// browser holds them.
struct ChildEnds(
    #[allow(dead_code)] std::io::PipeReader,
    #[allow(dead_code)] std::io::PipeWriter,
);

fn youtube_cookies(list: &[Value]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in list {
        let domain = c.get("domain").and_then(Value::as_str).unwrap_or("");
        if !(domain == "youtube.com" || domain.ends_with(".youtube.com")) {
            continue;
        }
        if let (Some(n), Some(v)) = (
            c.get("name").and_then(Value::as_str),
            c.get("value").and_then(Value::as_str),
        ) {
            out.insert(n.to_string(), v.to_string());
        }
    }
    out
}

/// Opens a browser window for the user to sign in to YouTube and waits until they
/// have. Returns the account name. `cancel` stops waiting and closes the browser.
pub fn browser_sign_in(cancel: Arc<AtomicBool>) -> Result<String> {
    let (_, exe) = find_browser().ok_or_else(|| {
        anyhow!("No supported browser found. Install Google Chrome, Microsoft Edge, Brave or Chromium, or import a cookies.txt file instead.")
    })?;
    let profile = crate::store::data_dir().join("signin-browser");
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile)?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            "--new-window",
        ])
        .arg(SIGN_IN_URL);
    if let Some(p) = crate::store::proxy_setting() {
        cmd.arg(format!("--proxy-server={p}"));
    }
    let (mut cdp, browser_ends) = Cdp::attach(&mut cmd)?;
    let mut child = cmd.spawn().context("couldn't start the browser")?;
    drop(browser_ends);
    let started = Instant::now();
    let result = loop {
        std::thread::sleep(Duration::from_millis(1500));
        if cancel.load(Ordering::Relaxed) {
            break Err(anyhow!("cancelled"));
        }
        if let Ok(Some(_)) = child.try_wait() {
            // the user closed the window; the browser may have handed off to an
            // already running copy, but with our own profile that doesn't happen
            break Err(anyhow!("The browser was closed before sign-in finished."));
        }
        if started.elapsed() > Duration::from_secs(20 * 60) {
            break Err(anyhow!("Sign-in timed out."));
        }
        let Ok(list) = cdp.cookies() else { continue };
        let cookies = youtube_cookies(&list);
        if has_login(&cookies) && cookies.contains_key("LOGIN_INFO") {
            break Ok(cookies);
        }
    };
    cdp.close_browser();
    for _ in 0..20 {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();
    for _ in 0..8 {
        if std::fs::remove_dir_all(&profile).is_ok() || !profile.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    connect(result?)
}

#[cfg(test)]
mod devtools_pipe_tests {
    /// Starts the real browser (headless) and talks to it over the private pipes.
    #[test]
    #[ignore = "needs Chrome, Edge, Brave or Chromium installed"]
    fn talks_to_the_browser_over_pipes() {
        let (_, exe) = super::find_browser().expect("a browser");
        let profile = std::env::temp_dir().join(format!("st-cdp-test-{}", std::process::id()));
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "about:blank",
            ]);
        let (mut cdp, ends) = super::Cdp::attach(&mut cmd).unwrap();
        let mut child = cmd.spawn().unwrap();
        drop(ends);
        let mut ok = None;
        for _ in 0..20 {
            if let Ok(c) = cdp.cookies() {
                ok = Some(c);
                break;
            }
        }
        cdp.close_browser();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&profile);
        assert!(ok.is_some(), "no reply over the DevTools pipe");
    }
}
