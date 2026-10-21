//! Predictions from a single session update. Every result is checked to be a finite number in
//! range, since the inputs are whatever arrived on stdin.

/// The length of the 5-hour rate-limit window, in seconds.
pub const FIVE_HOURS: f64 = 5.0 * 3600.0;
/// The length of the 7-day rate-limit window, in seconds.
pub const SEVEN_DAYS: f64 = 7.0 * 86400.0;

/// Too early in a window, a burst of use would project wildly, so pace waits for this share of it.
const MIN_ELAPSED_SHARE: f64 = 0.15;

/// How far back a fill rate looks, in seconds. Older samples describe a different pace of work,
/// and after this long idle there is no current pace to predict from.
const FILL_LOOKBACK: i64 = 15 * 60;

/// A fill rate needs samples at least this many seconds apart.
const MIN_FILL_SPAN: i64 = 60;

/// Where a rate limit is headed at the average pace since its window started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// The percentage used by the time the window resets.
    pub projected: f64,
    /// Seconds from now until 100%, when that comes before the reset. `0` once at or past it.
    pub eta: Option<f64>,
}

/// Seconds from `now` until `resets_at`, when that is within one `window` from now.
pub fn resets_in(resets_at: f64, now: i64, window: f64) -> Option<f64> {
    let left = resets_at - now as f64;
    (left > 0.0 && left <= window).then_some(left)
}

/// The pace of a rate limit `used` percent through a `window` that resets at `resets_at`. This
/// takes the window to have started one `window` before it resets.
pub fn pace(used: f64, resets_at: f64, now: i64, window: f64) -> Option<Pace> {
    if !(used > 0.0 && used <= 1000.0) {
        return None;
    }
    let elapsed = window - resets_in(resets_at, now, window)?;
    if elapsed < window * MIN_ELAPSED_SHARE {
        return None;
    }
    let projected = used * window / elapsed;
    // Reaching 100% before the reset and projecting at least 100% are the same test, so an eta
    // appears exactly when the projection says the limit will be hit.
    let eta = if used >= 100.0 {
        Some(0.0)
    } else if projected >= 100.0 {
        Some(elapsed * (100.0 - used) / used)
    } else {
        None
    };
    projected.is_finite().then_some(Pace { projected, eta })
}

/// Seconds after the last of `samples` until its percentage reaches 100, at the rate it rose
/// across the latest unbroken stretch of the last `FILL_LOOKBACK` seconds before `now`. The
/// stretch ends at a missing value, a drop (such as a compaction), or a sample that isn't older
/// than the one after it. `samples` are `(seconds since the epoch, percentage)`, oldest first.
pub fn fill_eta(samples: &[(i64, Option<f64>)], now: i64) -> Option<f64> {
    let (&(last_t, last), earlier) = samples.split_last()?;
    let last = last.filter(|last| (0.0..100.0).contains(last))?;
    let since = now.checked_sub(FILL_LOOKBACK)?;
    if last_t < since || last_t > now {
        return None;
    }
    let mut first = (last_t, last);
    for &(t, value) in earlier.iter().rev() {
        match value {
            Some(value) if t < first.0 && t >= since && value <= first.1 => first = (t, value),
            _ => break,
        }
    }
    let (span, rise) = (last_t - first.0, last - first.1);
    if span < MIN_FILL_SPAN || rise <= 0.0 {
        return None;
    }
    let eta = (100.0 - last) / (rise / span as f64);
    eta.is_finite().then_some(eta)
}

