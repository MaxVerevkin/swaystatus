//! A block controled by the DBus
//!
//! This block runs a DBus server with a custom name specified in the configuration. It creates
//! only one path  `/` that implements `rs.swaystatus.dbus` interface (output of `qbus <name> /`):
//! ```text
//! method void rs.swaystatus.dbus.SetFullText(QString full)
//! method void rs.swaystatus.dbus.SetIcon(QString icon)
//! method void rs.swaystatus.dbus.SetState(QString state)
//! method void rs.swaystatus.dbus.SetText(QString full, QString short)
//! ```
//!
//! # Example
//!
//! Config:
//! ```toml
//! [[block]]
//! block = "custom_dbus"
//! name = "my.example.block"
//! ```
//!
//! Useage:
//! ```sh
//! # set test to 'hello'
//! busctl --user call my.example.block / rs.swaystatus.dbus SetFullText s hello
//! # set icon to 'music'
//! busctl --user call my.example.block / rs.swaystatus.dbus SetIcon s music
//! # set state to 'good'
//! busctl --user call my.example.block / rs.swaystatus.dbus SetState s good
//! # set full test to 'hello' and short text to 'hi'
//! busctl --user call my.example.block / rs.swaystatus.dbus SetText ss hello hi
//! ```

use dbus::channel::MatchingReceiver;
use dbus::message::MatchRule;
use dbus::MethodErr;
use dbus_crossroads::Crossroads;
use dbus_tokio::connection;

use serde::de::Deserialize;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::widget::{State, Widget};

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct CustomDBusConfig {
    name: String,
}

struct Block {
    id: usize,
    text: Widget,
    sender: mpsc::Sender<BlockMessage>,
}

// TODO: send a signal in click?
pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    // Drop the reciever if we don't what to recieve events
    drop(events_reciever);

    // Parse config
    let dbus_name = CustomDBusConfig::deserialize(block_config)
        .block_config_error("custom_dbus")?
        .name;

    // Open dbus connection
    let (resource, dbus_conn) =
        connection::new_session_sync().block_error("music", "failed to open DBUS connection")?;
    tokio::spawn(async {
        let err = resource.await;
        panic!("Lost connection to D-Bus: {}", err);
    });

    // Let's request a name on the bus, so that clients can find us.
    // TODO revisit request_name() parameters
    dbus_conn
        .request_name(dbus_name, false, false, false)
        .await
        .block_error("custom_dbus", "request_name() failed")?;

    // Create a new crossroads instance.
    let mut crossroads = Crossroads::new();
    // Enable async support for the crossroads instance.
    crossroads.set_async_support(Some((
        dbus_conn.clone(),
        Box::new(|x| {
            tokio::spawn(x);
        }),
    )));

    // Let's build a new interface
    let iface_token = crossroads.register("rs.swaystatus.dbus", |b| {
        // Let's add a method to the interface. We have the method name, followed by
        // names of input and output arguments (used for introspection). The closure then controls
        // the types of these arguments. The last argument to the closure is a tuple of the input arguments.
        b.method_with_cr_async(
            "SetIcon",
            ("icon",),
            (),
            |mut ctx, cr, (icon,): (String,)| {
                let block: &mut Block = cr.data_mut(ctx.path()).unwrap(); // ok_or_else(|| MethodErr::no_path(ctx.path()))?;
                let result = block
                    .text
                    .set_icon(&icon)
                    .map_err(|e| MethodErr::failed(&e.to_string()));
                let sender = block.sender.clone();
                let message = BlockMessage {
                    id: block.id,
                    widgets: vec![block.text.get_data()],
                };
                async move {
                    // TODO do not ignore error
                    let _ = sender.send(message).await;
                    ctx.reply(result)
                }
            },
        );
        b.method_with_cr_async(
            "SetText",
            ("full", "short"),
            (),
            |mut ctx, cr, (full, short): (String, String)| {
                let block: &mut Block = cr.data_mut(ctx.path()).unwrap(); // ok_or_else(|| MethodErr::no_path(ctx.path()))?;
                block.text.set_text((full, Some(short)));
                let sender = block.sender.clone();
                let message = BlockMessage {
                    id: block.id,
                    widgets: vec![block.text.get_data()],
                };
                async move {
                    let _ = sender.send(message).await;
                    ctx.reply(Ok(()))
                }
            },
        );
        b.method_with_cr_async(
            "SetFullText",
            ("full",),
            (),
            |mut ctx, cr, (full,): (String,)| {
                let block: &mut Block = cr.data_mut(ctx.path()).unwrap(); // ok_or_else(|| MethodErr::no_path(ctx.path()))?;
                block.text.set_text((full, None));
                let sender = block.sender.clone();
                let message = BlockMessage {
                    id: block.id,
                    widgets: vec![block.text.get_data()],
                };
                async move {
                    let _ = sender.send(message).await;
                    ctx.reply(Ok(()))
                }
            },
        );
        b.method_with_cr_async(
            "SetState",
            ("state",),
            (),
            |mut ctx, cr, (state,): (String,)| {
                let block: &mut Block = cr.data_mut(ctx.path()).unwrap(); // ok_or_else(|| MethodErr::no_path(ctx.path()))?;
                let mut succes = true;
                match state.as_str() {
                    "idle" => block.text.set_state(State::Idle),
                    "info" => block.text.set_state(State::Info),
                    "good" => block.text.set_state(State::Good),
                    "warning" => block.text.set_state(State::Warning),
                    "critical" => block.text.set_state(State::Critical),
                    _ => succes = false,
                }
                let sender = block.sender.clone();
                let message = BlockMessage {
                    id: block.id,
                    widgets: vec![block.text.get_data()],
                };
                async move {
                    let _ = sender.send(message).await;
                    if succes {
                        ctx.reply(Ok(()))
                    } else {
                        ctx.reply(Err(MethodErr::failed("Incorrect state")))
                    }
                }
            },
        );
    });

    // Let's add the "/" path, which implements the rs.swaystatus.dbus interface to the crossroads instance.
    crossroads.insert(
        "/",
        &[iface_token],
        Block {
            id,
            text: Widget::new(id, shared_config),
            sender: message_sender,
        },
    );

    // We add the Crossroads instance to the connection so that incoming method calls will be handled.
    dbus_conn.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |msg, conn| {
            crossroads.handle_message(msg, conn).unwrap();
            true
        }),
    );

    // Everything is setup
    Ok(())
}
