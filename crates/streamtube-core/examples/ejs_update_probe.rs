//! Live A3 probe: install the latest verified EJS release into an isolated data
//! directory, then prove the installed scripts still solve a challenge.

use serde_json::json;

fn main() -> anyhow::Result<()> {
    let supplied = std::env::args().nth(1);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let generated = std::env::temp_dir().join(format!(
        "streamtube-ejs-update-probe-{}-{nonce}",
        std::process::id()
    ));
    let dir = supplied
        .clone()
        .unwrap_or_else(|| generated.to_string_lossy().into_owned());
    std::fs::create_dir_all(&dir)?;
    unsafe { std::env::set_var("STREAMTUBE_DATA_DIR", &dir) };
    anyhow::ensure!(streamtube_core::store::set_data_dir(std::path::Path::new(
        &dir
    )));
    let version = streamtube_core::api::nsig::update_from_github()?;
    let out = streamtube_core::api::nsig::solve(json!({
        "type": "preprocessed",
        "preprocessed_player": "_result.n = x => x + '_n'; _result.sig = x => x + '_s';",
        "requests": [{"type": "n", "challenges": ["abc"]}],
    }))?;
    anyhow::ensure!(out["responses"][0]["data"]["abc"] == "abc_n");
    println!("installed EJS {version}");
    println!("installed solver result: abc -> abc_n");
    let ejs = std::path::Path::new(&dir).join("ejs");
    let current = std::fs::read_to_string(ejs.join("CURRENT"))?;
    println!("active generation: {}", current.trim());
    let generation_dir = ejs.join(current.trim());
    for name in [
        "yt.solver.lib.min.js",
        "yt.solver.core.min.js",
        "SHA256SUMS",
    ] {
        let path = generation_dir.join(name);
        println!("{}: {} bytes", name, std::fs::metadata(path)?.len());
    }
    if supplied.is_none() {
        // Best-effort only: Windows may retain store/file handles until exit.
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(())
}
