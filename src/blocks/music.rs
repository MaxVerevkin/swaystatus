use dbus::arg;
use dbus::message::MatchRule;
use dbus::nonblock;
use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
use dbus::strings::{Interface, Member, Path};
use dbus_tokio::connection;

use futures::StreamExt;
use serde::de::Deserialize;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::util::escape_pango_text;
use crate::widgets::widget::Widget;
use crate::widgets::{Spacing, State};

const PLAY_PAUSE_BTN: usize = 1;
const NEXT_BTN: usize = 2;
const PREV_BTN: usize = 3;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct MusicConfig {
    // TODO add stuff here
    width: usize,

    buttons: Vec<String>,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            width: 10,
            buttons: Vec::new(),
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config = MusicConfig::deserialize(block_config).block_config_error("music")?;

    let mut text = Widget::new(id, shared_config.clone()).with_icon("music")?;
    let mut play_pause_button = Widget::new(id, shared_config.clone())
        .with_instance(PLAY_PAUSE_BTN)
        .with_spacing(Spacing::Hidden);
    let mut next_button = Widget::new(id, shared_config.clone())
        .with_instance(NEXT_BTN)
        .with_spacing(Spacing::Hidden)
        .with_icon("music_next")?;
    let mut prev_button = Widget::new(id, shared_config)
        .with_instance(PREV_BTN)
        .with_spacing(Spacing::Hidden)
        .with_icon("music_prev")?;

    // Connect to the D-Bus session bus (this is blocking, unfortunately).
    let (resource, dbus_conn) =
        connection::new_session_sync().block_error("music", "failed to open DBUS connection")?;
    // The resource is a task that should be spawned onto a tokio compatible
    // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
    tokio::spawn(async {
        let err = resource.await;
        panic!("Lost connection to D-Bus: {}", err);
    });

    // Add matches
    // TODO (maybe?) listen to "owner changed" events
    let mut dbus_rule = MatchRule::new();
    dbus_rule.interface = Some(Interface::from_slice("org.freedesktop.DBus.Properties").unwrap());
    dbus_rule.member = Some(Member::new("PropertiesChanged").unwrap());
    dbus_rule.path = Some(Path::new("/org/mpris/MediaPlayer2").unwrap());
    let (_incoming_signal, mut dbus_stream) = dbus_conn
        .add_match(dbus_rule)
        .await
        .block_error("music", "failed to add match")?
        .msg_stream();

    let mut player = get_any_player(dbus_conn.as_ref()).await?;

    loop {
        let widgets = match player {
            Some(ref player) => {
                text.set_text(escape_pango_text(player.display(block_config.width)));

                match player.status {
                    PlaybackStatus::Playing => {
                        text.set_state(State::Info);
                        play_pause_button.set_state(State::Info);
                        next_button.set_state(State::Info);
                        prev_button.set_state(State::Info);
                        play_pause_button.set_icon("music_pause")?;
                    }
                    _ => {
                        text.set_state(State::Idle);
                        play_pause_button.set_state(State::Idle);
                        next_button.set_state(State::Idle);
                        prev_button.set_state(State::Idle);
                        play_pause_button.set_icon("music_play")?;
                    }
                }

                let mut output = vec![text.get_data()];
                for button in &block_config.buttons {
                    match button.as_str() {
                        "play" => output.push(play_pause_button.get_data()),
                        "next" => output.push(next_button.get_data()),
                        "prev" => output.push(prev_button.get_data()),
                        _ => (),
                    }
                }
                output
            }
            None => {
                text.set_text(String::new());
                text.set_state(State::Idle);
                vec![text.get_data()]
            }
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("music", "failed to send message")?;

        if let Some(ref mut player) = player {
            player.rotating.rotate();
        }

        tokio::select! {
            // Time to update rotating text
            _ = tokio::time::sleep(Duration::from_secs(1)) => (),
            // Wait for a DBUS event
            _ = dbus_stream.next() => player = get_any_player(&dbus_conn).await?,
            // Wait for a click
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
                if click.button == MouseButton::Left {
                    if let (Some(PLAY_PAUSE_BTN), Some(player)) = (click.instance, &player) {
                            let proxy = nonblock::Proxy::new(&player.name, "/org/mpris/MediaPlayer2", Duration::from_secs(2), dbus_conn.clone());
                            let _resonce: () = proxy.method_call("org.mpris.MediaPlayer2.Player", "PlayPause", ()).await.block_error("music", "failed to call pause/play")?;
                    }
                }
            }
        }
    }
}

