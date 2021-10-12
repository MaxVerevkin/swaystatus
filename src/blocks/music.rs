use futures::StreamExt;

use zbus::fdo::DBusProxy;
use zbus::zvariant::{Optional, OwnedValue};
use zbus::MessageStream;
use zbus_names::{OwnedBusName, OwnedInterfaceName};
use zvariant::derive::Type;

use std::collections::{HashMap, VecDeque};
use std::convert::TryFrom;
use std::time::Duration;

use super::prelude::*;
use crate::util::escape_pango_text;

mod zbus_mpris;

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

#[derive(Debug, Clone, Type, serde_derive::Deserialize)]
struct PropChange {
    _interface_name: OwnedInterfaceName,
    changed_properties: HashMap<String, OwnedValue>,
    _invalidated_properties: Vec<String>,
}

#[derive(Debug, Clone, Type, serde_derive::Deserialize)]
struct OwnerChange {
    pub name: OwnedBusName,
    pub old_owner: Optional<String>,
    pub new_owner: Optional<String>,
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, events: EventsRxGetter) -> BlockHandle {
    let mut events = events();
    tokio::spawn(async move {
        let block_config = MusicConfig::deserialize(block_config).config_error()?;
        let dbus_conn = api.dbus_connection().await?;

        let mut text = api.new_widget().with_icon("music")?;
        let mut play_pause_button = api
            .new_widget()
            .with_instance(PLAY_PAUSE_BTN)
            .with_spacing(WidgetSpacing::Hidden);
        let mut next_button = api
            .new_widget()
            .with_instance(NEXT_BTN)
            .with_spacing(WidgetSpacing::Hidden)
            .with_icon("music_next")?;
        let mut prev_button = api
            .new_widget()
            .with_instance(PREV_BTN)
            .with_spacing(WidgetSpacing::Hidden)
            .with_icon("music_prev")?;

        let mut players = get_all_players(&dbus_conn).await?;
        let mut cur_player = None;
        for (i, player) in players.iter().enumerate() {
            cur_player = Some(i);
            if player.status == Some(PlaybackStatus::Playing) {
                break;
            }
        }

        let dbus_proxy = DBusProxy::new(&dbus_conn)
            .await
            .error( "failed to cerate DBusProxy")?;
        dbus_proxy.add_match("type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'")
            .await
            .error( "failed to add match")?;
        dbus_proxy.add_match("type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'")
            .await
            .error( "failed to add match")?;
        let mut dbus_stream = MessageStream::from(&dbus_conn);

        loop {
            let mut player = cur_player.map(|c| players.get_mut(c).unwrap());
            let widgets = match player {
                Some(ref player) => {
                    text.set_full_text(escape_pango_text(
                        player.rotating.display(block_config.width),
                    ));

                    match player.status {
                        Some(PlaybackStatus::Playing) => {
                            text.set_state(WidgetState::Info);
                            play_pause_button.set_state(WidgetState::Info);
                            next_button.set_state(WidgetState::Info);
                            prev_button.set_state(WidgetState::Info);
                            play_pause_button.set_icon("music_pause")?;
                        }
                        _ => {
                            text.set_state(WidgetState::Idle);
                            play_pause_button.set_state(WidgetState::Idle);
                            next_button.set_state(WidgetState::Idle);
                            prev_button.set_state(WidgetState::Idle);
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
                    text.set_text((String::new(), None));
                    text.set_state(WidgetState::Idle);
                    vec![text.get_data()]
                }
            };

            api.send_widgets(widgets).await?;

            if let Some(ref mut player) = player {
                player.rotating.rotate();
            }

            tokio::select! {
                // Time to update rotating text
                _ = tokio::time::sleep(Duration::from_secs(1)) => (),
                // Wait for a DBUS event
                Some(msg) = dbus_stream.next() => {
                    let msg = msg.unwrap();
                    match msg.member().unwrap().as_ref().map(|m| m.as_str()) {
                        Some("PropertiesChanged") => {
                            let header = msg.header().unwrap();
                            let sender = header.sender().unwrap().unwrap();
                            let player = players.iter_mut().find(|p| p.owner == sender.to_string()).unwrap();

                            let body: PropChange = msg.body_unchecked().unwrap();
                            let props = body.changed_properties;

                            if let Some(status) = props.get("PlaybackStatus") {
                                let status: &str = status.downcast_ref().unwrap();
                                player.status = PlaybackStatus::from_str(status);
                            }
                            if let Some(metadata) = props.get("Metadata") {
                                let metadata =
                                    zbus_mpris::PlayerMetadata::try_from(metadata.clone()).unwrap();
                                player.update_metadata(metadata);
                            }
                        }
                        Some("NameOwnerChanged") => {
                            let body: OwnerChange = msg.body_unchecked().unwrap();
                            dbg!(&body);
                            let old: Option<String> = body.old_owner.into();
                            let new: Option<String> = body.new_owner.into();
                            match (old, new) {
                                (None, Some(new)) => if new != body.name.to_string() {
                                    players.push(Player::new(&dbus_conn, body.name, new).await?);
                                    cur_player = Some(players.len() - 1);
                                }
                                (Some(old), None) => {
                                    if let Some(pos) = players.iter().position(|p| p.owner == old) {
                                        players.remove(pos);
                                        if let Some(cur) = cur_player {
                                            if players.is_empty() {
                                                cur_player = None;
                                            } else if pos == cur {
                                                cur_player = Some(0);
                                            } else if pos < cur {
                                                cur_player = Some(cur - 1);
                                            }
                                        }
                                    }
                                }
                                _ => (),
                            }
                        }
                        _ => (),
                    }
                }
                // Wait for a click
                Some(BlockEvent::I3Bar(click)) = events.recv() => {
                    match click.button {
                        MouseButton::Left => {
                            if let Some(ref player) = player {
                                match click.instance {
                                    Some(PLAY_PAUSE_BTN) => player.play_pause().await?,
                                    Some(NEXT_BTN) => player.next().await?,
                                    Some(PREV_BTN) => player.prev().await?,
                                    _ => (),
                                }
                            }
                        }
                        MouseButton::WheelUp => {
                            if let Some(cur) = cur_player {
                                if cur > 0 {
                                    cur_player = Some(cur - 1);
                                }
                            }
                        }
                        MouseButton::WheelDown => {
                            if let Some(cur) = cur_player {
                                if cur + 1 < players.len() {
                                    cur_player = Some(cur + 1);
                                }
                            }
                        }
                        _ => (),
                    }
                }
            }
        }
    })
}

async fn get_all_players(dbus_conn: &zbus::Connection) -> Result<Vec<Player<'_>>> {
    let proxy = DBusProxy::new(dbus_conn)
        .await
        .error( "failed to create DBusProxy")?;
    let names = proxy
        .list_names()
        .await
        .error( "failed to list dbus names")?;

    let mut players = Vec::new();
    for name in names {
        if name.starts_with("org.mpris.MediaPlayer2") {
            let owner = proxy
                .get_name_owner(name.as_ref())
                .await
                .unwrap()
                .to_string();
            players.push(Player::new(dbus_conn, name, owner).await?);
        }
    }
    Ok(players)
}

#[derive(Debug)]
struct Player<'a> {
    status: Option<PlaybackStatus>,
    owner: String,
    player_proxy: zbus_mpris::PlayerProxy<'a>,
    rotating: RotatingText,
}

