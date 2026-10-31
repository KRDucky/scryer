//! Pure observing → stable decision for a processed-folder file version.
//!
//! No IO. The caller injects the clock as `now`. This file is declared from
//! `import/workflow/mod.rs` until `import/processed_folders/mod.rs` exists.
//!
//! Nothing in the pipeline calls this yet; scan will, after the base lands.
#![cfg_attr(not(test), allow(dead_code))]

use chrono::{DateTime, Duration, Utc};

/// One observing row, plus the folder's stability settings, at decision time.
///
/// `stable_scan_count` is the count the scan will compare, after this scan's
/// increment. A growing file is expressed by `mtime` staying near `now`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StabilitySample {
    pub marker_present: bool,
    pub stable_scan_count: i64,
    pub first_seen_at: DateTime<Utc>,
    pub mtime: DateTime<Utc>,
    pub min_stable_scans: i64,
    pub min_age: Duration,
}

/// Contract §2.2 `observing → stable`.
///
/// A marker wins on its own. Otherwise the row is stable only when the scan
/// count and both ages are satisfied: `now - first_seen_at >= min_age` and
/// `now - mtime >= min_age`.
pub(crate) fn observing_becomes_stable(sample: &StabilitySample, now: DateTime<Utc>) -> bool {
    if sample.marker_present {
        return true;
    }
    let Some(eligible) = eligible_at(sample) else {
        return false;
    };
    now >= eligible
}

/// Earliest time a further scan can mark this observing row stable.
///
/// `None` when the scan count can never be satisfied by the clock alone.
/// A marker is eligible from `first_seen_at`. Otherwise the time is
/// `max(first_seen_at, mtime) + min_age`, and only when
/// `stable_scan_count >= min_stable_scans`.
pub(crate) fn eligible_at(sample: &StabilitySample) -> Option<DateTime<Utc>> {
    if sample.marker_present {
        return Some(sample.first_seen_at);
    }
    if sample.stable_scan_count < sample.min_stable_scans {
        return None;
    }
    let anchor = if sample.mtime > sample.first_seen_at {
        sample.mtime
    } else {
        sample.first_seen_at
    };
    Some(anchor + sample.min_age)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + seconds, 0).expect("timestamp")
    }

    fn sample(marker: bool, scans: i64, first_seen: i64, mtime: i64) -> StabilitySample {
        StabilitySample {
            marker_present: marker,
            stable_scan_count: scans,
            first_seen_at: at(first_seen),
            mtime: at(mtime),
            min_stable_scans: 2,
            min_age: Duration::seconds(300),
        }
    }

    #[test]
    fn a_growing_file_never_becomes_stable() {
        let first = sample(false, 2, 0, 600);
        let now = at(600);
        assert!(!observing_becomes_stable(&first, now));
        assert_eq!(eligible_at(&first), Some(at(600) + Duration::seconds(300)));

        let later = sample(false, 4, 0, 1200);
        assert!(!observing_becomes_stable(&later, at(1200)));
        assert!(eligible_at(&later).expect("age gate") > at(1200));
    }

    #[test]
    fn a_marker_wins_without_scans_or_age() {
        let growing = sample(true, 1, 0, 0);
        assert!(observing_becomes_stable(&growing, at(0)));
        assert_eq!(eligible_at(&growing), Some(at(0)));
    }

    #[test]
    fn stable_requires_both_scan_count_and_age() {
        let aged = sample(false, 1, 0, 0);
        assert!(!observing_becomes_stable(&aged, at(300)));
        assert!(eligible_at(&aged).is_none());

        let counted = sample(false, 2, 0, 0);
        assert!(!observing_becomes_stable(&counted, at(299)));
        assert!(observing_becomes_stable(&counted, at(300)));
    }

    #[test]
    fn stability_boundaries_are_inclusive() {
        let by_first_seen = sample(false, 2, 0, -1_000);
        let first_seen_eligible = eligible_at(&by_first_seen).expect("eligible");
        assert_eq!(first_seen_eligible, at(0) + Duration::seconds(300));
        assert!(!observing_becomes_stable(
            &by_first_seen,
            first_seen_eligible - Duration::nanoseconds(1)
        ));
        assert!(observing_becomes_stable(
            &by_first_seen,
            first_seen_eligible
        ));

        let by_mtime = sample(false, 2, -1_000, 0);
        let mtime_eligible = eligible_at(&by_mtime).expect("eligible");
        assert_eq!(mtime_eligible, at(0) + Duration::seconds(300));
        assert!(!observing_becomes_stable(
            &by_mtime,
            mtime_eligible - Duration::nanoseconds(1)
        ));
        assert!(observing_becomes_stable(&by_mtime, mtime_eligible));

        let one_scan_short = sample(false, 1, -1_000, -1_000);
        assert!(eligible_at(&one_scan_short).is_none());
        assert!(!observing_becomes_stable(&one_scan_short, at(10_000)));

        let exact = sample(false, 2, 0, 0);
        assert!(observing_becomes_stable(&exact, at(300)));
    }
}
