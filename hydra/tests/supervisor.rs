use std::sync::Arc;
use std::sync::Mutex;
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

/// What the children of a supervisor did while it shut down, in order.
type Events = Arc<Mutex<Vec<&'static str>>>;

/// A server that takes a second to stop, and records when its stop begins and ends.
struct SecondServer(Events);

impl GenServer for SecondServer {
    type Message = SlowMessage;

    async fn init(&mut self) -> Result<(), ExitReason> {
        Process::set_flags(ProcessFlags::TRAP_EXIT);

        Ok(())
    }

    async fn terminate(&mut self, _reason: ExitReason) {
        self.0.lock().unwrap().push("begin");
        Process::sleep(Duration::from_secs(1)).await;
        self.0.lock().unwrap().push("end");
    }
}

/// A server that ignores the shutdown: its stop never ends by itself.
struct StuckServer;

impl GenServer for StuckServer {
    type Message = SlowMessage;

    async fn init(&mut self) -> Result<(), ExitReason> {
        Process::set_flags(ProcessFlags::TRAP_EXIT);

        Ok(())
    }

    async fn terminate(&mut self, _reason: ExitReason) {
        Process::sleep(Duration::from_secs(3600)).await;
    }
}

/// Starts a supervisor with the given children and concurrent shutdown, shuts it down,
/// and returns how long the shutdown took. Every child is dead afterwards.
async fn shut_down(children: Vec<ChildSpec>) -> Duration {
    // The supervisor terminates when its parent signals it, and its exit is not ours.
    Process::set_flags(ProcessFlags::TRAP_EXIT);

    let supervisor = Supervisor::with_children(children)
        .concurrent_shutdown(true)
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

    let elapsed = started.elapsed();

    assert!(
        children
            .iter()
            .all(|child| !Process::alive(child.child.expect("Child is not running!")))
    );

    elapsed
}

#[hydra::test]
async fn concurrent_shutdown_terminates_children_at_the_same_time() {
    let events = Events::default();

    let children = ["first", "second", "third"]
        .map(|id| {
            let events = events.clone();

            ChildSpec::new(id)
                .start(move || SecondServer(events.clone()).start_link(GenServerOptions::new()))
                .shutdown(Duration::from_secs(10))
        })
        .to_vec();

    shut_down(children).await;

    // Every child begins its stop before the first one ends it.
    let events = events.lock().unwrap().clone();

    assert_eq!(events, ["begin", "begin", "begin", "end", "end", "end"]);
}

#[hydra::test]
async fn concurrent_shutdown_kills_a_child_that_ignores_it() {
    let children = ["first", "second", "third"]
        .map(|id| {
            ChildSpec::new(id)
                .start(|| StuckServer.start_link(GenServerOptions::new()))
                .shutdown(Duration::from_secs(2))
        })
        .to_vec();

    let elapsed = shut_down(children).await;

    // Each timeout counts from the one signal: waiting for the children one after
    // another would take 6 s.
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
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