impl<'a> Player<'a> {
    async fn new(
        dbus_conn: &'a zbus::Connection,
        bus_name: OwnedBusName,
        owner: String,
    ) -> Result<Player<'a>> {
        let proxy = zbus_mpris::PlayerProxy::builder(dbus_conn)
            .destination(bus_name.clone())
            .error( "failed to set proxy destination")?
            .build()
            .await
            .error( "failed to open player proxy")?;
        let metadata = proxy
            .metadata()
            .await
            .error( "failed to obtain player metadata")?;
        let status = proxy
            .playback_status()
            .await
            .error( "failed to obtain player status")?;

        Ok(Self {
            status: PlaybackStatus::from_str(&status),
            owner,
            player_proxy: proxy,
            rotating: RotatingText::from_metadata(metadata),
        })
    }

    fn update_metadata(&mut self, metadata: zbus_mpris::PlayerMetadata) {
        self.rotating = RotatingText::from_metadata(metadata);
    }

    async fn play_pause(&self) -> Result<()> {
        self.player_proxy
            .play_pause()
            .await
            .error( "play_pause() failed")
    }

    async fn prev(&self) -> Result<()> {
        self.player_proxy
            .previous()
            .await
            .error( "prev() failed")
    }

    async fn next(&self) -> Result<()> {
        self.player_proxy
            .next()
            .await
            .error( "next() failed")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Paused" => Some(Self::Paused),
            "Playing" => Some(Self::Playing),
            "Stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

// TODO move to util.rs or somewhere else
#[derive(Debug)]
pub struct RotatingText(VecDeque<char>);
impl RotatingText {
    pub fn from_metadata(metadata: zbus_mpris::PlayerMetadata) -> Self {
        let title = metadata.title();
        let artist = metadata.artist();
        Self::new(match (title, artist.as_deref()) {
            (Some(t), Some(a)) => format!("{}|{}|", t, a),
            (None, Some(s)) | (Some(s), None) => format!("{}|", s),
            _ => "".to_string(),
        })
    }

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
