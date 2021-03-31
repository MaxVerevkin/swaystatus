#[macro_use]
mod util;
mod blocks;
mod config;
mod errors;
mod formatting;
//mod http;
mod icons;
mod protocol;
mod signals;
mod subprocess;
mod themes;
mod widgets;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};

use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use tokio::sync::mpsc;

use crate::blocks::{run_block, BlockEvent};
use crate::config::Config;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;
use crate::protocol::i3bar_event::process_events;
use crate::signals::{process_signals, Signal};
use crate::util::deserialize_file;
use crate::widgets::text::TextWidget;
use crate::widgets::{I3BarWidget, State};

#[tokio::main]
pub async fn main() {
    let ver = if env!("GIT_COMMIT_HASH").is_empty() || env!("GIT_COMMIT_DATE").is_empty() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        format!(
            "{} (commit {} {})",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_COMMIT_HASH"),
            env!("GIT_COMMIT_DATE")
        )
    };

    let builder = app_from_crate!()
        .version(&*ver)
        .arg(
            Arg::with_name("config")
                .value_name("CONFIG_FILE")
                .help("Sets a toml config file")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::with_name("exit-on-error")
                .help("Exit rather than printing errors to i3bar and continuing")
                .long("exit-on-error")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("never-pause")
                .help("Ignore any attempts by i3 to pause the bar when hidden/fullscreen")
                .long("never-pause")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("one-shot")
                .help("Print blocks once and exit")
                .long("one-shot")
                .takes_value(false)
                .hidden(true),
        )
        .arg(
            Arg::with_name("no-init")
                .help("Do not send an init sequence")
                .long("no-init")
                .takes_value(false)
                .hidden(true),
        );

    let matches = builder.get_matches();
    let exit_on_error = matches.is_present("exit-on-error");

    // Run and match for potential error
    if let Err(error) = run(
        matches.value_of("config").map(String::from),
        matches.is_present("no-init"),
    )
    .await
    {
        if exit_on_error {
            eprintln!("{:?}", error);
            ::std::process::exit(1);
        }

        // Create widget with error message
        let error_widget = TextWidget::new(0, 0, Default::default())
            .with_state(State::Critical)
            .with_text(&format!("{:?}", error));

        // Print errors
        println!("[{}],", error_widget.get_data().render());
        eprintln!("\n\n{:?}", error);

        // Wait for USR2 signal to restart
        signal_hook::iterator::Signals::new(&[signal_hook::consts::SIGUSR2])
            .unwrap()
            .forever()
            .next()
            .unwrap();
        restart();
    }
}

async fn run(config: Option<String>, noinit: bool) -> Result<()> {
    if !noinit {
        // Now we can start to run the i3bar protocol
        protocol::init(false);
    }

    // Read & parse the config file
    let config_path = match config {
        Some(config_path) => std::path::PathBuf::from(config_path),
        None => util::find_file("config.toml", None, None)
            .unwrap_or_else(|| util::xdg_config_home().join("swaystatus/config.toml")),
    };
    let config: Config = deserialize_file(&config_path)?;
    let shared_config = SharedConfig::new(&config);

    // Initialize the blocks
    let mut blocks_events: Vec<mpsc::Sender<BlockEvent>> = Vec::new();
    let mut blocks_tasks = FuturesUnordered::new();
    let (message_sender, mut message_reciever) = mpsc::channel(64);
    for (block_type, block_config) in config.blocks {
        let (events_sender, events_reciever) = mpsc::channel(64);
        blocks_events.push(events_sender);

        let shared_config = shared_config.clone();
        let message_sender = message_sender.clone();
        let id = blocks_tasks.len();

        blocks_tasks.push(tokio::spawn(run_block(
            id,
            block_type,
            block_config,
            shared_config,
            message_sender,
            events_reciever,
        )));
    }

    // TODO first wait for all the blocks to send their widgets and then print
    let mut rendered: Vec<Vec<I3BarBlock>> = blocks_events
        .iter()
        .map(|_| vec![I3BarBlock::default()])
        .collect();

    // Listen to signals and clicks
    let (signals_sender, mut signals_reciever) = mpsc::channel(64);
    let (events_sender, mut events_reciever) = mpsc::channel(64);
    tokio::spawn(process_signals(signals_sender));
    tokio::spawn(process_events(events_sender));

    // Main loop
    loop {
        tokio::select! {
            block_result = blocks_tasks.next() => {
                // TODO remove unwraps
                // Handle blocks' errors
                block_result.unwrap().unwrap()?;
            }
            message = message_reciever.recv() => {
                // Recieve widgets from blocks
                let message = message.unwrap();
                *rendered.get_mut(message.id).internal_error("handle block's message", "failed to get block")? = message.widgets;
                protocol::print_blocks(&rendered, &shared_config)?;
            }
            event = events_reciever.recv() => {
                // Hnadle clicks
                let event = event.unwrap();
                if let Some(id) = event.id {
                    let blocks_event = blocks_events.get(id).unwrap();
                    blocks_event.send(BlockEvent::I3Bar(event)).await.unwrap();
                }
            }
            signal = signals_reciever.recv() => {
                // Handle signals
                match signal.unwrap() {
                    Signal::USR2 => restart(),
                    to_blocks => {
                        for block in &blocks_events {
                            block.send(BlockEvent::Signal(to_blocks)).await.unwrap();
                        }
                    }
                }
            }
        }
    }
}

/// Restart `swaystatus` in-place
fn restart() -> ! {
    use std::env;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStringExt;

    // On linux this line should be OK
    let exe = CString::new(env::current_exe().unwrap().into_os_string().into_vec()).unwrap();

    // Get current arguments
    let mut arg = env::args()
        .map(|a| CString::new(a).unwrap())
        .collect::<Vec<CString>>();

    // Add "--no-init" argument if not already added
    let no_init_arg = CString::new("--no-init").unwrap();
    if !arg.iter().any(|a| *a == no_init_arg) {
        arg.push(no_init_arg);
    }

    // Restart
    nix::unistd::execvp(&exe, &arg).unwrap();
    unreachable!();
}
