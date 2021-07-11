use neli::{
    consts::{nl::*, rtnl::*, socket::*},
    nl::{NlPayload, Nlmsghdr},
    rtnl::*,
    socket::*,
    types::RtBuffer,
};

use std::convert::TryInto;
use std::path::{Path, PathBuf};

use crate::errors::*;
use crate::util;
use crate::util::escape_pango_text;

#[derive(Debug)]
pub struct NetDevice {
    pub interface: String,
    pub path: PathBuf,
    pub wireless: bool,
    pub tun: bool,
    pub wg: bool,
    pub ppp: bool,
    pub icon: &'static str,
}

impl NetDevice {
    /// Use the network device `device`. Raises an error if a directory for that
    /// device is not found.
    pub async fn from_interface(interface: String) -> Self {
        let path = Path::new("/sys/class/net").join(interface.clone());

        // I don't believe that this should ever change, so set it now:
        let wireless = path.join("wireless").exists();
        let tun = path.join("tun_flags").exists()
            || interface.starts_with("tun")
            || interface.starts_with("tap");

        let uevent_path = path.join("uevent");
        let uevent_content = util::read_file(&uevent_path).await;

        let wg = match &uevent_content {
            Ok(s) => s.contains("wireguard"),
            Err(_e) => false,
        };
        let ppp = match &uevent_content {
            Ok(s) => s.contains("ppp"),
            Err(_e) => false,
        };

        let icon = if wireless {
            "net_wireless"
        } else if tun || wg || ppp {
            "net_vpn"
        } else if interface == "lo" {
            "net_loopback"
        } else {
            "net_wired"
        };

        NetDevice {
            interface,
            path,
            wireless,
            tun,
            wg,
            ppp,
            icon,
        }
    }

    pub async fn read_stats(&self) -> Option<(u64, u64)> {
        let rx: u64 = util::read_file(&self.path.join("statistics/rx_bytes"))
            .await
            .ok()
            .and_then(|x| x.parse().ok())?;
        let tx: u64 = util::read_file(&self.path.join("statistics/tx_bytes"))
            .await
            .ok()
            .and_then(|x| x.parse().ok())?;
        Some((rx, tx))
    }

    /// Queries the wireless SSID of this device, if it is connected to one.
    pub fn wifi_info(&self) -> Result<(Option<String>, Option<f64>, Option<i64>)> {
        let interfaces = nl80211::Socket::connect()
            .block_error("net", "nl80211: failed to connect to the socket")?
            .get_interfaces_info()
            .block_error("net", "nl80211: failed to get interfaces' information")?;

        for interface in interfaces {
            if let Ok(ap) = interface.get_station_info() {
                // SSID is `None` when not connected
                if let (Some(ssid), Some(device)) = (interface.ssid, interface.name) {
                    let device = String::from_utf8_lossy(&device);
                    let device = device.trim_matches(char::from(0));
                    if device != self.interface {
                        continue;
                    }

                    let ssid = String::from_utf8(ssid)
                        .internal_error("netlink.rs", "SSID is not valid UTF8")?;
                    let ssid = Some(escape_pango_text(ssid));
                    let freq = interface
                        .frequency
                        .map(|f| nl80211::parse_u32(&f) as f64 * 1e6);
                    let signal = ap.signal.map(|s| signal_percents(nl80211::parse_i8(&s)));

                    return Ok((ssid, freq, signal));
                }
            }
        }

        Ok((None, None, None))
    }
}

fn index_to_interface(index: u32) -> String {
    let mut buff = [0i8; 16];
    let buff: [u8; 16] = unsafe {
        libc::if_indextoname(index, &mut buff[0]);
        std::mem::transmute(buff)
    };

    std::str::from_utf8(&buff)
        .unwrap()
        .trim_matches(char::from(0))
        .to_string()
}

// TODO FIXME make async
pub fn default_interface() -> Option<String> {
    let mut socket = NlSocketHandle::connect(NlFamily::Route, None, &[]).ok()?;

    let rtmsg = Rtmsg {
        rtm_family: RtAddrFamily::Inet,
        rtm_dst_len: 0,
        rtm_src_len: 0,
        rtm_tos: 0,
        rtm_table: RtTable::Main,
        rtm_protocol: Rtprot::Unspec,
        rtm_scope: RtScope::Universe,
        rtm_type: Rtn::Unspec,
        rtm_flags: RtmFFlags::empty(),
        rtattrs: RtBuffer::new(),
    };
    let nlhdr = {
        let len = None;
        let nl_type = Rtm::Getroute;
        let flags = NlmFFlags::new(&[NlmF::Request, NlmF::Dump]);
        let seq = None;
        let pid = None;
        let payload = rtmsg;
        Nlmsghdr::new(len, nl_type, flags, seq, pid, NlPayload::Payload(payload))
    };

    socket.send(nlhdr).ok()?;

    for rtm_result in socket.iter(false) {
        let rtm: Nlmsghdr<NlTypeWrapper, Rtmsg> = rtm_result.ok()?;
        if let NlTypeWrapper::Rtm(_) = rtm.nl_type {
            let payload = rtm.get_payload().ok()?;
            if payload.rtm_table == RtTable::Main {
                let mut is_default = false;
                let mut name = None;
                for attr in payload.rtattrs.iter() {
                    match attr.rta_type {
                        Rta::Dst => is_default = true,
                        Rta::Oif => {
                            name = Some(index_to_interface(u32::from_le_bytes(
                                attr.rta_payload.as_ref().try_into().unwrap(),
                            )))
                        }
                        _ => (),
                    }
                }
                if is_default {
                    return name;
                }
            }
        }
    }

    None
}

fn signal_percents(raw: i8) -> i64 {
    let raw = raw as f64;

    let perfect = -20.;
    let worst = -85.;
    let d = perfect - worst;

    // https://github.com/torvalds/linux/blob/9ff9b0d392ea08090cd1780fb196f36dbb586529/drivers/net/wireless/intel/ipw2x00/ipw2200.c#L4322-L4334
    let percents = 100. - (perfect - raw) * (15. * d + 62. * (perfect - raw)) / (d * d);

    (percents as i64).clamp(0, 100)
}
