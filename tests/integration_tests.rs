#![feature(assert_matches)]

mod integration_tests {
    mod announce_tcp;
    mod announce_udp;
    mod max_connections;
    mod message_types;
    mod peer_disconnect;
    mod peer_uids;
    mod tcp_garbage;
    mod udp_garbage;
    mod udp_heartbeat;
}

use futures::StreamExt as _;
use std::sync::Once;

static ENV_LOGGER_INIT: Once = Once::new();

fn env_logger_setup() {
    ENV_LOGGER_INIT.call_once(|| {
        env_logger::builder()
            .filter(None, log::LevelFilter::Info)
            .filter(Some(std::module_path!()), log::LevelFilter::Trace)
            .filter(Some("net_stream"), log::LevelFilter::Trace)
            .parse_default_env()
            .is_test(true)
            .try_init()
            .expect("Failed to initialise env_logger");
    });
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct StringMessages;
impl net_stream::MessageTypes for StringMessages {
    type TcpToServer = String;
    type TcpFromServer = String;
    type UdpToServer = String;
    type UdpFromServer = String;
}

/// Expect the next two events to be, in random order
/// - Event::CanReceiveUdpMessages
/// - Event::CanSendUdpMessages
async fn expect_udp_events_can_send_and_receive_udp_messages<M>(events: &mut futures::channel::mpsc::Receiver<net_stream::client::event::Event<M>>)
where
    M: net_stream::MessageTypes + Eq + std::hash::Hash,
    <M as net_stream::MessageTypes>::TcpFromServer: Eq + std::hash::Hash,
    <M as net_stream::MessageTypes>::UdpFromServer: Eq + std::hash::Hash,
{
    let mut actual = std::collections::HashSet::new();
    actual.insert(events.next().await.unwrap());
    actual.insert(events.next().await.unwrap());

    let expected = [
        net_stream::client::event::Event::CanReceiveUdpMessages,
        net_stream::client::event::Event::CanSendUdpMessages,
    ]
    .into_iter()
    .collect();

    assert_eq!(actual, expected);
}