/// Dollars per hour over the session so far, idle time included.
pub fn cost_rate(usd: f64, duration_ms: f64) -> Option<f64> {
    if !(usd >= 0.0 && duration_ms >= 60_000.0) {
        return None;
    }
    let rate = usd / (duration_ms / 3_600_000.0);
    rate.is_finite().then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_738_400_000;

    /// A 5-hour window `hours_left` from resetting.
    fn five_hour(used: f64, hours_left: f64) -> Option<Pace> {
        pace(used, NOW as f64 + hours_left * 3600.0, NOW, FIVE_HOURS)
    }

    #[test]
    fn counts_down_to_a_reset_within_the_window() {
        assert_eq!(resets_in(NOW as f64 + 90.0, NOW, FIVE_HOURS), Some(90.0));
        assert_eq!(
            resets_in(NOW as f64 + FIVE_HOURS, NOW, FIVE_HOURS),
            Some(FIVE_HOURS)
        );
        assert_eq!(resets_in(NOW as f64, NOW, FIVE_HOURS), None);
        assert_eq!(resets_in(NOW as f64 - 1.0, NOW, FIVE_HOURS), None);
        assert_eq!(
            resets_in(NOW as f64 + FIVE_HOURS + 1.0, NOW, FIVE_HOURS),
            None
        );
        assert_eq!(resets_in(1e300, NOW, SEVEN_DAYS), None);
    }

    #[test]
    fn projects_the_average_pace_to_the_reset() {
        // 4 of 5 hours gone and 40% used: 50% by the reset, and no limit hit.
        assert_eq!(
            five_hour(40.0, 1.0),
            Some(Pace {
                projected: 50.0,
                eta: None
            })
        );
        // 2.5 hours gone and 60% used: 120% by the reset, and 100% in another 1h40m.
        let pace = five_hour(60.0, 2.5).unwrap();
        assert_eq!(pace.projected, 120.0);
        assert_eq!(pace.eta, Some(6000.0));
        // Exactly on pace for 100% hits it right at the reset.
        assert_eq!(five_hour(80.0, 1.0).unwrap().eta, Some(3600.0));
    }

    #[test]
    fn says_now_once_the_limit_is_reached() {
        assert_eq!(five_hour(100.0, 1.0).unwrap().eta, Some(0.0));
        assert_eq!(five_hour(130.0, 1.0).unwrap().eta, Some(0.0));
    }

    #[test]
    fn waits_until_enough_of_the_window_has_passed() {
        // 15% of 5 hours is 45 minutes.
        assert_eq!(five_hour(10.0, 5.0 - 0.74), None);
        assert!(five_hour(10.0, 5.0 - 0.75).is_some());
    }

    #[test]
    fn has_no_pace_for_odd_input() {
        assert_eq!(five_hour(0.0, 1.0), None);
        assert_eq!(five_hour(-5.0, 1.0), None);
        assert_eq!(five_hour(1e308, 1.0), None);
        assert_eq!(five_hour(f64::NAN, 1.0), None);
        assert_eq!(five_hour(50.0, 0.0), None);
        assert_eq!(five_hour(50.0, -1.0), None);
        assert_eq!(five_hour(50.0, 6.0), None);
        assert_eq!(pace(50.0, 1e300, NOW, SEVEN_DAYS), None);
        assert_eq!(pace(50.0, f64::NAN, NOW, SEVEN_DAYS), None);
        assert_eq!(pace(50.0, NOW as f64 + 3600.0, i64::MIN, FIVE_HOURS), None);
    }

    #[test]
    fn predicts_when_the_context_fills_from_its_recent_rise() {
        // 40% to 60% over ten minutes: 2% a minute, so 20 more minutes to 100%.
        let samples = [
            (NOW - 600, Some(40.0)),
            (NOW - 300, Some(50.0)),
            (NOW, Some(60.0)),
        ];
        assert_eq!(fill_eta(&samples, NOW), Some(1200.0));
        // Counted from the last sample, while it's recent.
        assert_eq!(fill_eta(&samples, NOW + 120), Some(1200.0));
        // A flat stretch slows the rate down.
        let flat = [
            (NOW - 600, Some(50.0)),
            (NOW - 300, Some(50.0)),
            (NOW, Some(60.0)),
        ];
        assert_eq!(fill_eta(&flat, NOW), Some(2400.0));
    }

    #[test]
    fn measures_only_since_the_last_break() {
        let after = |before: Option<f64>| {
            fill_eta(
                &[
                    (NOW - 900, Some(10.0)),
                    (NOW - 600, before),
                    (NOW - 300, Some(40.0)),
                    (NOW, Some(60.0)),
                ],
                NOW,
            )
        };
        // Unbroken from 10%: 50 points in 15 minutes.
        assert_eq!(after(Some(20.0)), Some(720.0));
        // A compaction to 5% or a missing value starts the stretch afresh at 40%.
        assert_eq!(after(Some(90.0)), Some(600.0));
        assert_eq!(after(None), Some(600.0));
        // Samples past the lookback don't count.
        let old = [
            (NOW - 1000, Some(0.0)),
            (NOW - 300, Some(40.0)),
            (NOW, Some(60.0)),
        ];
        assert_eq!(fill_eta(&old, NOW), Some(600.0));
    }

    #[test]
    fn has_no_fill_eta_without_a_recent_rise() {
        let none = |samples: &[(i64, Option<f64>)]| fill_eta(samples, NOW);
        assert_eq!(none(&[]), None);
        assert_eq!(none(&[(NOW, Some(50.0))]), None);
        // Under a minute apart, falling, flat, full, or missing.
        assert_eq!(none(&[(NOW - 59, Some(10.0)), (NOW, Some(50.0))]), None);
        assert_eq!(none(&[(NOW - 300, Some(60.0)), (NOW, Some(50.0))]), None);
        assert_eq!(none(&[(NOW - 300, Some(50.0)), (NOW, Some(50.0))]), None);
        assert_eq!(none(&[(NOW - 300, Some(50.0)), (NOW, Some(100.0))]), None);
        assert_eq!(none(&[(NOW - 300, Some(50.0)), (NOW, None)]), None);
        assert_eq!(none(&[(NOW - 300, Some(-50.0)), (NOW, Some(-10.0))]), None);
        // Idle past the lookback, a last sample from the future, or out-of-order times.
        assert_eq!(
            fill_eta(&[(NOW - 300, Some(40.0)), (NOW, Some(60.0))], NOW + 901),
            None
        );
        assert_eq!(
            none(&[(NOW - 300, Some(40.0)), (NOW + 5, Some(60.0))]),
            None
        );
        assert_eq!(none(&[(NOW, Some(40.0)), (NOW, Some(60.0))]), None);
        assert_eq!(
            fill_eta(&[(0, Some(40.0)), (60, Some(60.0))], i64::MIN),
            None
        );
        // A tiny rise, far from full, is a very long time, but still a number.
        let slow = none(&[(NOW - 900, Some(0.0)), (NOW, Some(1e-300))]);
        assert!(slow.is_some_and(f64::is_finite), "{slow:?}");
    }

    #[test]
    fn averages_cost_over_the_session() {
        assert_eq!(cost_rate(1.5, 1_800_000.0), Some(3.0));
        assert_eq!(cost_rate(0.0, 3_600_000.0), Some(0.0));
        assert_eq!(cost_rate(1.0, 59_999.0), None);
        assert_eq!(cost_rate(-1.0, 3_600_000.0), None);
        assert_eq!(cost_rate(f64::NAN, 3_600_000.0), None);
        assert_eq!(cost_rate(1e308, 60_000.0), None);
    }
}
