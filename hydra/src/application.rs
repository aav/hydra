use std::future::Future;
use std::process::ExitCode;
use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Builder;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

use crate::ApplicationConfig;
use crate::ExitReason;
use crate::Message;
use crate::Pid;
use crate::Process;
use crate::ProcessFlags;
use crate::SystemMessage;

#[cfg(feature = "console")]
use crate::ConsoleServer;

/// Messages used internally by [Application].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ApplicationMessage {
    ShutdownTimeout,
    Shutdown(u8),
}

/// The process of the running [Application] if there is one.
static APPLICATION: Mutex<Option<Pid>> = Mutex::new(None);

/// Main application logic and entry point for a hydra program.
///
/// [Application] provides graceful shutdown by allowing you to link a process inside the call to `start`.
/// The `run` call will only return once that process has terminated. It's recommended to link a supervisor.
pub trait Application: Sized + Send + 'static {
    /// The state that `start` returns, which is passed on to `prep_stop` and `stop`.
    type State: Send + 'static;

    /// Override to change the application configuration defaults.
    fn config() -> ApplicationConfig {
        ApplicationConfig::default()
    }

    /// Called when an application is starting. You should link a process here and return it's [Pid]
    /// along with the state of the application.
    ///
    /// The [Application] will wait for that process to exit before returning from `run`.
    fn start(&self) -> impl Future<Output = Result<(Pid, Self::State), ExitReason>> + Send;

    /// Called when an application is about to stop, before the linked process created in `start` is told to shut down.
    ///
    /// The returned state is passed on to `stop`. If the linked process exits on its own, this is called after it has exited.
    ///
    /// The graceful shutdown timeout starts once this returns.
    fn prep_stop(&self, state: Self::State) -> impl Future<Output = Self::State> + Send {
        async move { state }
    }

    /// Called when an application has stopped, after the linked process created in `start` has exited.
    ///
    /// This is called with the state returned from `prep_stop` whenever `start` was successful, it should do any cleanup required.
    fn stop(&self, state: Self::State) -> impl Future<Output = ()> + Send {
        async move {
            let _ = state;
        }
    }

    /// Shuts down the running [Application] with the given `status`, this can be called from any process.
    ///
    /// This begins the same graceful shutdown as a shutdown signal and `run` returns `status` once
    /// the linked process created in `start` has exited. Only the first shutdown, or shutdown signal counts.
    fn shutdown(status: u8) {
        if let Some(pid) = *APPLICATION.lock().unwrap() {
            Process::send(pid, ApplicationMessage::Shutdown(status));
        }
    }

    /// Runs the [Application] to completion.
    ///
    /// This method will return when the linked process created in `start` has exited.
    ///
    /// The returned [ExitCode] is:
    /// - The status given to `shutdown`, even if the graceful shutdown timed out.
    /// - Success, if a shutdown signal began the shutdown, or failure if that shutdown timed out.
    /// - Failure, if the linked process exited on its own, regardless of the reason, or if `start` failed.
    fn run(self) -> ExitCode {
        use ApplicationMessage::*;

        let config = Self::config();

        #[cfg(feature = "tracing")]
        if config.tracing_subscribe {
            use std::sync::Once;

            static TRACING_SUBSCRIBE_ONCE: Once = Once::new();

            TRACING_SUBSCRIBE_ONCE.call_once(|| {
                tracing_subscriber::fmt::init();
            });
        }

        #[allow(unused_mut)]
        let mut prev_hook: Option<_> = None;

        #[cfg(feature = "tracing")]
        if config.tracing_panics {
            prev_hook = Some(std::panic::take_hook());

            std::panic::set_hook(Box::new(panic_hook));
        }

        let rt = Runtime::new().unwrap();

        let code = rt.block_on(async move {
            let (tx, rx) = oneshot::channel();

            Process::spawn(async move {
                Process::set_flags(ProcessFlags::TRAP_EXIT);

                *APPLICATION.lock().unwrap() = Some(Process::current());

                // The exit code of the shutdown, or shutdown signal that began the shutdown.
                let mut code: Option<ExitCode> = None;
                let mut signaled = false;

                #[cfg(feature="console")]
                let mut cpid = ConsoleServer::new()
                                        .start_link()
                                        .await
                                        .expect("Failed to start console server!");

                match self.start().await {
                    Ok((pid, state)) => {
                        #[cfg(feature = "tracing")]
                        tracing::info!(supervisor = ?pid, "Application supervisor has started");

                        let mut state = state;

                        let spid = if config.graceful_shutdown {
                            Some(Process::spawn_link(signal_handler()))
                        } else {
                            None
                        };

                        loop {
                            let message = Process::receive::<ApplicationMessage>().await;

                            match message {
                                Message::User(ShutdownTimeout) => {
                                    #[cfg(feature = "tracing")]
                                    tracing::error!(timeout = ?config.graceful_shutdown_timeout, "Application failed to shutdown gracefully");

                                    if signaled {
                                        code = Some(ExitCode::FAILURE);
                                    }

                                    Process::exit(pid, ExitReason::Kill);
                                }
                                Message::User(Shutdown(status)) => {
                                    if code.is_none() {
                                        #[cfg(feature = "tracing")]
                                        tracing::info!(status = ?status, supervisor = ?pid, timeout = ?config.graceful_shutdown_timeout, "Application starting graceful shutdown");

                                        code = Some(ExitCode::from(status));

                                        state = self.prep_stop(state).await;

                                        Process::exit(pid, ExitReason::from("shutdown"));
                                        Process::send_after(Process::current(), ShutdownTimeout, config.graceful_shutdown_timeout);
                                    }
                                }
                                Message::System(SystemMessage::Exit(epid, ereason)) => {
                                    if epid == pid {
                                        if ereason.is_custom() && ereason != "shutdown" {
                                            #[cfg(feature = "tracing")]
                                            tracing::error!(reason = ?ereason, supervisor = ?pid, "Application supervisor has terminated");
                                        } else {
                                            #[cfg(feature = "tracing")]
                                            tracing::info!(reason = ?ereason, supervisor = ?pid, "Application supervisor has exited");
                                        }
                                        break;
                                    } else if spid.is_some_and(|spid| spid == epid) && code.is_none() {
                                        #[cfg(feature = "tracing")]
                                        tracing::info!(reason = ?ereason, supervisor = ?pid, timeout = ?config.graceful_shutdown_timeout, "Application starting graceful shutdown");

                                        code = Some(ExitCode::SUCCESS);
                                        signaled = true;

                                        state = self.prep_stop(state).await;

                                        Process::exit(pid, ExitReason::from("shutdown"));
                                        Process::send_after(Process::current(), ShutdownTimeout, config.graceful_shutdown_timeout);
                                    }

                                    #[cfg(feature = "console")]
                                    if cpid == epid && ereason != "shutdown" {
                                        cpid = ConsoleServer::new()
                                                        .start_link()
                                                        .await
                                                        .expect("Failed to restart console server!");
                                    }
                                }
                                _ => continue,
                            }
                        }

                        // The supervisor exited on its own, so the application wasn't prepared to stop yet.
                        if code.is_none() {
                            state = self.prep_stop(state).await;
                        }

                        self.stop(state).await;
                    }
                    Err(reason) => {
                        #[cfg(feature = "tracing")]
                        tracing::error!(reason = ?reason, "Application supervisor failed to start");

                        #[cfg(not(feature = "tracing"))]
                        let _ = reason;
                    }
                }

                APPLICATION.lock().unwrap().take();

                tx.send(code.unwrap_or(ExitCode::FAILURE)).unwrap();
            });

            rx.await.unwrap_or(ExitCode::FAILURE)
        });

        if let Some(prev_hook) = prev_hook {
            std::panic::set_hook(prev_hook);
        }

        code
    }

    /// Runs the [Application] to completion for tests.
    ///
    /// This method will panic if the process doesn't cleanly exit with `normal` or `shutdown` reasons.
    fn test(self) {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();

        rt.block_on(async move {
            let (tx, rx) = oneshot::channel();

            Process::spawn(async move {
                Process::set_flags(ProcessFlags::TRAP_EXIT);

                match self.start().await {
                    Ok((pid, state)) => {
                        let reason = loop {
                            let message = Process::receive::<()>().await;

                            match message {
                                Message::System(SystemMessage::Exit(epid, ereason)) => {
                                    if epid == pid {
                                        break ereason;
                                    }
                                }
                                _ => continue,
                            }
                        };

                        let state = self.prep_stop(state).await;

                        self.stop(state).await;

                        tx.send(Some(reason)).unwrap();
                    }
                    Err(reason) => {
                        tx.send(Some(reason)).unwrap();
                    }
                }
            });

            if let Ok(Some(reason)) = rx.await
                && reason.is_custom()
                && reason != "shutdown"
            {
                panic!("Exited: {:?}", reason);
            }
        });
    }
}

