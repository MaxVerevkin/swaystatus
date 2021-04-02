use futures::stream::StreamExt;
use signal_hook::consts;
use signal_hook_tokio::Signals;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy)]
pub enum Signal {
    Usr1,
    Usr2,
}

/// Starts a thread that listens for provided signals and sends these on the provided channel
pub async fn process_signals(sender: mpsc::Sender<Signal>) {
    const SIGNALS: [i32; 2] = [consts::SIGUSR1, consts::SIGUSR2];
    let signals = Signals::new(&SIGNALS).unwrap();
    let mut signals = signals.fuse();

    loop {
        sender
            .send(match signals.next().await.unwrap() {
                signal_hook::consts::SIGUSR1 => Signal::Usr1,
                signal_hook::consts::SIGUSR2 => Signal::Usr2,
                _ => unreachable!(),
            })
            .await
            .unwrap();
    }
}
