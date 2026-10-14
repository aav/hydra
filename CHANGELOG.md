# 0.1.40-aav.1 - 2026-10-16

The carvers' fork, published to the forge registry.

### Added
- `Supervisor::concurrent_shutdown` terminates the children at the same time when the supervisor itself terminates.
- `Application::shutdown` shuts down the running application with a status, from any process.
- `Application::prep_stop` and `Application::stop` callbacks, called with the state returned from `start` before and after the application stops.

### Changed
- `ChildSpec::start` and `Registry::with_start` no longer require the start future to be `Sync`, only `Send`: a child that holds a stream which is not `Sync` while it starts can start.
- `Supervisor::start_child`, `terminate_child`, `restart_child`, `delete_child`, `which_children`, and `count_children` now wait for the supervisor without a timeout, they gave up after 5 seconds while the supervisor carried on.
- The fields of `SupervisorChildInfo` are now public, so the result of `Supervisor::which_children` can be read.
- `Application::run` now returns a `std::process::ExitCode` (Breaking).
- `Application::start` now returns the application state along with the pid, the type is `Application::State` (Breaking).

### Fixed
- `Application::run` now listens for shutdown signals before it calls `start`, a signal that arrived while starting killed the program without `stop`.
- A process is now registered before it runs, on a multi-threaded runtime it could start without its flags, name, or exit reason.
- A timer is now registered before it runs.

# 0.1.40

### Changed
- Updated dependencies.
- Updated rust edition to `2024`.

# 0.1.39

### Changed
- Updated dependencies.
- `WebsocketMessage` was updated to match `tungstenite`'s message type (Breaking).

# 0.1.38

### Fixed
- Return if process exited from `GenServer::init`.

# 0.1.37

### Fixed
- Properly propagate exit reason returned from `GenServer::init`.

# 0.1.36

### Changed
- Removed unnecessary `Send` bound on `GenServer::stop<T>`.

# 0.1.35

### Changed
- Registry methods now have an argument to specify `timeout` for calls.

# 0.1.34

### Changed
- Registry `stop_process`/`start_process` now has a fast path to check if a local registry has/doesn't have the process.

# 0.1.33

### Added
- Added `WebsocketServer::child_spec()` to make it easier to supervise the server.

# 0.1.32

### Changed
- **BREAKING**: Changed the signature of `WebsocketHandler::accept()` to also pass the connecting socket address down.

# 0.1.31

### Changed
- Updated dependencies.

# 0.1.30

### Fixed
- Use named field for remote protocol to fix serialize issue.

# 0.1.29

### Added
- Added memory statistics to the console api.

# 0.1.28

### Changed
- Updated to the latest release of `dashmap` that fixes a deadlock.

# 0.1.27

### Changed
- Updated to latest release of `dashmap`, removed forked `hydra-dashmap`, better performance and official release.

# 0.1.26

### Added
- Process::info to get debug information on a specific local process.
- ConsoleServer, enabled using the `console` feature which enables a remote debug api for gathering debug information in realtime.

# 0.1.25

### Fixed
- Fixed crash when encoding multiple remote protocol frames at once.

# 0.1.24

### Fixed
- Fixed remote protocol performance on windows platforms.

# 0.1.23

### Changed
- Heavily optimized `HashRing` for fast reads.
- Added unit tests for `HashRing`.

# 0.1.22

### Changed
- Improved remote network protocol performance.

# 0.1.21

### Changed
- Added a nicer api for changing the application config.

# 0.1.20

### Added
- Method to cancel timers returned from `send_after`/`cast_after`.
- Method to read timers returned from `send_after`/`cast_after`.

# 0.1.19

### Changed
- Added `RegistryOptions` for Registry::start/start_link/child_spec.
- Added `SupervisorOptions` for Supervisor::start/start_link/child_spec.
- Ability to customize the process name of the registry (use at own risk).

### Fixed
- Fixed warnings when tracing feature is disabled.

# 0.1.18

### Added
- Added a hash ring implementation.

### Changed
- Updated documentation for `OwnedSemaphore`.

# 0.1.17

### Fixed
- Fixed issue initializing websocket handlers.

# 0.1.16

### Changed
- Removed json feature, hydra provides the best case codec for the internal transport.

# 0.1.15

### Fixed
- Fixed incorrect logging of the supervisor pid when applications shutdown.

# 0.1.13

### Added
- Expose configuration values for maximum inbound message sizes from clients.

# 0.1.12

### Fixed
- Actually fix macro build issue...

# 0.1.11

### Fixed
- Fixed issue building without the macros feature in hydra.

# 0.1.10

### Fixed
- Fixed default features being forced when using websockets crate.

# 0.1.9

### Changed
- Reworked semaphore api so that we can create an owned semaphore that works in dynamic environments (not static).

# 0.1.8

### Changed
- Websocket commands api now has a buffer instead of returning a vec.

### Fixed
- Websocket command execution respects a close command.

# 0.1.7

### Added
- Support for tls websockets using `native-tls` feature.

# 0.1.6

### Fixed
- Use a custom fork of dashmap 'hydra-dashmap' to fix the issues during panics until dashmap is fixed.