async fn get_any_player(dbus_conn: &nonblock::SyncConnection) -> Result<Option<Player>> {
    // Get already oppened players
    let dbus_proxy = nonblock::Proxy::new(
        "org.freedesktop.DBus",
        "/",
        Duration::from_secs(2),
        dbus_conn,
    );
    let (names,): (Vec<String>,) = dbus_proxy
        .method_call("org.freedesktop.DBus", "ListNames", ())
        .await
        .block_error("music", "failed to execute 'ListNames'")?;

    // Get all the players with a name that starts with "org.mpris.MediaPlayer2"
    let names = names
        .into_iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2"));

    // Try each name
    for name in names {
        let bus_name = dbus_proxy
            .method_call("org.freedesktop.DBus", "GetNameOwner", (&name,))
            .await;
        if let Ok((bus_name,)) = bus_name {
            return Ok(Some(Player::new(dbus_conn, name, bus_name).await));
        }
    }

    // Couldn't find anything
    Ok(None)
}

#[derive(Debug)]
struct Player {
    name: String,
    bus_name: String,
    status: PlaybackStatus,
    title: Option<String>,
    artist: Option<String>,
    rotating: RotatingText,
}

impl Player {
    async fn new(dbus_conn: &nonblock::SyncConnection, name: String, bus_name: String) -> Self {
        let proxy = nonblock::Proxy::new(
            &bus_name,
            "/org/mpris/MediaPlayer2",
            Duration::from_secs(2),
            dbus_conn,
        );

        let status = proxy
            .get::<String>("org.mpris.MediaPlayer2.Player", "PlaybackStatus")
            .await;
        let status = match status.as_deref() {
            Ok("Playing") => PlaybackStatus::Playing,
            Ok("Paused") => PlaybackStatus::Paused,
            Ok("Stopped") => PlaybackStatus::Stopped,
            _ => PlaybackStatus::Unknown,
        };

        let metadata = proxy
            .get::<arg::PropMap>("org.mpris.MediaPlayer2.Player", "Metadata")
            .await;

        let (title, artist) = match metadata {
            Ok(metadata) => {
                let title: Option<&String> = arg::prop_cast(&metadata, "xesam:title");
                let artist: Option<&Vec<String>> = arg::prop_cast(&metadata, "xesam:artist");
                let artist = artist.map(|a| a.first()).flatten();
                (title.cloned(), artist.cloned())
            }
            _ => (None, None),
        };

        Self {
            rotating: RotatingText::new(match (title.as_deref(), artist.as_deref()) {
                (Some(t), Some(a)) => format!("{}|{}|", t, a),
                (None, Some(s)) | (Some(s), None) => format!("{}|", s),
                _ => "".to_string(),
            }),
            name,
            bus_name,
            status,
            title,
            artist,
        }
    }

    fn display(&self, len: usize) -> String {
        self.rotating.display(len)
    }
}

#[derive(Debug, Clone, Copy)]
enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
    Unknown,
}

// TODO move to util.rs or somewhere else
#[derive(Debug)]
pub struct RotatingText(VecDeque<char>);
impl RotatingText {
    pub fn new(text: String) -> Self {
        Self(text.chars().collect())
    }

    pub fn display(&self, len: usize) -> String {
        self.0.iter().cycle().take(len).collect()
    }

    pub fn rotate(&mut self) {
        if let Some(c) = self.0.pop_front() {
            self.0.push_back(c);
        }
    }
}
