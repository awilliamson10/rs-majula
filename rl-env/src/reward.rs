/// Terminal episode outcome from one player's (`me`'s) perspective, resolved
/// by [`crate::EnvHarness::is_terminal`] against a scenario's
/// [`crate::scenario::Terminal`] condition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome { Win, Loss, Draw }
