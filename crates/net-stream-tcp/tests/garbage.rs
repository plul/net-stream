use assert_let_bind::assert_let;
use core::time::Duration;
use futures::StreamExt;
use net_stream_tcp::server::event;
use net_stream_tcp::server::event::Event;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, PartialEq, Eq, Hash)]
struct M;
impl net_stream_tcp::MessageTypes for M {
    type ToServer = String;
    type FromServer = String;
}

#[tokio::test]
async fn main() {
    env_logger::builder()
        .filter(None, log::LevelFilter::Debug)
        .parse_default_env()
        .is_test(true)
        .init();

    let server_socket_addr = "127.0.0.1:5100".parse().unwrap();

    let config = net_stream_tcp::server::Config::default();
    let (_server_handle, mut server_rx) = net_stream_tcp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let mut tcp_client = TcpStream::connect(server_socket_addr).await.unwrap();
    tcp_client.writable().await.unwrap();

    // Expect connection event
    let ev = timeout(Duration::from_millis(100), server_rx.next()).await.unwrap().unwrap();
    assert!(matches!(ev, Event::NewPeer(_)));

    // Garbage on TCP should drop client
    tcp_client.write_all(b"garbage").await.unwrap();
    let ev = timeout(Duration::from_millis(100), server_rx.next()).await.unwrap().unwrap();
    assert_let!(Event::PeerDisconnect(peer_disconnect), ev);
    assert!(matches!(
        peer_disconnect.disconnect_reason,
        event::DisconnectReason::PeerSubmittedUnintelligibleData
    ));

    // assert tcp connection is closed by remote
    let mut buf = [0];
    let read_result = tcp_client.read_exact(&mut buf).await;
    assert_let!(Err(err), read_result);
    assert!(matches!(err.kind(), std::io::ErrorKind::UnexpectedEof));
}
