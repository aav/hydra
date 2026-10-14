use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

use hydra::ChildSpec;
use hydra::ExitReason;
use hydra::GenServer;
use hydra::GenServerOptions;
use hydra::Message;
use hydra::Process;
use hydra::ProcessFlags;
use hydra::Supervisor;
use hydra::SupervisorOptions;
use hydra::SystemMessage;

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

/// A server that takes a second to stop.
struct SecondServer;

impl GenServer for SecondServer {
    type Message = SlowMessage;

    async fn init(&mut self) -> Result<(), ExitReason> {
        Process::set_flags(ProcessFlags::TRAP_EXIT);

        Ok(())
    }

    async fn terminate(&mut self, _reason: ExitReason) {
        Process::sleep(Duration::from_secs(1)).await;
    }
}

/// Starts a supervisor with three children that each take a second to stop, and returns how long its shutdown takes.
async fn shutdown_time(concurrent_shutdown: bool) -> Duration {
    let children = ["first", "second", "third"].map(|id| {
        ChildSpec::new(id)
            .start(|| SecondServer.start_link(GenServerOptions::new()))
            .shutdown(Duration::from_secs(10))
    });

    // The supervisor terminates when its parent signals it, and its exit is not ours.
    Process::set_flags(ProcessFlags::TRAP_EXIT);

    let supervisor = Supervisor::with_children(children)
        .concurrent_shutdown(concurrent_shutdown)
        .start_link(SupervisorOptions::new())
        .await
        .expect("Failed to start supervisor!");

    let children = Supervisor::which_children(supervisor)
        .await
        .expect("Failed to list children!");

    let monitor = Process::monitor(supervisor);
    let started = Instant::now();

    Process::exit(supervisor, ExitReason::from("shutdown"));

    let _ = Process::receiver()
        .for_message::<()>()
        .select(|message| matches!(message, Message::System(SystemMessage::ProcessDown(_, tag, _)) if *tag == monitor))
        .await;

    assert!(
        children
            .iter()
            .all(|child| !Process::alive(child.child.expect("Child is not running!")))
    );

    started.elapsed()
}

#[hydra::test]
async fn shutdown_terminates_children_one_after_another() {
    let elapsed = shutdown_time(false).await;

    assert!(elapsed >= Duration::from_secs(3), "{elapsed:?}");
}

#[hydra::test]
async fn concurrent_shutdown_terminates_children_at_the_same_time() {
    let elapsed = shutdown_time(true).await;

    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}

#[hydra::test]
async fn a_child_whose_start_is_not_sync_starts() {
    // A `Cell` is `Send` and not `Sync`, as a stream often is, and the start
    // holds it across an await.
    let child = ChildSpec::new("unsync").start(|| async {
        let held = std::cell::Cell::new(1);
        Process::sleep(Duration::from_millis(1)).await;
        assert_eq!(held.get(), 1);
        Ok(Process::spawn_link(async {
            loop {
                Process::sleep(Duration::from_secs(1)).await;
            }
        }))
    });

    let supervisor = Supervisor::with_children([child])
        .start_link(SupervisorOptions::new())
        .await
        .expect("Failed to start supervisor!");

    let children = Supervisor::which_children(supervisor)
        .await
        .expect("Failed to list children!");

    assert!(children[0].child.is_some());
}

/// A server that holds a value which is `Send` and not `Sync`, as a stream
/// often is, from before its start until its `init`.
struct HoldsCell(std::cell::Cell<u32>);

impl GenServer for HoldsCell {
    type Message = SlowMessage;

    async fn init(&mut self) -> Result<(), ExitReason> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

#[hydra::test]
async fn a_server_that_is_not_sync_starts_as_a_child() {
    let child = ChildSpec::new("cell").start(|| async {
        let server = HoldsCell(std::cell::Cell::new(1));
        Process::sleep(Duration::from_millis(1)).await;
        server.start_link(GenServerOptions::new()).await
    });

    let supervisor = Supervisor::with_children([child])
        .start_link(SupervisorOptions::new())
        .await
        .expect("Failed to start supervisor!");

    let children = Supervisor::which_children(supervisor)
        .await
        .expect("Failed to list children!");

    assert!(children[0].child.is_some());
}
