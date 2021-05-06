use futures::stream::StreamExt;
use signal_hook::consts;
use signal_hook_tokio::Signals;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy)]
pub enum Signal {
    Usr1,
    Usr2,
    Custom(i32),
}

/// Starts a thread that listens for provided signals and sends these on the provided channel
pub async fn process_signals(sender: mpsc::Sender<Signal>) {
    let (sigmin, sigmax) = unsafe { (__libc_current_sigrtmin(), __libc_current_sigrtmax()) };

    let mut signals: Vec<i32> = (sigmin..sigmax).collect();
    signals.push(consts::SIGUSR1);
    signals.push(consts::SIGUSR2);

    let signals = Signals::new(&signals).unwrap();
    let mut signals = signals.fuse();

    loop {
        sender
            .send(match signals.next().await.unwrap() {
                signal_hook::consts::SIGUSR1 => Signal::Usr1,
                signal_hook::consts::SIGUSR2 => Signal::Usr2,
                x => Signal::Custom(x - sigmin),
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
