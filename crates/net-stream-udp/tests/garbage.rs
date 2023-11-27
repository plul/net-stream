use assert_let_bind::assert_let;
use core::time::Duration;
use futures::StreamExt;
use net_stream_udp::server::event::Event;
use tokio::net::TcpStream;
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
    env_logger::builder()
        .filter(None, log::LevelFilter::Debug)
        .parse_default_env()
        .is_test(true)
        .init();

    let server_socket_addr = "127.0.0.1:5100".parse().unwrap();

    let config = net_stream_udp::server::Config::default();
    let (_server_handle, mut server_rx) = net_stream_udp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let tcp_client = TcpStream::connect(server_socket_addr).await.unwrap();
    tcp_client.writable().await.unwrap();
    let local_addr = tcp_client.local_addr().unwrap();
    let udp_client = UdpSocket::bind(local_addr).await.unwrap();
    udp_client.connect(server_socket_addr).await.unwrap();

    let ev = timeout(Duration::from_millis(100), server_rx.next()).await.expect("timeout").unwrap();
    assert_let!(Event::NewPeer(new_peer), ev);
    let _peer_uid = new_peer.peer_uid;

    // TODO this test needs to be repaired: The UDP handshake hasn't been completed at this point - so
    // the server doesn't know that the garbage we're about to send comes from the connected peer.

    // // Send garbage
    // udp_client.send(b"hello guv'na").await.unwrap();

    // let ev = timeout(Duration::from_millis(100), server_rx.next()).await.expect("Timeout").unwrap();

    // assert_let!(Event::PeerDisconnect(peer_disconnect), ev);
    // assert_eq!(peer_uid, peer_disconnect.peer_uid);
    // assert!(matches!(
    //     peer_disconnect.disconnect_reason,
    //     event::DisconnectReason::PeerSubmittedUnintelligibleData
    // ));
}
