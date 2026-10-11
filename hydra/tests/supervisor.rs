use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use hydra::ChildSpec;
use hydra::ExitReason;
use hydra::GenServer;
use hydra::GenServerOptions;
use hydra::Process;
use hydra::ProcessFlags;
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

#[derive(Serialize, Deserialize)]
struct SlowMessage;

/// A server that takes longer to stop than a call waits by default.
struct SlowServer;

impl GenServer for SlowServer {
    type Message = SlowMessage;

    async fn init(&mut self) -> Result<(), ExitReason> {
        Process::set_flags(ProcessFlags::TRAP_EXIT);

        Ok(())
    }

    async fn terminate(&mut self, _reason: ExitReason) {
        Process::sleep(Duration::from_secs(6)).await;
    }
}

#[hydra::test]
async fn terminate_child_waits_for_slow_child() {
    let child = ChildSpec::new("slow")
        .start(|| SlowServer.start_link(GenServerOptions::new()))
        .shutdown(Duration::from_secs(10));

    let supervisor = Supervisor::with_children([child])
        .start_link(SupervisorOptions::new())
        .await
        .expect("Failed to start supervisor!");

    Supervisor::terminate_child(supervisor, "slow")
        .await
        .expect("Failed to terminate child!");

    let children = Supervisor::which_children(supervisor)
        .await
        .expect("Failed to list children!");

    assert!(children[0].child.is_none());
}
