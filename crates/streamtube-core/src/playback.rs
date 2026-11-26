//! Platform-independent playback policy: seek/EOF/chapter math, resume position,
//! queue handling, back-cache retention and SponsorBlock decisions. Player shells
//! (the GPUI desktop now, Android later) keep their own state and mpv/GUI side
//! effects, and call into this module for the logic they must not duplicate.

use crate::api::services::Segment;
use crate::api::types::VideoItem;
use crate::store::Settings;
use std::collections::HashSet;

/// More back-cache spilled to disk than this is not worth keeping across a seek.
pub const SEEK_DISK_CACHE_LIMIT: u64 = 128 * 1024 * 1024;

/// libmpv can report EOF for truncated streams. Allow for the last frame's
/// timestamp.
pub fn premature_eof(position: f64, duration: f64) -> bool {
    position.is_finite()
        && position >= 0.0
        && duration.is_finite()
        && duration > 0.0
        && duration - position > (duration * 0.02).clamp(1.0, 2.0)
}

/// Where a seek lands: inside the video, never on its very last instant.
pub fn clamp_seek(t: f64, duration: f64) -> f64 {
    if duration > 0.0 {
        t.clamp(0.0, (duration - 0.1).max(0.0))
    } else {
        t.max(0.0)
    }
}

/// Where a chapter jump from `now` lands: the next chapter's start (`dir` > 0),
/// or the previous chapter's start (in the first chapter, its own start).
pub fn chapter_target(starts: &[f64], now: f64, dir: i32) -> Option<f64> {
    let now = now + 0.01;
    if dir > 0 {
        return starts.iter().copied().find(|&s| s > now);
    }
    let cur = starts.iter().rposition(|&s| s <= now)?;
    Some(starts[cur.saturating_sub(1)])
}

/// Where "Jump ahead" goes: the first local peak close to the strongest heatmap
/// maximum after `time`, i.e. the part of the video most viewers skip to.
pub fn jump_ahead_target(heatmap: &[(f64, f64, f32)], time: f64) -> Option<f64> {
    let from = time + 5.0;
    let later: Vec<&(f64, f64, f32)> = heatmap.iter().filter(|m| m.0 > from).collect();
    let max = later.iter().map(|m| m.2).fold(0.0f32, f32::max);
    if max <= 0.0 {
        return None;
    }
    for (i, m) in later.iter().enumerate() {
        let next = later.get(i + 1).map(|n| n.2).unwrap_or(0.0);
        if m.2 >= max * 0.8 && m.2 >= next {
            return Some(m.0);
        }
    }
    None
}

/// The saved position to resume from: past the intro, and not (almost) the end.
pub fn resume_position(saved: Option<&(f64, f64)>) -> Option<f64> {
    saved.and_then(|&(p, d)| (p > 10.0 && (d <= 0.0 || p < d - 15.0)).then_some(p))
}

/// mpv seek precision: a long unbuffered jump inside an HLS VOD lands on a
/// keyframe, because fetching and decoding an extra GOP before the first
/// picture would cost seconds; everything else seeks exactly.
pub fn seek_mode(t: f64, time: f64, cache_time: f64, hls_vod: bool) -> &'static str {
    let buffered = t >= time && t <= cache_time;
    if (t - time).abs() >= 8.0 && !buffered && hls_vod {
        "absolute+keyframes"
    } else {
        "absolute+exact"
    }
}

/// Whether a seek to `t` should keep already-downloaded data on disk: only
/// backwards jumps (or while an earlier refill is still running), on VOD.
pub fn keeps_back_cache(
    t: f64,
    time: f64,
    refilling: bool,
    loading: bool,
    vod: bool,
    disk_bytes: u64,
) -> bool {
    (t < time || refilling) && !loading && vod && disk_bytes < SEEK_DISK_CACHE_LIMIT
}

