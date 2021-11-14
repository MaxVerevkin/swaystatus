//! Monitor Bluetooth device
//!
//! This block displays the connectivity of a given Bluetooth device and the battery level if this
//! is supported. Relies on the Bluez D-Bus API.
//!
//! When the device can be identified as an audio headset, a keyboard, joystick, or mouse, use the
//! relevant icon. Otherwise, fall back on the generic Bluetooth symbol.
//!
//! Right-clicking the block will attempt to connect (or disconnect) the device.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `mac` | MAC address of the Bluetooth device | Yes | N/A
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | <code>"$name{ $percentage&vert;}"</code>
//! `hide_disconnected` | Whether to hide the block when disconnected | No | `false`
//!
//! Placeholder  | Value                                                                 | Type   | Unit
//! -------------|-----------------------------------------------------------------------|--------|------
//! `name`       | Device's name                                                         | Text   | -
//! `percentage` | Device's battery level (may be absent if the device is not supported) | Number | %
//!
//! # Examples
//!
//! This example just shows the icon when device is connected.
//!
//! ```toml
//! [[block]]
//! block = "bluetooth"
//! mac = "00:18:09:92:1B:BA"
//! hide_disconnected = true
//! format = ""
//! ```
//!
//! # Icons Used
//! - `headphones` for bluetooth devices identifying as "audio-card"
//! - `joystick` for bluetooth devices identifying as "input-gaming"
//! - `keyboard` for bluetooth devices identifying as "input-keyboard"
//! - `mouse` for bluetooth devices identifying as "input-mouse"
//! - `bluetooth` for all other devices
//!
//! # TODO:
//! - Don't throw errors when there is no bluetooth

#![allow(clippy::type_complexity)]

use futures::{Stream, StreamExt};
use std::pin::Pin;
use zbus::fdo::ObjectManagerProxy;
use zbus_names::InterfaceName;

use super::prelude::*;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct BluetoothConfig {
    mac: String,
    #[serde(default)]
    format: FormatConfig,
    #[serde(default)]
    hide_disconnected: bool,
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, events: EventsRxGetter) -> BlockHandle {
    let mut events = events();
    tokio::spawn(async move {
        let block_config = BluetoothConfig::deserialize(block_config).config_error()?;
        api.set_format(block_config.format.init("$name{ $percentage|}", &api)?);

        let dbus_conn = api.system_dbus_connection().await?;
        let device = Device::from_mac(&dbus_conn, &block_config.mac).await?;
        api.set_icon(device.icon)?;

        let name = device
            .device_proxy
            .name()
            .await
            .unwrap_or_else(|_| "N/A".to_string());
        let mut connected = device
            .device_proxy
            .connected()
            .await
            .error("Failed to get device state")?;

        let mut connected_stream = device.device_proxy.receive_connected_changed().await;

        let (mut battery_stream, mut percentage): (
            Pin<Box<dyn Stream<Item = Option<u8>> + Send + Sync>>,
            Option<u8>,
        ) = if let Some(bp) = &device.battery_proxy {
            (
                Box::pin(bp.receive_percentage_changed().await),
                Some(bp.percentage().await.error("Failed to get percentage")?),
            )
        } else {
            (Box::pin(futures::stream::empty()), None)
        };

        loop {
            if connected || !block_config.hide_disconnected {
                api.set_state(if connected {
                    WidgetState::Good
                } else {
                    WidgetState::Idle
                });
                let mut values = map! {
                    "name" => Value::text((&name).into()),
                };
                percentage.map(|p| values.insert("percentage".into(), Value::percents(p)));
                api.set_values(values);
                api.show();
                api.render();
            } else {
                api.hide();
            }
            api.flush().await?;

            tokio::select! {
                Some(BlockEvent::Click(click)) = events.recv() => {
                    if click.button == MouseButton::Right {
                        if connected {
                            let _ = device.device_proxy.disconnect().await;
                        } else {
                            let _ = device.device_proxy.connect().await;
                        }
                    }
                }
                Some(Some(new_connected)) = connected_stream.next() => {
                    connected = new_connected;
                }
                Some(Some(new_precentage)) = battery_stream.next() => {
                    percentage = Some(new_precentage);
                }
            }
        }
    })
}

#[zbus::dbus_proxy(interface = "org.bluez.Device1", default_service = "org.bluez")]
trait Device1 {
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;

    #[dbus_proxy(property)]
    fn connected(&self) -> zbus::Result<bool>;

    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<StdString>;
}

#[zbus::dbus_proxy(interface = "org.bluez.Battery1", default_service = "org.bluez")]
trait Battery1 {
    #[dbus_proxy(property)]
    fn percentage(&self) -> zbus::Result<u8>;
}

struct Device<'a> {
    icon: &'static str,
    device_proxy: Device1Proxy<'a>,
    battery_proxy: Option<Battery1Proxy<'a>>,
}

impl<'a> Device<'a> {
    async fn from_mac(dbus_conn: &'a zbus::Connection, mac: &str) -> Result<Device<'a>> {
        // Get list of all devics
        let devices = ObjectManagerProxy::builder(dbus_conn)
            .destination("org.bluez")
            .unwrap()
            .path("/")
            .unwrap()
            .build()
            .await
            .error("Failed to create ObjectManagerProxy")?
            .get_managed_objects()
            .await
            .error("Failed to obtain the list of devices")?;
        // Find the device with specified MAC
        for (path, interfaces) in devices {
            // TODO: avoid this allocation
            // InterfaceName<'static> should impl AsRef<OwnedInterfaceName>, but it doesn't.
            let interface_name = |name: &'static str| InterfaceName::try_from(name).unwrap().into();

            if let Some(device_inter) = interfaces.get(&interface_name("org.bluez.Device1")) {
                let addr: &str = device_inter.get("Address").unwrap().downcast_ref().unwrap();
                if addr != mac {
                    continue;
                }

                let icon: &str = device_inter.get("Icon").unwrap().downcast_ref().unwrap();
                let icon = match icon {
                    "audio-card" => "headphones",
                    "input-gaming" => "joystick",
                    "input-keyboard" => "keyboard",
                    "input-mouse" => "mouse",
                    _ => "bluetooth",
                };

                let battery_proxy = if interfaces
                    .get(&interface_name("org.bluez.Battery1"))
                    .is_some()
                {
                    Some(
                        Battery1Proxy::builder(dbus_conn)
                            .path(path.clone())
                            .unwrap()
                            .build()
                            .await
                            .error("Failed to create Battery1Proxy")?,
                    )
                } else {
                    None
                };

                return Ok(Self {
                    icon,
                    device_proxy: Device1Proxy::builder(dbus_conn)
                        .path(path)
                        .unwrap()
                        .build()
                        .await
                        .error("Failed to create Device1Proxy")?,
                    battery_proxy,
                });
            }
        }
        Err(Error::new("Device not found"))
    }
}
