//! Live SponsorBlock policy probe: fetch real segments, then ask the shared
//! playback policy what to do one second into the first sponsor segment.

use std::collections::HashSet;
use streamtube_core::api::services;
use streamtube_core::playback;
use streamtube_core::store::Settings;

fn main() -> anyhow::Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "W9Ikfr0TKCo".into());
    let mut settings = Settings::default();
    settings.sponsorblock = true;
    settings.sb_actions.insert("sponsor".into(), "skip".into());
    let segments = services::sponsor_segments(
        &video_id,
        &settings.sponsorblock_server,
        &["sponsor".into()],
    )?;
    println!("video={video_id} live_segments={}", segments.len());
    let Some(segment) = segments.first() else {
        anyhow::bail!("no live sponsor segment returned");
    };
    let playhead = segment.start + 1.0;
    let decision = playback::sb_decision(&settings, &segments, playhead, &HashSet::new());
    let skip = decision
        .skips
        .iter()
        .find(|candidate| candidate.uuid == segment.uuid)
        .ok_or_else(|| anyhow::anyhow!("shared policy did not skip the live segment"))?;
    println!(
        "category={} action={} start={:.3} end={:.3} playhead={playhead:.3} => SKIP to {:.3}",
        skip.category, skip.action, skip.start, skip.end, skip.end
    );
    Ok(())
}