/// The refill target for a seek to `t`: reconnect at the end of the buffered
/// range past `t` (or of the running refill), so the back-cache can reattach.
pub fn seek_refill_target(
    t: f64,
    ranges: &[(f64, f64)],
    refilling: Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let end = ranges
        .iter()
        .filter(|&&(start, _)| start > t)
        .map(|&(_, end)| end)
        .chain(refilling.map(|(_, end)| end))
        .fold(t, f64::max);
    (t < end).then_some((t, end))
}

/// Follows the buffered range that covers the refill point `from`, up to `end`:
/// the target ends when the gap is filled, or when the disk cache grew past
/// the limit.
pub fn refill_progress(
    from: f64,
    end: f64,
    ranges: &[(f64, f64)],
    disk_bytes: u64,
) -> Option<(f64, f64)> {
    // Follow only the range that actually covers our last seek or refill point.
    // A separate range ahead may itself be shrinking.
    let filled = ranges
        .iter()
        .filter(|&&(a, b)| a <= from && from <= b)
        .map(|&(_, b)| b)
        .fold(from, f64::max);
    (filled < end && disk_bytes < SEEK_DISK_CACHE_LIMIT).then_some((filled, end))
}

/// Adds `v` to the queue. True when nothing was playing and the caller must
/// start playing it immediately; otherwise `v` plays after the current video
/// and anything queued before it, with the current video heading an empty queue.
pub fn enqueue(
    v: VideoItem,
    current: Option<&VideoItem>,
    queue: &mut Vec<VideoItem>,
    pos: &mut Option<usize>,
) -> bool {
    let Some(cur) = current else {
        queue.push(v);
        *pos = Some(queue.len() - 1);
        return true;
    };
    if queue.is_empty() {
        queue.push(cur.clone());
        *pos = Some(0);
    }
    queue.push(v);
    false
}

/// Index of the video after the current queue position, if any.
pub fn next_index(queue: &[VideoItem], pos: Option<usize>) -> Option<usize> {
    pos.filter(|&p| p + 1 < queue.len()).map(|p| p + 1)
}

/// One pass of SponsorBlock policy at playback time `t`.
pub struct SbDecision {
    /// Segments to jump over now, in timeline order.
    pub skips: Vec<Segment>,
    /// Segment to ask about (action "ask"); the last match wins.
    pub prompt: Option<Segment>,
    /// Inside a segment whose action is "mute".
    pub mute: bool,
}

