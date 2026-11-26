//! YouTube nsig/signature solver using the bundled yt-dlp EJS scripts.
//!
//! The scripts run in a fresh QuickJS runtime with no host functions, a memory
//! limit, and a deadline. Only JSON crosses the boundary.

use anyhow::{Result, anyhow};
use rquickjs::{Context, Runtime};
use serde_json::Value;
use std::time::{Duration, Instant};

const EJS_LIB: &str = include_str!("../../../../third_party/ejs/yt.solver.lib.min.js");
const EJS_CORE: &str = include_str!("../../../../third_party/ejs/yt.solver.core.min.js");
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const STACK_LIMIT: usize = 8 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);

/// Solves one batch of `n`/`sig` challenges with the bundled EJS scripts.
pub fn solve(input: Value) -> Result<Value> {
    let started = Instant::now();
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(STACK_LIMIT);
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > DEADLINE)));
    let context = Context::full(&runtime)?;
    context.with(|ctx| -> Result<Value> {
        let _: () = ctx.eval(EJS_LIB).map_err(|e| anyhow!("load EJS library: {e}"))?;
        let _: () = ctx
            .eval("var meriyah = lib.meriyah; var astring = lib.astring;")
            .map_err(|e| anyhow!("bind EJS globals: {e}"))?;
        let _: () = ctx.eval(EJS_CORE).map_err(|e| anyhow!("load EJS core: {e}"))?;
        let input = input.to_string();
        let solve: rquickjs::Function = ctx.eval(
            "(s) => { try { return JSON.stringify(jsc(JSON.parse(s))); } catch (e) { return JSON.stringify({error: String(e), stack: e.stack}); } }",
        )?;
        let output: String = solve.call((input,))?;
        let value: Value = serde_json::from_str(&output)?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(anyhow::anyhow!("{error}"));
        }
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::solve;
    use serde_json::json;

    #[test]
    fn solves_preprocessed_challenges_inside_quickjs() {
        let out = solve(json!({
            "type": "preprocessed",
            "preprocessed_player": "_result.n = x => x + '_n'; _result.sig = x => x + '_s';",
            "requests": [
                {"type": "n", "challenges": ["abc"]},
                {"type": "sig", "challenges": ["def"]},
            ],
        }))
        .unwrap();
        assert_eq!(out["responses"][0]["data"]["abc"], "abc_n");
        assert_eq!(out["responses"][1]["data"]["def"], "def_s");
    }

    #[test]
    fn malformed_player_is_an_error_not_a_panic() {
        assert!(
            solve(json!({
                "type": "player",
                "player": "var x = 1;",
                "requests": [{"type": "n", "challenges": ["abc"]}],
            }))
            .is_err()
        );
    }
}
