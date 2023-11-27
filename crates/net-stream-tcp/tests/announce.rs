use assert_let_bind::assert_let;
use client::event::Event as ClientEvent;
use futures::StreamExt;
use net_stream_tcp::client;
use net_stream_tcp::server;
use server::event::Event as ServerEvent;

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

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    log::info!("Starting server...");
    let config = net_stream_tcp::server::Config::default();
    let mut server = net_stream_tcp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");

    log::info!("Started server");

    log::info!("Connecting client 1...");
    let (_client_1_handle, mut client_1_events) = net_stream_tcp::client::connect::<M>(server_host).await.unwrap();
    log::info!("OK: Connected client 1");
    assert!(matches!(server.event_receiver.next().await.unwrap(), ServerEvent::NewPeer(_)));

    server.actor_handle.announce(String::from("First message!"));

    {
        let msg = client_1_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::Message(msg), msg);
        assert_eq!(msg, "First message!");
    }

    log::info!("Connecting client 2...");
    let (_client_2_handle, mut client_2_events) = net_stream_tcp::client::connect::<M>(server_host).await.unwrap();
    log::info!("OK: Connected client 2");
    assert!(matches!(server.event_receiver.next().await.unwrap(), ServerEvent::NewPeer(_)));

    server.actor_handle.announce(String::from("Second message!"));

    {
        let event = client_1_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::Message(msg), event);
        assert_eq!(msg, "Second message!");
    }
    {
        let event = client_2_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::Message(msg), event);
        assert_eq!(msg, "Second message!");
    }
}
