//! RSI before/after with an unrelated-task regression guard (#625).
//!
//! An RSI-appended rule should improve the targeted behavior WITHOUT degrading
//! unrelated tasks — the BinEval capability-vs-guidance failure mode, where
//! accumulated rules help the target but quietly regress working categories.
//!
//! This compares the targeted task before and after the change and requires a
//! genuine improvement, guarded by a control (unrelated) task that must not
//! regress. The change is accepted only when both hold.

use super::scorer::Scorecard;

/// Overall scores for the targeted and control tasks, before and after a change.
#[derive(Debug, Clone, Copy)]
pub struct BeforeAfter {
    pub targeted_before: f64,
    pub targeted_after: f64,
    pub control_before: f64,
    pub control_after: f64,
}

/// The accept/reject decision with its component reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    /// targeted_after - targeted_before.
    pub targeted_delta: f64,
    /// control_after - control_before.
    pub control_delta: f64,
    /// The targeted task improved by at least the required margin.
    pub improved: bool,
    /// The control task did not regress beyond tolerance.
    pub control_held: bool,
    /// Accepted only when it improved AND the control held.
    pub accepted: bool,
}

impl BeforeAfter {
    /// Read the overall scores from four scorecards.
    pub fn from_scorecards(
        targeted_before: &Scorecard,
        targeted_after: &Scorecard,
        control_before: &Scorecard,
        control_after: &Scorecard,
    ) -> Self {
        Self {
            targeted_before: targeted_before.overall(),
            targeted_after: targeted_after.overall(),
            control_before: control_before.overall(),
            control_after: control_after.overall(),
        }
    }

    /// Decide whether the change is a net improvement:
    /// - the targeted task must gain at least `improve_margin`, and
    /// - the control task must not drop more than `control_tolerance`.
    pub fn evaluate(&self, improve_margin: f64, control_tolerance: f64) -> Verdict {
        let targeted_delta = self.targeted_after - self.targeted_before;
        let control_delta = self.control_after - self.control_before;
        let improved = targeted_delta >= improve_margin;
        let control_held = self.control_after >= self.control_before - control_tolerance;
        Verdict {
            targeted_delta,
            control_delta,
            improved,
            control_held,
            accepted: improved && control_held,
        }
    }
}
