use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use hydra::Application;
use hydra::ApplicationConfig;
use hydra::ExitReason;
use hydra::Pid;
use hydra::Process;
use hydra::ProcessFlags;

/// What the linked process of the test application does.
#[derive(Clone, Copy)]
enum Behavior {
    /// Fails to start.
    FailStart,
    /// Exits on its own with a normal reason.
    Exit,
    /// Shuts down the application with a status, then waits for the shutdown.
    Shutdown(u8),
    /// Shuts down the application with a status, then ignores the shutdown.
    ShutdownAndHang(u8),
    /// Gets a SIGTERM while it starts, then waits for the shutdown.
    SignalWhileStarting,
}

/// Sends a SIGTERM to this program, and returns once it was sent.
fn sigterm() {
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("Failed to run kill!");

    assert!(sent.success());
}

struct TestApplication(Behavior);

impl Application for TestApplication {
    type State = ();

    fn config() -> ApplicationConfig {
        ApplicationConfig::new()
            .with_graceful_shutdown(true)
            .with_graceful_shutdown_timeout(Duration::from_millis(100))
    }

    async fn start(&self) -> Result<(Pid, Self::State), ExitReason> {
        let behavior = self.0;

        if matches!(behavior, Behavior::FailStart) {
            return Err(ExitReason::from("failed"));
        }

        if matches!(behavior, Behavior::SignalWhileStarting) {
            sigterm();
        }

        let pid = Process::spawn_link(async move {
            match behavior {
                Behavior::SignalWhileStarting => {}
                Behavior::Shutdown(status) => {
                    TestApplication::shutdown(status);
                }
                Behavior::ShutdownAndHang(status) => {
                    Process::set_flags(ProcessFlags::TRAP_EXIT);

                    TestApplication::shutdown(status);
                }
                _ => return,
            }

            // Wait for the shutdown, or forever when trapping exits.
            loop {
                Process::sleep(Duration::from_secs(1)).await;
            }
        });

        Ok((pid, ()))
    }
}

/// What a callback of the state application saw: its name, whether the linked process was alive, and the state.
type Seen = (&'static str, bool, Vec<&'static str>);

struct StateApplication {
    behavior: Behavior,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl StateApplication {
    /// Runs the application with the given `behavior` and returns what the callbacks saw.
    fn seen(behavior: Behavior) -> Vec<Seen> {
        let seen = Arc::new(Mutex::new(Vec::new()));

        let _ = StateApplication {
            behavior,
            seen: seen.clone(),
        }
        .run();

        seen.lock().unwrap().clone()
    }
}

impl Application for StateApplication {
    type State = (Pid, Vec<&'static str>);

    fn config() -> ApplicationConfig {
        ApplicationConfig::new().with_graceful_shutdown(true)
    }

    async fn start(&self) -> Result<(Pid, Self::State), ExitReason> {
        let behavior = self.behavior;

        if matches!(behavior, Behavior::SignalWhileStarting) {
            sigterm();
        }

        let pid = Process::spawn_link(async move {
            if let Behavior::Shutdown(status) = behavior {
                StateApplication::shutdown(status);
            }

            if !matches!(behavior, Behavior::Exit) {
                // Wait for the shutdown.
                loop {
                    Process::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        Ok((pid, (pid, vec!["start"])))
    }

    async fn prep_stop(&self, state: Self::State) -> Self::State {
        let (pid, mut state) = state;

        self.seen
            .lock()
            .unwrap()
            .push(("prep_stop", Process::alive(pid), state.clone()));

        state.push("prep_stop");

        (pid, state)
    }

    async fn stop(&self, state: Self::State) {
        let (pid, state) = state;

        self.seen
            .lock()
            .unwrap()
            .push(("stop", Process::alive(pid), state));
    }
}

// The scenarios run in one test, since there is only one running application to shut down.
#[test]
fn application_works() {
    assert!(TestApplication(Behavior::FailStart).run() == ExitCode::FAILURE);
    assert!(TestApplication(Behavior::Exit).run() == ExitCode::FAILURE);
    assert!(TestApplication(Behavior::Shutdown(0)).run() == ExitCode::SUCCESS);
    assert!(TestApplication(Behavior::Shutdown(75)).run() == ExitCode::from(75));
    assert!(TestApplication(Behavior::ShutdownAndHang(75)).run() == ExitCode::from(75));

    // A shutdown prepares to stop while the linked process is alive, then stops once it has exited.
    assert!(
        StateApplication::seen(Behavior::Shutdown(0))
            == [
                ("prep_stop", true, vec!["start"]),
                ("stop", false, vec!["start", "prep_stop"]),
            ]
    );

    // A signal that arrives while starting is kept, and begins the shutdown once the start has returned.
    assert!(TestApplication(Behavior::SignalWhileStarting).run() == ExitCode::SUCCESS);
    assert!(
        StateApplication::seen(Behavior::SignalWhileStarting)
            == [
                ("prep_stop", true, vec!["start"]),
                ("stop", false, vec!["start", "prep_stop"]),
            ]
    );

    // A linked process that exited on its own is gone before the application prepares to stop.
    assert!(
        StateApplication::seen(Behavior::Exit)
            == [
                ("prep_stop", false, vec!["start"]),
                ("stop", false, vec!["start", "prep_stop"]),
            ]
    );
}
