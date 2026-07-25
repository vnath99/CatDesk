use super::contracts::RunState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateTransition {
    pub from: RunState,
    pub to: RunState,
    pub reason: String,
}

pub fn validate_transition(from: RunState, to: RunState) -> Result<StateTransition, String> {
    if is_valid_transition(&from, &to) {
        Ok(StateTransition {
            from,
            to,
            reason: "transition accepted".into(),
        })
    } else {
        Err(format!(
            "invalid delegated run transition from {from:?} to {to:?}"
        ))
    }
}

pub fn is_terminal(state: &RunState) -> bool {
    matches!(
        state,
        RunState::CompletedVerified | RunState::Failed | RunState::Cancelled
    )
}

fn is_valid_transition(from: &RunState, to: &RunState) -> bool {
    use RunState::*;
    match (from, to) {
        (Draft, AwaitingApproval | Ready | Cancelled) => true,
        (AwaitingApproval, Ready | Cancelled) => true,
        (Ready, Starting | Cancelled) => true,
        (Starting, Running | Failed | Cancelled) => true,
        (Running, Paused | NeedsSupervisor | Verifying | Failed | Cancelled) => true,
        (Paused, Running | NeedsSupervisor | Cancelled) => true,
        (NeedsSupervisor, Running | Paused | Failed | Cancelled) => true,
        (Verifying, CompletedVerified | Running | NeedsSupervisor | Failed | Cancelled) => true,
        (CompletedVerified | Failed | Cancelled, _) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_run_state_transitions_succeed() {
        validate_transition(RunState::Draft, RunState::AwaitingApproval).expect("draft approval");
        validate_transition(RunState::AwaitingApproval, RunState::Ready).expect("approved");
        validate_transition(RunState::Ready, RunState::Starting).expect("start");
        validate_transition(RunState::Starting, RunState::Running).expect("running");
        validate_transition(RunState::Running, RunState::Verifying).expect("verify");
        validate_transition(RunState::Verifying, RunState::CompletedVerified).expect("complete");
    }

    #[test]
    fn invalid_and_terminal_transitions_fail() {
        assert!(validate_transition(RunState::Draft, RunState::Running).is_err());
        assert!(validate_transition(RunState::CompletedVerified, RunState::Running).is_err());
        assert!(is_terminal(&RunState::Failed));
        assert!(!is_terminal(&RunState::NeedsSupervisor));
    }
}
