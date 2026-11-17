//! The commands the control plane sent for the run, and what they decide (ADR-0061).
//!
//! The inbound loop records a command, the turn loop and the idle wait read them. All four live in
//! one value behind one lock, so the precedence between them is written down once, in
//! [`RunControl::turn_outcome`].

use std::sync::{Arc, Mutex, PoisonError};

/// How the commands end the run, at the end of a turn or while it waits for input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOutcome {
    /// `Cancel`: the run ends `cancelled`.
    Cancelled,
    /// A tool result ended the run (`end_run_after_turn`): `completed`.
    Completed,
    /// `Release` of an idle run: `end_turn`.
    Released,
    /// `Release` while a blocking request was open: `blocked`.
    Blocked,
}

/// What the control plane asked for. Every command is sticky: one that arrives before the runtime
/// looks for it still counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunControl {
    cancelled: bool,
    released: bool,
    end_run_after_turn: bool,
    /// A blocking request of the current turn (`ToolCallResult.await_input_after_turn`).
    await_input: bool,
}

impl RunControl {
    /// A run whose `Cancel` arrived before `Assign`.
    pub const fn cancelled() -> Self {
        Self {
            cancelled: true,
            released: false,
            end_run_after_turn: false,
            await_input: false,
        }
    }

    pub const fn on_cancel(&mut self) {
        self.cancelled = true;
    }

    pub const fn on_release(&mut self) {
        self.released = true;
    }

    pub const fn on_end_run_requested(&mut self) {
        self.end_run_after_turn = true;
    }

    pub const fn on_await_input(&mut self) {
        self.await_input = true;
    }

    /// A new turn forgets the blocking request of the last one.
    pub const fn on_turn_start(&mut self) {
        self.await_input = false;
    }

    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub const fn is_released(&self) -> bool {
        self.released
    }

    /// How the commands end the run, `None` if none does. `Cancel` wins over a finished task, which
    /// wins over `Release`: a `complete_task` of the turn ends the run `completed` even if the
    /// release arrived during the turn. A blocking request only shapes `Release`.
    pub const fn turn_outcome(&self) -> Option<ControlOutcome> {
        if self.cancelled {
            Some(ControlOutcome::Cancelled)
        } else if self.end_run_after_turn {
            Some(ControlOutcome::Completed)
        } else if self.released {
            Some(self.release_outcome())
        } else {
            None
        }
    }

    /// The outcome of a `Release` of an idle run: `Blocked` whenever a blocking request is open,
    /// whatever else was recorded.
    pub const fn release_outcome(&self) -> ControlOutcome {
        if self.await_input {
            ControlOutcome::Blocked
        } else {
            ControlOutcome::Released
        }
    }
}

/// The [`RunControl`] shared by the inbound loop, the socket handler and the turn loop.
#[derive(Debug, Clone, Default)]
pub struct SharedControl(Arc<Mutex<RunControl>>);

impl SharedControl {
    pub fn new(control: RunControl) -> Self {
        Self(Arc::new(Mutex::new(control)))
    }

    /// The commands as they are now.
    pub fn get(&self) -> RunControl {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a command. The guard is never held across an `.await`.
    pub fn update(&self, record: impl FnOnce(&mut RunControl)) {
        record(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

#[cfg(test)]
mod tests {
    use super::ControlOutcome::{Blocked, Cancelled, Completed, Released};
    use super::*;

    const COMMANDS: [fn(&mut RunControl); 3] = [
        RunControl::on_cancel,
        RunControl::on_end_run_requested,
        RunControl::on_release,
    ];

    fn with(commands: &[fn(&mut RunControl)]) -> RunControl {
        let mut control = RunControl::default();
        for command in commands {
            command(&mut control);
        }
        control
    }

    #[test]
    fn without_a_command_nothing_ends_the_run() {
        assert_eq!(RunControl::default().turn_outcome(), None);
        assert_eq!(with(&[RunControl::on_await_input]).turn_outcome(), None);
    }

    #[test]
    fn each_command_alone_decides_its_outcome() {
        assert_eq!(
            with(&[RunControl::on_cancel]).turn_outcome(),
            Some(Cancelled)
        );
        assert_eq!(
            with(&[RunControl::on_end_run_requested]).turn_outcome(),
            Some(Completed)
        );
        assert_eq!(
            with(&[RunControl::on_release]).turn_outcome(),
            Some(Released)
        );
        assert_eq!(RunControl::cancelled().turn_outcome(), Some(Cancelled));
    }

    #[test]
    fn a_release_during_a_blocking_request_is_blocked() {
        let control = with(&[RunControl::on_await_input, RunControl::on_release]);
        assert_eq!(control.turn_outcome(), Some(Blocked));
    }

    #[test]
    fn a_released_run_with_a_blocking_request_ends_blocked_whatever_else_is_set() {
        let control = with(&[
            RunControl::on_await_input,
            RunControl::on_end_run_requested,
            RunControl::on_release,
        ]);
        assert_eq!(control.release_outcome(), Blocked);
        let cancelled = with(&[RunControl::on_await_input, RunControl::on_cancel]);
        assert_eq!(cancelled.release_outcome(), Blocked);
        assert_eq!(with(&[RunControl::on_release]).release_outcome(), Released);
    }

    #[test]
    fn a_new_turn_forgets_the_blocking_request() {
        let mut control = with(&[RunControl::on_await_input, RunControl::on_release]);
        control.on_turn_start();
        assert_eq!(control.turn_outcome(), Some(Released));
    }

    #[test]
    fn each_pair_decides_in_the_order_cancel_completed_released() {
        // The commands in precedence order: the earlier one of any pair wins, in either arrival
        // order.
        let outcomes = [Cancelled, Completed, Released];
        for (i, first) in COMMANDS.iter().enumerate() {
            for second in COMMANDS.iter().skip(i + 1) {
                assert_eq!(with(&[*first, *second]).turn_outcome(), Some(outcomes[i]));
                assert_eq!(with(&[*second, *first]).turn_outcome(), Some(outcomes[i]));
            }
        }
    }

    #[test]
    fn a_blocking_request_does_not_change_a_cancel_or_a_completion() {
        let cancelled = with(&[RunControl::on_await_input, RunControl::on_cancel]);
        assert_eq!(cancelled.turn_outcome(), Some(Cancelled));
        let completed = with(&[RunControl::on_await_input, RunControl::on_end_run_requested]);
        assert_eq!(completed.turn_outcome(), Some(Completed));
    }

    #[test]
    fn the_shared_value_records_commands_for_every_clone() {
        let shared = SharedControl::default();
        let other = shared.clone();
        other.update(RunControl::on_release);
        assert!(shared.get().is_released());
        assert!(!shared.get().is_cancelled());
    }
}
