use dbus::arg;
use dbus::message::MatchRule;
use dbus::nonblock;
use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
use dbus::strings::{Interface, Member, Path};
use dbus_tokio::connection;

use futures::StreamExt;
use serde::de::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_event::MouseButton;
use crate::widgets::rotatingtext::RotatingTextWidget;
use crate::widgets::text::TextWidget;
use crate::widgets::{I3BarWidget, Spacing, State};

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct MusicConfig {
    // TODO add stuff here
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {}
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    const PLAY_PAUSE_BTN: usize = 1;

    let _block_config = MusicConfig::deserialize(block_config).block_config_error("time")?;

    let mut text = RotatingTextWidget::new(
        id,
        0,
        Duration::from_secs(1),
        Duration::from_secs(1),
        20,
        false,
        shared_config.clone(),
    )
    .with_icon("music")?
    .with_state(State::Info);
    let mut play_pause_button = TextWidget::new(id, PLAY_PAUSE_BTN, shared_config.clone())
        .with_state(State::Info)
        .with_spacing(Spacing::Hidden);

    // Connect to the D-Bus session bus (this is blocking, unfortunately).
    let (resource, dbus_conn) =
        connection::new_session_local().block_error("music", "failed to open DBUS connection")?;
    // The resource is a task that should be spawned onto a tokio compatible
    // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
    tokio::task::spawn_local(async {
        let err = resource.await;
        panic!("Lost connection to D-Bus: {}", err);
    });

    // Add matches
    let mut rule1 = MatchRule::new();
    rule1.interface = Some(Interface::from_slice("org.freedesktop.DBus.Properties").unwrap());
    rule1.member = Some(Member::new("PropertiesChanged").unwrap());
    rule1.path = Some(Path::new("/org/mpris/MediaPlayer2").unwrap());
    let (_incoming_signal1, mut stream1) = dbus_conn
        .add_match(rule1)
        .await
        .block_error("music", "failed to add match")?
        .msg_stream();

    let mut player = get_any_player(dbus_conn.as_ref()).await?;

    loop {
        let widgets = match player {
            Some(ref player) => {
                let mut output = String::new();
                player.title.as_deref().map(|t| output.push_str(t));
                output.push('|');
                player.artist.as_deref().map(|a| output.push_str(a));
                text.set_text(output);
                match player.status {
                    PlaybackStatus::Paused => {
                        play_pause_button.set_icon("music_play")?;
                        vec![text.get_data(), play_pause_button.get_data()]
                    }
                    PlaybackStatus::Playing => {
                        play_pause_button.set_icon("music_pause")?;
                        vec![text.get_data(), play_pause_button.get_data()]
                    }
                    _ => vec![text.get_data()],
                }
            }
            None => {
                text.set_text(String::new());
                vec![text.get_data()]
            }
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("music", "failed to send message")?;

        text.next()?;

        tokio::select! {
            // Time to update rotating text
            //_ = tokio::time::sleep(Duration::from_secs(1)) => {text.next()?;},
            _ = tokio::time::sleep(Duration::from_secs(1)) => (),
            // Wait for a DBUS event
            _ = stream1.next() => player = get_any_player(&dbus_conn).await?,
            // Wait for a click
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
                if click.button == MouseButton::Left {
                    match (click.instance, &player) {
                        (Some(PLAY_PAUSE_BTN), Some(player)) => {
                            let proxy = nonblock::Proxy::new(&player.name, "/org/mpris/MediaPlayer2", Duration::from_secs(2), dbus_conn.clone());
                            let _resonce: () = proxy.method_call("org.mpris.MediaPlayer2.Player", "PlayPause", ()).await.block_error("music", "failed to call pause/play")?;
                        },
                        _ => (),
                    }
                }
            }
        }
    }
}

async fn get_any_player(dbus_conn: &nonblock::LocalConnection) -> Result<Option<Player>> {
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
        let bus_name: Option<(String,)> = dbus_proxy
            .method_call("org.freedesktop.DBus", "GetNameOwner", (&name,))
            .await
            .ok();
        let bus_name = match bus_name {
            Some((bus_name,)) => bus_name,
            None => continue,
        };
        return Ok(Some(Player::new(&dbus_conn, name, bus_name).await));
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
}

impl Player {
    async fn new(dbus_conn: &nonblock::LocalConnection, name: String, bus_name: String) -> Self {
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
            name,
            bus_name,
            status,
            title,
            artist,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
    Unknown,
}
