#[macro_use]
mod util;
mod blocks;
mod click;
mod config;
mod de;
mod errors;
mod formatting;
mod icons;
mod netlink;
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
use crate::widgets::widget::Widget;
use crate::widgets::State;

fn main() {
    let args = app_from_crate!()
        .version(env!("VERSION"))
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
        )
        .get_matches();

    // Build the runtime adn run the program
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(
            args.value_of("config").map(String::from),
            args.is_present("no-init"),
            args.is_present("never_pause"),
        ));

    // Match for potential error
    if let Err(error) = result {
        if args.is_present("exit-on-error") {
            eprintln!("{:?}", error);
            std::process::exit(1);
        }

        // Create widget with error message
        let error_widget = Widget::new(0, Default::default())
            .with_state(State::Critical)
            .with_full_text(error.to_string());

        // Print errors
        println!("[{}],", error_widget.get_data().render());
        eprintln!("\n\n{}\n\n", error);
        dbg!(error);

        // Wait for USR2 signal to restart
        signal_hook::iterator::Signals::new(&[signal_hook::consts::SIGUSR2])
            .unwrap()
            .forever()
            .next()
            .unwrap();
        restart();
    }
}

async fn run(config: Option<String>, noinit: bool, never_pause: bool) -> Result<()> {
    if !noinit {
        // Now we can start to run the i3bar protocol
        protocol::init(never_pause);
    }

    // Read & parse the config file
    let config = config.unwrap_or_else(|| "config.toml".to_string());
    let config_path = util::find_file(&config, None, Some("toml"))
        .internal_error("run()", "configuration file not found")?;

    let config: Config = deserialize_file(&config_path)?;
    let shared_config = SharedConfig::new(&config);

    // Initialize the blocks
    let mut blocks_events: Vec<mpsc::Sender<BlockEvent>> = Vec::new();
    let mut blocks_tasks = FuturesUnordered::new();
    let (message_sender, mut message_receiver) = mpsc::channel(64);

    for (block_type, block_config) in config.blocks {
        let (events_sender, events_reciever) = mpsc::channel(64);
        blocks_events.push(events_sender);

        blocks_tasks.push(tokio::spawn(run_block(
            blocks_tasks.len(),
            block_type,
            block_config,
            shared_config.clone(),
            message_sender.clone(),
            events_reciever,
        )));
    }

    // Listen to signals and clicks
    let (signals_sender, mut signals_receiver) = mpsc::channel(64);
    let (events_sender, mut events_receiver) = mpsc::channel(64);
    tokio::spawn(process_signals(signals_sender));
    tokio::spawn(process_events(events_sender, config.invert_scrolling));

    // Main loop
    let mut rendered = vec![Vec::<I3BarBlock>::new(); blocks_events.len()];
    loop {
        tokio::select! {
            // Handle blocks' errors
            Some(block_result) = blocks_tasks.next() => block_result.unwrap()?,
            // Recieve widgets from blocks
            Some(message) = message_receiver.recv() => {
                *rendered.get_mut(message.id).internal_error("handle block's message", "failed to get block")? = message.widgets;
                protocol::print_blocks(&rendered, &shared_config)?;
            }
            // Handle clicks
            Some(event) = events_receiver.recv() => {
                let blocks_event = blocks_events.get(event.id).unwrap();
                blocks_event.send(BlockEvent::I3Bar(event)).await.unwrap();
            }
            // Handle signals
            Some(signal) = signals_receiver.recv() => match signal {
                Signal::Usr2 => restart(),
                signal => {
                    for block in &blocks_events {
                        block.send(BlockEvent::Signal(signal)).await.unwrap();
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