/// Handles SIGTERM and ctrl+c signals on unix-like platforms.
#[cfg(unix)]
async fn signal_handler() {
    use tokio::signal::unix;

    let mut sigterm =
        unix::signal(unix::SignalKind::terminate()).expect("Failed to register SIGTERM handler");

    tokio::select! {
        _ = sigterm.recv() => {
            Process::exit(Process::current(), ExitReason::from("sigterm"));
        }
        _ = tokio::signal::ctrl_c() => {
            Process::exit(Process::current(), ExitReason::from("ctrl_c"));
        }
    }
}

/// Handles ctrl+c signals on non-unix-like platforms.
#[cfg(not(unix))]
async fn signal_handler() {
    let _ = tokio::signal::ctrl_c().await;

    Process::exit(Process::current(), ExitReason::from("ctrl_c"));
}

/// Handles forwarding panic messages through tracing when enabled.
#[cfg(feature = "tracing")]
fn panic_hook(panic_info: &std::panic::PanicHookInfo) {
    use std::backtrace::Backtrace;
    use std::backtrace::BacktraceStatus;

    use tracing::*;

    let payload = panic_info.payload();

    #[allow(clippy::manual_map)]
    let payload = if let Some(s) = payload.downcast_ref::<&str>() {
        Some(&**s)
    } else if let Some(s) = payload.downcast_ref::<String>() {
        Some(s.as_str())
    } else {
        None
    };

    let location = panic_info.location().map(|location| location.to_string());

    let backtrace = Backtrace::capture();
    let backtrace = if backtrace.status() == BacktraceStatus::Disabled {
        String::from("run with RUST_BACKTRACE=1 environment variable to display a backtrace")
    } else {
        field::display(backtrace).to_string()
    };

    event!(
        target: "hydra",
        Level::ERROR,
        payload = payload,
        location = location,
        backtrace = ?backtrace,
        "A process has panicked",
    );
}
