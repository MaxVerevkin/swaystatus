#[macro_use]
mod util;
mod blocks;
mod click;
mod config;
mod de;
mod errors;
mod escape;
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
use futures::Future;
use futures::TryFutureExt;
use protocol::i3bar_event::I3BarEvent;
use smallvec::SmallVec;
use smartstring::alias::String;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinError;

use blocks::{BlockEvent, BlockType, CommonApi, CommonConfig};
use click::ClickHandler;
use config::Config;
use config::SharedConfig;
use errors::*;
use formatting::{value::Value, Format};
use protocol::i3bar_event::process_events;
use signals::{process_signals, Signal};
use util::deserialize_file;
use widget::{Widget, WidgetState};

const DBUS_WELL_KNOWN_NAME: &str = "rs.swaystatus";

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
                    .with_full_text(error.to_string().into());

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
        .error("Configuration file not found")?;
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

type BlockFuture = dyn Future<Output = std::result::Result<Result<()>, JoinError>>;

pub struct Block {
    block_type: BlockType,

    event_sender: Option<mpsc::Sender<BlockEvent>>,
    click_handler: ClickHandler,

    hidden: bool,
    collapsed: bool,
    widget: Widget,
    buttons: Vec<Widget>,

    values: HashMap<String, Value>,
    format: Option<Arc<Format>>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub block_id: usize,
    pub cmds: SmallVec<[RequestCmd; 4]>,
}

#[derive(Debug, Clone)]
pub enum RequestCmd {
    Hide,
    Collapse,
    Show,

    SetIcon(String),
    SetState(WidgetState),
    SetText((String, Option<String>)),
    SetValues(HashMap<String, Value>),
    SetFormat(Arc<Format>),

    AddButton(usize, String),
    SetButton(usize, String),

    Render,
}

pub struct Swaystatus {
    pub shared_config: SharedConfig,

    pub blocks: Vec<Block>,
    pub spawned_blocks: FuturesUnordered<Pin<Box<BlockFuture>>>,

    pub request_sender: mpsc::Sender<Request>,
    pub request_receiver: mpsc::Receiver<Request>,

    pub dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
    pub system_dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
}

impl Swaystatus {
    pub fn new(shared_config: SharedConfig) -> Self {
        let (request_sender, request_receiver) = mpsc::channel(64);
        Self {
            shared_config,

            blocks: Vec::new(),
            spawned_blocks: FuturesUnordered::new(),

            request_sender,
            request_receiver,

            dbus_connection: Arc::new(async_lock::Mutex::new(None)),
            system_dbus_connection: Arc::new(async_lock::Mutex::new(None)),
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
            id: self.blocks.len(),
            shared_config,

            request_sender: self.request_sender.clone(),
            cmd_buf: SmallVec::new(),

            dbus_connection: Arc::clone(&self.dbus_connection),
            system_dbus_connection: Arc::clone(&self.system_dbus_connection),
        };

        let mut block = Block {
            block_type,

            event_sender: None,
            click_handler: common_config.click,

            hidden: false,
            collapsed: false,
            widget: Widget::new(api.id, api.shared_config.clone()),
            buttons: Vec::new(),

            values: HashMap::new(),
            format: None,
        };

        let handle = blocks::block_spawner(block_type)(block_config, api, &mut || {
            let (sender, receiver) = mpsc::channel(64);
            block.event_sender = Some(sender);
            receiver
        });
        let handle = handle.and_then(move |r| async move { Ok(r.in_block(block_type)) });

        self.spawned_blocks.push(Box::pin(handle));
        self.blocks.push(block);
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
                    block_result.error("Error handler: Failed to read block exit status")??;
                },
                // Recieve messages from blocks
                Some(request) = self.request_receiver.recv() => {
                    let block = self.blocks.get_mut(request.block_id).error("Message receiver: ID out of bounds")?;
                    for cmd in request.cmds {
                        match cmd {
                            RequestCmd::Hide => block.hidden = true,
                            RequestCmd::Collapse => block.collapsed = true,
                            RequestCmd::Show => {
                                block.hidden = false;
                                block.collapsed = false;
                            }
                            RequestCmd::SetIcon(icon) => block.widget.icon = icon,
                            RequestCmd::SetText(text) => block.widget.set_text(text),
                            RequestCmd::SetState(state) => {
                                block.widget.set_state(state);
                                for b in &mut block.buttons {
                                    b.set_state(state);
                                }
                            }
                            RequestCmd::SetValues(values) => block.values = values,
                            RequestCmd::SetFormat(format) => block.format = Some(format),
                            RequestCmd::AddButton(instance, icon) => block.buttons.push(
                                Widget::new(request.block_id, block.widget.shared_config.clone())
                                    .with_instance(instance)
                                    .with_icon_str(icon)
                            ),
                            RequestCmd::SetButton(instance, icon) => {
                                for b in &mut block.buttons {
                                    if b.instance == Some(instance) {
                                        b.icon = icon.clone();
                                    }
                                }
                            }
                            RequestCmd::Render => {
                                if let Some(format) = &block.format {
                                    block.widget.set_text(
                                        format
                                            .render(&block.values)
                                            .in_block(block.block_type)?
                                    );
                                }
                            }
                        }
                    }

                    // TODO: cache
                    let mut vec = Vec::new();
                    for b in &mut self.blocks {
                        if !b.hidden {
                            let mut v = Vec::new();
                            if b.collapsed {
                                b.widget.set_text((String::new(), None));
                                v.push(b.widget.get_data());
                            } else {
                                v.push(b.widget.get_data());
                                for button in &b.buttons {
                                    v.push(button.get_data());
                                }
                            }
                            vec.push(v);
                        }
                    }
                    protocol::print_blocks(&vec, &self.shared_config)?;
                }
                // Handle clicks
                Some(event) = events_receiver.recv() => {
                    let block = self.blocks.get(event.id).error("Events receiver: ID out of bounds")?;
                    if block.click_handler.handle(event.button).await.in_block(block.block_type)? {
                        if let Some(sender) = &block.event_sender {
                            sender.send(BlockEvent::Click(event)).await.error("Failed to send event to block")?;
                        }
                    }
                }
                // Handle signals
                Some(signal) = signals_receiver.recv() => match signal {
                    Signal::Usr2 => restart(),
                    signal => {
                        for block in &self.blocks {
                            if let Some(sender) = &block.event_sender {
                                sender.send(BlockEvent::Signal(signal)).await.error("Failed to send signal to block")?;
                            }
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
