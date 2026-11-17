//! Wiring preferences and the policy captured for one gesture (WD12).

use std::fmt;

/// Rubber-band selection behaviour (WD8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    Enclosed,
    Intersecting,
}

/// The follow threshold must be a whole number of at least one (WK2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFollowThreshold;

impl fmt::Display for InvalidFollowThreshold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("follow pin threshold must be at least 1")
    }
}

impl std::error::Error for InvalidFollowThreshold {}

/// In-memory preferences; these are neither document state nor persisted settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WiringPrefs {
    pub follow_on_move: bool,
    follow_pin_threshold: u16,
    pub select_mode: SelectMode,
    pub delete_wires_with_part: bool,
}

impl WiringPrefs {
    /// Rejects a zero threshold instead of silently clamping it.
    pub const fn new(
        follow_on_move: bool,
        follow_pin_threshold: u16,
        select_mode: SelectMode,
        delete_wires_with_part: bool,
    ) -> Result<Self, InvalidFollowThreshold> {
        if follow_pin_threshold == 0 {
            return Err(InvalidFollowThreshold);
        }
        Ok(Self {
            follow_on_move,
            follow_pin_threshold,
            select_mode,
            delete_wires_with_part,
        })
    }

    pub const fn follow_pin_threshold(&self) -> u16 {
        self.follow_pin_threshold
    }

    /// Captures preferences and Alt state for preview or commit (WK3, WD12).
    /// At commit the caller must supply the modifier from the commit event.
    pub const fn follow_policy(&self, modifier_held: bool) -> FollowPolicy {
        FollowPolicy {
            follow_on_move: self.follow_on_move,
            pin_threshold: self.follow_pin_threshold,
            invert: modifier_held,
        }
    }
}

impl Default for WiringPrefs {
    fn default() -> Self {
        Self {
            // WK10 enables this once Q6 is answered yes; HW1 is still pending.
            follow_on_move: false,
            follow_pin_threshold: 10,
            select_mode: SelectMode::Enclosed,
            delete_wires_with_part: false,
        }
    }
}

/// A gesture's captured policy, retained by a command for deterministic redo.
/// Rigid translation is decided before consulting this policy (WD3 step 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowPolicy {
    pub follow_on_move: bool,
    pin_threshold: u16,
    pub invert: bool,
}

impl FollowPolicy {
    pub const fn new(
        follow_on_move: bool,
        pin_threshold: u16,
        invert: bool,
    ) -> Result<Self, InvalidFollowThreshold> {
        if pin_threshold == 0 {
            return Err(InvalidFollowThreshold);
        }
        Ok(Self {
            follow_on_move,
            pin_threshold,
            invert,
        })
    }

    pub const fn pin_threshold(&self) -> u16 {
        self.pin_threshold
    }

    /// WD3 step 2: invert the whole decision, including the strict threshold test.
    pub const fn part_follows(&self, connected_pins: usize) -> bool {
        (self.follow_on_move && connected_pins < self.pin_threshold as usize) ^ self.invert
    }

    /// Policy for a boundary belonging to a moved wire rather than a part.
    pub const fn wire_follows(&self) -> bool {
        self.follow_on_move ^ self.invert
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wk10_defaults_pending_q6() {
        let prefs = WiringPrefs::default();
        assert!(!prefs.follow_on_move);
        assert_eq!(prefs.follow_pin_threshold(), 10);
        assert_eq!(prefs.select_mode, SelectMode::Enclosed);
        assert!(!prefs.delete_wires_with_part);
        let policy = prefs.follow_policy(false);
        assert!(!policy.follow_on_move);
        assert_eq!(policy.pin_threshold(), 10);
        assert!(!policy.invert);
    }

    #[test]
    fn thresholds_are_checked_at_construction() {
        assert_eq!(
            WiringPrefs::new(true, 0, SelectMode::Enclosed, false),
            Err(InvalidFollowThreshold)
        );
        assert_eq!(
            FollowPolicy::new(true, 0, false),
            Err(InvalidFollowThreshold)
        );
        for threshold in [1, u16::MAX] {
            let prefs = WiringPrefs::new(true, threshold, SelectMode::Intersecting, true).unwrap();
            assert_eq!(prefs.follow_pin_threshold(), threshold);
            assert_eq!(prefs.select_mode, SelectMode::Intersecting);
            assert!(prefs.delete_wires_with_part);
            assert_eq!(
                prefs.follow_policy(true),
                FollowPolicy::new(true, threshold, true).unwrap()
            );
        }
    }

    #[test]
    fn wd3_part_follow_truth_table_all_twelve_rows() {
        // follow preference, connected pins, modifier, expected follow.
        let rows = [
            (false, 9, false, false),
            (false, 9, true, true),
            (false, 10, false, false),
            (false, 10, true, true),
            (false, 11, false, false),
            (false, 11, true, true),
            (true, 9, false, true),
            (true, 9, true, false),
            (true, 10, false, false),
            (true, 10, true, true),
            (true, 11, false, false),
            (true, 11, true, true),
        ];
        assert_eq!(rows.len(), 12);
        for (follow, count, modifier, expected) in rows {
            let prefs = WiringPrefs::new(follow, 10, SelectMode::Enclosed, false).unwrap();
            assert_eq!(
                prefs.follow_policy(modifier).part_follows(count),
                expected,
                "follow={follow}, count={count}, modifier={modifier}"
            );
        }
    }

    #[test]
    fn policy_uses_custom_threshold_and_does_not_truncate_counts() {
        let policy = FollowPolicy::new(true, 1, false).unwrap();
        assert!(policy.part_follows(0));
        assert!(!policy.part_follows(1));
        assert!(!policy.part_follows(usize::MAX));
    }

    #[test]
    fn policy_captures_preferences_and_modifier() {
        let mut prefs = WiringPrefs::default();
        let policy = prefs.follow_policy(true);
        prefs.follow_on_move = true;
        assert!(!policy.follow_on_move);
        assert!(policy.invert);
        assert!(policy.part_follows(10));
        assert!(!prefs.follow_policy(false).part_follows(10));
    }

    #[test]
    fn moved_wire_policy_ignores_pin_threshold() {
        for (follow, modifier, expected) in [
            (false, false, false),
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert_eq!(
                FollowPolicy::new(follow, 1, modifier)
                    .unwrap()
                    .wire_follows(),
                expected
            );
        }
    }
}
