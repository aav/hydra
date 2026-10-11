use std::process::ExitCode;
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
}

struct TestApplication(Behavior);

impl Application for TestApplication {
    fn config() -> ApplicationConfig {
        ApplicationConfig::new().with_graceful_shutdown_timeout(Duration::from_millis(100))
    }

    async fn start(&self) -> Result<Pid, ExitReason> {
        let behavior = self.0;

        if matches!(behavior, Behavior::FailStart) {
            return Err(ExitReason::from("failed"));
        }

        Ok(Process::spawn_link(async move {
            match behavior {
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
        }))
    }
}

// The scenarios run in one test, since there is only one running application to shut down.
#[test]
fn application_exit_codes() {
    assert!(TestApplication(Behavior::FailStart).run() == ExitCode::FAILURE);
    assert!(TestApplication(Behavior::Exit).run() == ExitCode::FAILURE);
    assert!(TestApplication(Behavior::Shutdown(0)).run() == ExitCode::SUCCESS);
    assert!(TestApplication(Behavior::Shutdown(75)).run() == ExitCode::from(75));
    assert!(TestApplication(Behavior::ShutdownAndHang(75)).run() == ExitCode::from(75));
}
