//! A block controled by the DBus
//!
//! This block creates a new DBus object in `rs.swaystatus` service. This object implements
//! `rs.swaystatus.dbus` interface which allows you to set block's icon, text and state.
//!
//! Output of `busctl --user introspect rs.swaystatus /<path> rs.swaystatus.dbus`:
//! ```text
//! NAME                                TYPE      SIGNATURE RESULT/VALUE FLAGS
//! rs.swaystatus.dbus                  interface -         -            -
//! .SetIcon                            method    s         s            -
//! .SetState                           method    s         s            -
//! .SetText                            method    ss        s            -
//! ```
//!
//! # Example
//!
//! Config:
//! ```toml
//! [[block]]
//! block = "custom_dbus"
//! path = "/my_path"
//! ```
//!
//! Useage:
//! ```sh
//! # set full text to 'hello' and short text to 'hi'
//! busctl --user call rs.swaystatus /my_path rs.swaystatus.dbus SetText ss hello hi
//! # set icon to 'music'
//! busctl --user call rs.swaystatus /my_path rs.swaystatus.dbus SetIcon s music
//! # set state to 'good'
//! busctl --user call rs.swaystatus /my_path rs.swaystatus.dbus SetState s good
//! ```
//!
//! # TODO
//! - Send a signal on click?

use zbus::dbus_interface;

use super::prelude::*;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CustomDBusConfig {
    path: StdString,
}

struct Block {
    api: CommonApi,
}

#[dbus_interface(name = "rs.swaystatus.dbus")]
impl Block {
    async fn set_icon(&mut self, icon: &str) -> StdString {
        if let Err(e) = self.api.set_icon(icon) {
            return e.to_string();
        }
        if let Err(e) = self.api.flush().await {
            return e.to_string();
        }
        "OK".into()
    }

    async fn set_text(&mut self, full: StdString, short: StdString) -> StdString {
        self.api.set_texts(full.into(), short.into());
        if let Err(e) = self.api.flush().await {
            return e.to_string();
        }
        "OK".into()
    }

    async fn set_state(&mut self, state: &str) -> StdString {
        match state {
            "idle" => self.api.set_state(State::Idle),
            "info" => self.api.set_state(State::Info),
            "good" => self.api.set_state(State::Good),
            "warning" => self.api.set_state(State::Warning),
            "critical" => self.api.set_state(State::Critical),
            _ => return format!("'{}' is not a valid state", state),
        }
        if let Err(e) = self.api.flush().await {
            return e.to_string();
        }
        "OK".into()
    }
}

pub async fn run(config: toml::Value, mut api: CommonApi) -> Result<()> {
    let path = CustomDBusConfig::deserialize(config).config_error()?.path;
    let dbus_conn = api.get_dbus_connection().await?;
    dbus_conn
        .object_server()
        .at(path, Block { api })
        .await
        .error("Failed to setup DBus server")?;
    Ok(())
}
