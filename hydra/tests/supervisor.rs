use std::time::Duration;

use hydra::ChildSpec;
use hydra::Process;
use hydra::Supervisor;
use hydra::SupervisorOptions;

/// A child that runs until it is terminated.
fn child(id: &str) -> ChildSpec {
    ChildSpec::new(id).start(|| async {
        Ok(Process::spawn_link(async {
            loop {
                Process::sleep(Duration::from_secs(1)).await;
            }
        }))
    })
}

#[hydra::test]
async fn which_children_works() {
    let supervisor = Supervisor::with_children([child("first"), child("second")])
        .start_link(SupervisorOptions::new())
        .await
        .expect("Failed to start supervisor!");

    Supervisor::terminate_child(supervisor, "second")
        .await
        .expect("Failed to terminate child!");

    let children = Supervisor::which_children(supervisor)
        .await
        .expect("Failed to list children!");

    let ids: Vec<&str> = children.iter().map(|child| child.id.as_str()).collect();

    assert!(ids == ["first", "second"]);
    assert!(children[0].child.is_some());
    assert!(children[1].child.is_none());
    assert!(children.iter().all(|child| !child.restarting));
}
