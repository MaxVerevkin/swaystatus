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
mod widget;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use protocol::i3bar_event::I3BarEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use blocks::prelude::*;
use blocks::CommonConfig;
use click::ClickHandler;
use config::Config;
use config::SharedConfig;
use protocol::i3bar_block::I3BarBlock;
use protocol::i3bar_event::process_events;
use signals::{process_signals, Signal};
use util::deserialize_file;

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
            Arg::with_name("no-init")
                .help("Do not send an init sequence")
                .long("no-init")
                .takes_value(false)
                .hidden(true),
        )
        .get_matches();

    // Build the runtime and run the program
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            if let Err(error) = run(
                args.value_of("config"),
                args.is_present("no-init"),
                args.is_present("never_pause"),
            )
            .await
            {
                if args.is_present("exit-on-error") {
                    eprintln!("{:?}", error);
                    std::process::exit(1);
                }

                // Create widget with error message
                let error_widget = Widget::new(0, Default::default())
                    .with_state(WidgetState::Critical)
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
        });
}

async fn run(config: Option<&str>, noinit: bool, never_pause: bool) -> Result<()> {
    if !noinit {
        // Now we can start to run the i3bar protocol
        protocol::init(never_pause);
    }

    // Read & parse the config file
    let config_path = util::find_file(config.unwrap_or("config.toml"), None, Some("toml"))
        .internal_error("run()", "configuration file not found")?;
    let config: Config = deserialize_file(&config_path)?;
    let (shared_config, block_list, invert_scrolling) = config.into_parts();

    // Spawn blocks
    let mut swaystatus = Swaystatus::new(shared_config);
    for (block_type, block_config) in block_list {
        swaystatus.spawn_block(block_type, block_config)?;
    }

    // Listen to signals and clicks
    let (signals_sender, signals_receiver) = mpsc::channel(64);
    let (events_sender, events_receiver) = mpsc::channel(64);
    tokio::spawn(process_signals(signals_sender));
    tokio::spawn(process_events(events_sender, invert_scrolling));

    // Main loop
    swaystatus
        .run_event_loop(signals_receiver, events_receiver)
        .await
}

pub struct Swaystatus {
    pub blocks_cnt: usize,
    pub shared_config: SharedConfig,

    pub spawned_blocks: FuturesUnordered<BlockHandle>,
    pub block_event_sentders: HashMap<usize, mpsc::Sender<BlockEvent>>,
    pub rendered_blocks: Vec<Vec<I3BarBlock>>,
    pub block_click_handlers: Vec<ClickHandler>,

    pub message_sender: mpsc::Sender<BlockMessage>,
    pub message_receiver: mpsc::Receiver<BlockMessage>,

    pub dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
}

impl Swaystatus {
    pub fn new(shared_config: SharedConfig) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(64);
        Self {
            blocks_cnt: 0,
            shared_config,

            spawned_blocks: FuturesUnordered::new(),
            block_event_sentders: HashMap::new(),
            rendered_blocks: Vec::new(),
            block_click_handlers: Vec::new(),

            message_sender,
            message_receiver,

            dbus_connection: Arc::new(async_lock::Mutex::new(None)),
        }
    }

    pub fn spawn_block(
        &mut self,
        block_type: BlockType,
        mut block_config: toml::Value,
    ) -> Result<()> {
        let common_config = CommonConfig::new(&mut block_config)?;
        let mut shared_config = self.shared_config.clone();

        // Overrides
        if let Some(icons_format) = common_config.icons_format {
            *Arc::make_mut(&mut shared_config.icons_format) = icons_format;
        }
        if let Some(theme_overrides) = common_config.theme_overrides {
            Arc::make_mut(&mut shared_config.theme).apply_overrides(&theme_overrides)?;
        }

        let api = CommonApi {
            id: self.blocks_cnt,
            block_name: blocks::block_name(block_type),
            shared_config,
            message_sender: self.message_sender.clone(),
            dbus_connection: Arc::clone(&self.dbus_connection),
        };

        let handle = blocks::block_spawner(block_type)(block_config, api, &mut || {
            let (sender, receiver) = mpsc::channel(64);
            self.block_event_sentders.insert(self.blocks_cnt, sender);
            receiver
        });

        self.spawned_blocks.push(handle);
        self.block_click_handlers.push(common_config.click);
        self.rendered_blocks.push(Vec::new());
        self.blocks_cnt += 1;
        Ok(())
    }

    pub async fn run_event_loop(
        mut self,
        mut signals_receiver: mpsc::Receiver<Signal>,
        mut events_receiver: mpsc::Receiver<I3BarEvent>,
    ) -> Result<()> {
        loop {
            tokio::select! {
                // Handle blocks' errors
                Some(block_result) = self.spawned_blocks.next() => {
                    block_result.internal_error("error handler", "failed to read block exit status")??;
                },
                // Recieve widgets from blocks
                Some(message) = self.message_receiver.recv() => {
                    *self.rendered_blocks
                        .get_mut(message.id)
                        .internal_error("handle block's message", "failed to get block")?
                            = message.widgets;
                    protocol::print_blocks(&self.rendered_blocks, &self.shared_config)?;
                }
                // Handle clicks
                Some(event) = events_receiver.recv() => {
                    let update = self.block_click_handlers.get(event.id).unwrap().handle(event.button).await;
                    if update {
                        if let Some(sender) = self.block_event_sentders.get(&event.id) {
                            sender.send(BlockEvent::I3Bar(event)).await.unwrap();
                        }
                    }
                }
                // Handle signals
                Some(signal) = signals_receiver.recv() => match signal {
                    Signal::Usr2 => restart(),
                    signal => {
                        for sender in self.block_event_sentders.values() {
                            sender.send(BlockEvent::Signal(signal)).await.unwrap();
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
    let mut arg: Vec<CString> = env::args().map(|a| CString::new(a).unwrap()).collect();

    // Add "--no-init" argument if not already added
    let no_init_arg = CString::new("--no-init").unwrap();
    if !arg.iter().any(|a| *a == no_init_arg) {
        arg.push(no_init_arg);
    }

    // Restart
    nix::unistd::execvp(&exe, &arg).unwrap();
    unreachable!();
}
