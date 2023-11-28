use assert_let_bind::assert_let;
use core::time::Duration;
use futures::StreamExt;
use net_stream_udp::server::event::Event;
use tokio::net::UdpSocket;
use tokio::time::timeout;

#[derive(Debug, PartialEq, Eq, Hash)]
struct M;
impl net_stream_udp::MessageTypes for M {
    type ToServer = String;
    type FromServer = String;
}

#[tokio::test]
async fn udp_garbage() {
    setup();

    let config = net_stream_udp::server::Config::default();
    let mut server = net_stream_udp::server::start::<M>("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let udp_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    udp_client.connect(server.local_addr).await.unwrap();

    // Send garbage
    udp_client.send(b"hello guv'na").await.unwrap();

    // Expect no New Peer event, because we've sent no good payloads.
    timeout(Duration::from_millis(100), server.event_receiver.next())
        .await
        .expect_err("this should have timed out");
}

#[tokio::test]
async fn udp_garbage_after_one_good_payload() {
    setup();

    let config = net_stream_udp::server::Config::default();
    let mut server = net_stream_udp::server::start::<M>("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let udp_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    udp_client.connect(server.local_addr).await.unwrap();

    // TODO: Send one good payload (heartbeat fx), then the below should work

    // Send garbage
    udp_client.send(b"hello guv'na").await.unwrap();

    // Expect New Peer event
    let ev = timeout(Duration::from_millis(100), server.event_receiver.next())
        .await
        .expect("timeout")
        .unwrap();
    assert_let!(Event::NewPeer(new_peer), ev);

    // Expect Unintelligible peer event
    let ev = timeout(Duration::from_millis(100), server.event_receiver.next())
        .await
        .expect("timeout")
        .unwrap();
    assert_let!(Event::UnintelligiblePeer(unintelligible_peer), ev);
    assert_eq!(new_peer.peer_uid, unintelligible_peer.peer_uid);

    // Send more garbage
    udp_client.send(b"whazz uuuuuuup").await.unwrap();

    // Expect no further unintelligble event
    timeout(Duration::from_millis(100), server.event_receiver.next())
        .await
        .expect_err("this should have timed out");
}

static ENV_LOGGER_INIT: std::sync::Once = std::sync::Once::new();
fn setup() {
    ENV_LOGGER_INIT.call_once(|| {
        env_logger::builder()
            .filter(None, log::LevelFilter::Debug)
            .parse_default_env()
            .is_test(true)
            .init()
    });
}
