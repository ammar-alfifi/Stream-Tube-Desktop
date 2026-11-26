use serde_json::{Value, json};

fn main() -> anyhow::Result<()> {
    let player = std::fs::read_to_string(std::env::args().nth(1).expect("player JS path"))?;
    let challenge: Value = serde_json::from_slice(&std::fs::read(
        std::env::args().nth(2).expect("challenge JSON path"),
    )?)?;
    let n = url::Url::parse(&challenge["url"].as_str().unwrap())?
        .query_pairs()
        .find(|(k, _)| k == "n")
        .map(|(_, v)| v.into_owned())
        .ok_or_else(|| anyhow::anyhow!("challenge URL has no n parameter"))?;
    let output = streamtube_core::api::nsig::solve(json!({
        "type": "player",
        "player": player,
        "output_preprocessed": false,
        "requests": [
            {"type": "sig", "challenges": [challenge["s"].as_str().unwrap()]},
            {"type": "n", "challenges": [n]},
        ],
    }))?;
    println!("{output}");
    Ok(())
}
