use neli::{
    consts::{nl::*, rtnl::*, socket::*},
    nl::{NlPayload, Nlmsghdr},
    rtnl::*,
    socket::*,
    types::RtBuffer,
};
use std::convert::TryInto;

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
