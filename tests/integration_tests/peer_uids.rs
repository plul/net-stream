use core::time::Duration;
use futures::StreamExt;
use net_stream::server::event::Event;
use net_stream::server::event::NewPeer;
use net_stream::server::PeerUid;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::test]
async fn main() {
    crate::env_logger_setup();

    let server_socket_addr = "127.0.0.1:5100".parse().unwrap();

    let config = net_stream::server::Config::default();
    let (_server_handle, mut server_rx) = net_stream::server::start::<crate::StringMessages>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let tcp_client = TcpStream::connect(server_socket_addr).await.unwrap();
    tcp_client.writable().await.unwrap();

    let ev = timeout(Duration::from_millis(100), server_rx.next()).await.unwrap().unwrap();
    assert!(
        matches!(ev, Event::NewPeer(NewPeer { peer_uid: PeerUid(0) }),),
        "First peer should be peer with UID 0"
    );

    // Peer Connect
    let tcp_client = TcpStream::connect(server_socket_addr).await.unwrap();
    tcp_client.writable().await.unwrap();

    let ev = timeout(Duration::from_millis(100), server_rx.next()).await.unwrap().unwrap();
    assert!(
        matches!(ev, Event::NewPeer(NewPeer { peer_uid: PeerUid(1) })),
        "Second peer should be peer with UID 1"
    );
}
