use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use hydra::Process;
use hydra::ProcessFlags;

/// The number of processes to spawn.
const SPAWNS: usize = 10_000;

#[test]
fn spawn_registers_before_running() {
    let registered = Arc::new(AtomicUsize::new(0));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    // Spawning from outside of the runtime's workers lets a worker run the process right away.
    rt.block_on(async {
        for index in 0..SPAWNS {
            let registered = registered.clone();

            // Each process relies on its registration the moment it starts, then exits at once.
            Process::spawn(async move {
                Process::set_flags(ProcessFlags::TRAP_EXIT);

                if !Process::flags().contains(ProcessFlags::TRAP_EXIT) {
                    return;
                }

                if Process::register(Process::current(), format!("spawn-{index}")).is_err() {
                    return;
                }

                registered.fetch_add(1, Ordering::Relaxed);
            });
        }

        // A process that ran before it was registered never reports.
        for _ in 0..500 {
            if registered.load(Ordering::Relaxed) == SPAWNS {
                break;
            }

            Process::sleep(Duration::from_millis(10)).await;
        }
    });

    assert!(registered.load(Ordering::Relaxed) == SPAWNS);
    assert!(Process::registered().is_empty());
}
