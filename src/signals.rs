use tokio::sync::mpsc;

use futures::stream::StreamExt;
use signal_hook_tokio::Signals;

#[derive(Debug, Clone, Copy)]
pub enum Signal {
    USR1,
    USR2,
    Custom(i32),
}

/// Starts a thread that listens for provided signals and sends these on the provided channel
pub async fn process_signals(sender: mpsc::Sender<Signal>) {
    let (sigmin, sigmax) = unsafe { (__libc_current_sigrtmin(), __libc_current_sigrtmax()) };
    let mut signals = (sigmin..sigmax).collect::<Vec<_>>();
    signals.push(signal_hook::consts::SIGUSR1);
    signals.push(signal_hook::consts::SIGUSR2);
    let signals = Signals::new(&signals).unwrap();

    // TODO why is is necessary?
    let _handle = signals.handle();

    let mut signals = signals.fuse();
    loop {
        sender
            .send(match signals.next().await.unwrap() {
                signal_hook::consts::SIGUSR1 => Signal::USR1,
                signal_hook::consts::SIGUSR2 => Signal::USR2,
                signal => Signal::Custom(signal - sigmin),
            })
            .await
            .unwrap();
    }
}

//TODO when libc exposes this through their library and even better when the nix crate does we
//should be using that binding rather than a C-binding.
///C bindings to SIGMIN and SIGMAX values
extern "C" {
    fn __libc_current_sigrtmin() -> i32;
    fn __libc_current_sigrtmax() -> i32;
}