/// Decides what SponsorBlock should do at playback time `t` under the user's
/// per-category actions. The seek, unskip toast, voting and the actual mute stay
/// with the caller.
pub fn sb_decision(
    s: &Settings,
    segments: &[Segment],
    t: f64,
    skipped: &HashSet<String>,
) -> SbDecision {
    let mut d = SbDecision {
        skips: Vec::new(),
        prompt: None,
        mute: false,
    };
    for seg in segments {
        let action = s
            .sb_actions
            .get(&seg.category)
            .map(String::as_str)
            .unwrap_or("off");
        if action == "off" || seg.action == "poi" || seg.action == "full" {
            continue;
        }
        let inside = t >= seg.start && t < seg.end - 0.3;
        if !inside {
            continue;
        }
        if seg.action == "mute" {
            d.mute = true;
            continue;
        }
        if seg.end - seg.start < s.sb_min_duration {
            continue;
        }
        match action {
            "skip" if !skipped.contains(&seg.uuid) => d.skips.push(seg.clone()),
            "ask" => d.prompt = Some(seg.clone()),
            _ => {}
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_truncated_playback_from_a_normal_end() {
        assert!(premature_eof(3.76, 12.0));
        assert!(!premature_eof(11.96, 12.0));
        assert!(!premature_eof(0.0, 0.0));
        assert!(!premature_eof(0.0, 0.3));
    }

    #[test]
    fn seeks_stay_inside_the_video() {
        assert_eq!(clamp_seek(30.0, 120.0), 30.0);
        assert_eq!(clamp_seek(120.0, 120.0), 119.9);
        assert_eq!(clamp_seek(-1.0, 120.0), 0.0);
        assert_eq!(clamp_seek(5.0, 0.0), 5.0);
        assert_eq!(clamp_seek(0.05, 0.05), 0.0);
        assert_eq!(clamp_seek(0.1, 0.1), 0.0);
    }

    #[test]
    fn chapter_jumps() {
        let c = [0.0, 60.0, 120.0];
        assert_eq!(chapter_target(&c, 30.0, 1), Some(60.0));
        assert_eq!(chapter_target(&c, 60.0, 1), Some(120.0));
        assert_eq!(chapter_target(&c, 130.0, 1), None);
        assert_eq!(chapter_target(&c, 90.0, -1), Some(0.0));
        assert_eq!(chapter_target(&c, 60.0, -1), Some(0.0));
        assert_eq!(chapter_target(&c, 10.0, -1), Some(0.0));
        assert_eq!(chapter_target(&[], 10.0, 1), None);
    }

    #[test]
    fn jump_ahead_finds_the_first_strong_local_peak() {
        let heat = vec![
            (0.0, 1.0, 0.1),
            (20.0, 21.0, 0.9),
            (21.0, 22.0, 0.5),
            (60.0, 61.0, 1.0),
            (200.0, 201.0, 0.7),
        ];
        // the 0.9 entry is the first local peak close to the strongest one
        assert_eq!(jump_ahead_target(&heat, 10.0), Some(20.0));
        // nothing hot ahead
        assert_eq!(jump_ahead_target(&heat, 200.0), None);
    }

    #[test]
    fn resumes_mid_video_only() {
        assert_eq!(resume_position(Some(&(120.0, 600.0))), Some(120.0));
        assert_eq!(resume_position(Some(&(5.0, 600.0))), None); // still the intro
        assert_eq!(resume_position(Some(&(590.0, 600.0))), None); // (almost) the end
        assert_eq!(resume_position(Some(&(300.0, 0.0))), Some(300.0)); // unknown duration
        assert_eq!(resume_position(None), None);
    }

    #[test]
    fn keyframe_seeks_only_for_remote_hls_vod_jumps() {
        assert_eq!(seek_mode(100.0, 10.0, 20.0, true), "absolute+keyframes");
        assert_eq!(seek_mode(100.0, 10.0, 120.0, true), "absolute+exact"); // buffered
        assert_eq!(seek_mode(100.0, 10.0, 20.0, false), "absolute+exact"); // not HLS VOD
        assert_eq!(seek_mode(12.0, 10.0, 20.0, true), "absolute+exact"); // short jump
    }

    #[test]
    fn back_cache_survives_backward_vod_seeks_only() {
        let vod = |t: f64, time: f64, refilling: bool| {
            keeps_back_cache(t, time, refilling, false, true, 0)
        };
        assert!(vod(30.0, 100.0, false));
        assert!(!vod(130.0, 100.0, false)); // forward
        assert!(vod(130.0, 100.0, true)); // already refilling
        assert!(!keeps_back_cache(30.0, 100.0, false, true, true, 0)); // loading
        assert!(!keeps_back_cache(30.0, 100.0, false, false, false, 0)); // live
        assert!(!keeps_back_cache(
            30.0,
            100.0,
            false,
            false,
            true,
            SEEK_DISK_CACHE_LIMIT
        )); // disk full
        assert_eq!(
            seek_refill_target(30.0, &[(0.0, 20.0), (40.0, 80.0)], None),
            Some((30.0, 80.0))
        );
        assert_eq!(seek_refill_target(30.0, &[(0.0, 20.0)], None), None); // nothing buffered ahead
        assert_eq!(
            refill_progress(30.0, 80.0, &[(0.0, 55.0), (70.0, 90.0)], 0),
            Some((55.0, 80.0))
        );
        assert_eq!(refill_progress(30.0, 55.0, &[(0.0, 55.0)], 0), None); // gap filled
    }

    fn item(id: &str) -> VideoItem {
        VideoItem {
            id: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn enqueue_plays_immediately_only_when_idle() {
        // idle: the item becomes the queue and starts playing
        let mut queue = Vec::new();
        let mut pos = None;
        assert!(enqueue(item("a"), None, &mut queue, &mut pos));
        assert_eq!((queue.len(), pos), (1, Some(0)));
        // playing with an empty queue: the current video heads it, the new one follows
        let mut queue = Vec::new();
        let mut pos = None;
        let cur = item("now");
        assert!(!enqueue(item("b"), Some(&cur), &mut queue, &mut pos));
        assert_eq!((queue.len(), pos), (2, Some(0)));
        assert_eq!(
            queue.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            ["now", "b"]
        );
        // queue already has items: just append
        assert!(!enqueue(item("c"), Some(&cur), &mut queue, &mut pos));
        assert_eq!((queue.len(), pos), (3, Some(0)));
        assert_eq!(
            queue.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            ["now", "b", "c"]
        );
    }

    #[test]
    fn next_index_stays_inside_the_queue() {
        let q = vec![item("a"), item("b"), item("c")];
        assert_eq!(next_index(&q, None), None);
        assert_eq!(next_index(&q, Some(0)), Some(1));
        assert_eq!(next_index(&q, Some(2)), None);
        assert_eq!(next_index(&[], Some(0)), None);
    }

    fn seg(category: &str, action: &str, start: f64, end: f64, uuid: &str) -> Segment {
        Segment {
            category: category.into(),
            action: action.into(),
            start,
            end,
            uuid: uuid.into(),
        }
    }

    fn settings(actions: &[(&str, &str)], min_duration: f64) -> Settings {
        let mut s = Settings::default();
        s.sb_actions = actions
            .iter()
            .map(|(c, a)| (c.to_string(), a.to_string()))
            .collect();
        s.sb_min_duration = min_duration;
        s
    }

    #[test]
    fn sponsorblock_skips_prompts_and_mutes_by_action() {
        let s = settings(
            &[
                ("sponsor", "skip"),
                ("intro", "ask"),
                ("music_offtopic", "show"),
            ],
            1.0,
        );
        let segments = vec![
            seg("sponsor", "skip", 10.0, 20.0, "s1"),
            // the configured "ask" wins over the segment's own action
            seg("intro", "skip", 12.0, 18.0, "i1"),
            // category not configured -> off
            seg("selfpromo", "skip", 11.0, 19.0, "x1"),
            seg("music_offtopic", "mute", 11.0, 25.0, "m1"),
            // ahead of the playhead
            seg("sponsor", "skip", 30.0, 40.0, "s2"),
            // POI segments are never skipped
            seg("sponsor", "poi", 12.0, 18.0, "p1"),
        ];
        let d = sb_decision(&s, &segments, 15.0, &HashSet::new());
        assert_eq!(
            d.skips.iter().map(|g| g.uuid.as_str()).collect::<Vec<_>>(),
            ["s1"]
        );
        assert_eq!(d.prompt.as_ref().map(|g| g.uuid.as_str()), Some("i1"));
        assert!(d.mute);
        // an already-skipped segment is not returned again
        let mut skipped = HashSet::new();
        skipped.insert("s1".to_string());
        let d = sb_decision(&s, &segments, 15.0, &skipped);
        assert!(d.skips.is_empty());
    }

    #[test]
    fn sponsorblock_filters_by_duration_and_playhead() {
        let s = settings(&[("sponsor", "skip")], 3.0);
        let segments = vec![seg("sponsor", "skip", 10.0, 20.0, "s1")];
        // shorter than the configured minimum duration
        let short = vec![seg("sponsor", "skip", 13.0, 15.0, "sh")];
        assert!(
            sb_decision(&s, &short, 14.0, &HashSet::new())
                .skips
                .is_empty()
        );
        // before the segment, and in its last 0.3 seconds
        assert!(
            sb_decision(&s, &segments, 5.0, &HashSet::new())
                .skips
                .is_empty()
        );
        assert!(
            sb_decision(&s, &segments, 19.8, &HashSet::new())
                .skips
                .is_empty()
        );
        assert!(
            !sb_decision(&s, &segments, 10.0, &HashSet::new())
                .skips
                .is_empty()
        );
    }
}
