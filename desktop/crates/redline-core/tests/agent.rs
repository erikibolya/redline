use std::time::Duration;

use redline_core::agent::{
    AgentControl, AgentSettings, Claim, ClaimError, ClaimRequest, ClaimState,
};
use tempfile::TempDir;

fn control() -> (TempDir, AgentControl) {
    let dir = TempDir::new().unwrap();
    let control = AgentControl::load(dir.path().join("data"));
    (dir, control)
}

fn claim(
    control: &mut AgentControl,
    session: &str,
    agent: &str,
    project: &str,
    work_key: &str,
    state: ClaimState,
) -> Result<Claim, ClaimError> {
    control.try_claim(ClaimRequest {
        session_id: session.to_string(),
        agent_id: agent.to_string(),
        project: project.to_string(),
        work_key: work_key.to_string(),
        state,
        global_max: 2,
        project_max: 1,
        lease_seconds: 120,
    })
}

#[test]
fn running_slots_are_global_and_per_project_but_waiting_does_not_hold_one() {
    let (_dir, mut control) = control();
    control
        .set_settings(AgentSettings {
            max_parallel: 2,
            project_max_parallel: 1,
            lease_seconds: 120,
        })
        .unwrap();

    claim(
        &mut control,
        "a",
        "agent",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap();
    let project_full = claim(
        &mut control,
        "b",
        "agent",
        "p1",
        "feat/b",
        ClaimState::Running,
    )
    .unwrap_err();
    assert!(matches!(project_full, ClaimError::AtCapacity { .. }));

    claim(
        &mut control,
        "b",
        "agent",
        "p2",
        "feat/b",
        ClaimState::Running,
    )
    .unwrap();
    let global_full = claim(
        &mut control,
        "c",
        "agent",
        "p3",
        "feat/c",
        ClaimState::Running,
    )
    .unwrap_err();
    assert!(matches!(global_full, ClaimError::AtCapacity { .. }));

    // A is waiting for the human: its project slot is free again, but its branch stays owned.
    control
        .set_state("a", "agent", ClaimState::Waiting, 2, 1, 120)
        .unwrap();
    claim(
        &mut control,
        "c",
        "agent",
        "p3",
        "feat/c",
        ClaimState::Running,
    )
    .unwrap();
    let branch_locked = claim(
        &mut control,
        "d",
        "other",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap_err();
    assert!(matches!(branch_locked, ClaimError::BranchLocked { .. }));
}

#[test]
fn one_branch_has_one_owner_until_release() {
    let (_dir, mut control) = control();
    claim(
        &mut control,
        "a",
        "agent",
        "p1",
        "feat/x",
        ClaimState::Running,
    )
    .unwrap();
    let blocked = claim(
        &mut control,
        "b",
        "agent",
        "p2",
        "feat/x",
        ClaimState::Running,
    )
    .unwrap_err();
    assert!(matches!(blocked, ClaimError::BranchLocked { .. }));

    control.release("a", "agent").unwrap();
    claim(
        &mut control,
        "b",
        "agent",
        "p2",
        "feat/x",
        ClaimState::Running,
    )
    .unwrap();
}

#[test]
fn release_and_heartbeat_are_owner_only() {
    let (_dir, mut control) = control();
    claim(
        &mut control,
        "a",
        "agent",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap();

    let heartbeat = control.heartbeat("a", "someone-else", 120).unwrap_err();
    assert!(matches!(heartbeat, ClaimError::NotOwner { .. }));
    let release = control.release("a", "someone-else").unwrap_err();
    assert!(matches!(release, ClaimError::NotOwner { .. }));

    control.heartbeat("a", "agent", 120).unwrap();
    control.release("a", "agent").unwrap();
    assert!(matches!(
        control.release("a", "agent").unwrap_err(),
        ClaimError::NotClaimed
    ));
}

#[test]
fn an_expired_lease_frees_the_branch() {
    let (_dir, mut control) = control();
    control
        .try_claim(ClaimRequest {
            session_id: "a".to_string(),
            agent_id: "agent".to_string(),
            project: "p1".to_string(),
            work_key: "feat/a".to_string(),
            state: ClaimState::Running,
            global_max: 2,
            project_max: 1,
            lease_seconds: 1,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(2100));

    assert!(control.status().claims.is_empty());
    claim(
        &mut control,
        "b",
        "other",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap();
}

#[test]
fn claims_survive_a_reload() {
    let (dir, mut control) = control();
    claim(
        &mut control,
        "a",
        "agent",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap();
    drop(control);

    let mut reloaded = AgentControl::load(dir.path().join("data"));
    assert_eq!(reloaded.status().claims.len(), 1);
    let blocked = claim(
        &mut reloaded,
        "b",
        "other",
        "p1",
        "feat/a",
        ClaimState::Running,
    )
    .unwrap_err();
    assert!(matches!(blocked, ClaimError::BranchLocked { .. }));
}

#[test]
fn settings_are_validated_and_persisted() {
    let (dir, mut control) = control();
    assert!(control
        .set_settings(AgentSettings {
            max_parallel: 0,
            project_max_parallel: 1,
            lease_seconds: 120,
        })
        .is_err());
    assert!(control
        .set_settings(AgentSettings {
            max_parallel: 2,
            project_max_parallel: 1,
            lease_seconds: 10,
        })
        .is_err());

    control
        .set_settings(AgentSettings {
            max_parallel: 3,
            project_max_parallel: 2,
            lease_seconds: 60,
        })
        .unwrap();
    drop(control);

    let reloaded = AgentControl::load(dir.path().join("data"));
    assert_eq!(reloaded.settings().max_parallel, 3);
    assert_eq!(reloaded.settings().project_max_parallel, 2);
    assert_eq!(reloaded.settings().lease_seconds, 60);
}
