use clap::Parser;
use futures::StreamExt;
use net_stream::server::PeerUid;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use tokio::io::AsyncBufReadExt;

#[tokio::main]
async fn main() {
    let Cli { bind, port, command } = Cli::parse();
    let host = format!("{bind}:{port}");
    let socket_addr: SocketAddr = tokio::net::lookup_host(&host).await.unwrap().next().unwrap();

    match command {
        Command::Server => server(socket_addr).await,
        Command::Client => client(&host).await,
    }
    .unwrap();
}

#[derive(Debug, Clone)]
struct MessageTypes;
impl net_stream::MessageTypes for MessageTypes {
    type TcpToServer = FromPeerToServer;
    type TcpFromServer = FromServerToPeer;

    // No UDP messages used in this example.
    type UdpToServer = ();
    type UdpFromServer = ();
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
enum FromPeerToServer {
    /// Send message to chat room.
    Message(String),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
enum FromServerToPeer {
    /// Welcome message.
    Welcome(WelcomeMessage),

    /// New user connected.
    UserConnect(User),

    /// User disconnected.
    UserDisconnect(User),

    /// New message.
    NewMessage(Message),
}

/// Welcome message received upon connecting to the server.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct WelcomeMessage {
    /// For this example, the server just assigns a name to the peer automatically.
    /// In a real application, the peer might want to send its preferred username instead.
    your_name: String,

    /// As part of the welcome message, tell the new user who else is in the chat room.
    connected_users: Vec<User>,

    /// List of recent messages for some history.
    latest_messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct User {
    name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Message {
    from: User,
    message: String,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Bind address
    #[arg(long, default_value = "::")]
    bind: String,

    /// Port number
    #[arg(long, default_value = "5000")]
    port: u16,

    /// Command
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Launch server.
    Server,

    /// Launch client.
    Client,
}

#[derive(Debug, Default)]
struct ServerState {
    /// The connected users at any time.
    connected_users: HashMap<PeerUid, User>,

    /// The server keeps a small number of messages in memory.
    /// When a new user connects to the chat room, the server sends them a welcome message containing this list of messages.
    messages: VecDeque<Message>,

    /// Counter incremented for each user that connects.
    /// This is used to generate usernames, `User 1`, `User 2`, etc.
    user_counter: usize,
}

async fn server(socket_addr: SocketAddr) -> anyhow::Result<()> {
    // TODO: Config with UDP disabled?
    let config = net_stream::server::Config::default();
    let (server_handle, mut events) = net_stream::server::start::<MessageTypes>(socket_addr, socket_addr, config).await?;

    let mut state = ServerState::default();

    // Main server loop.
    while let Some(ev) = events.next().await {
        match ev {
            net_stream::server::event::Event::NewPeer(new_peer) => {
                // Give the user a username.
                state.user_counter += 1;
                let name = format!("User {}", state.user_counter);

                let user = User { name: name.clone() };

                // Insert new peer into server state.
                state.connected_users.insert(new_peer.peer_uid, user.clone());

                // Send a welcome message to the newly connected peer.
                let welcome_msg = FromServerToPeer::Welcome(WelcomeMessage {
                    connected_users: state.connected_users.values().cloned().collect(),
                    latest_messages: state.messages.iter().cloned().collect(),
                    your_name: name,
                });
                server_handle.message_peer_tcp(new_peer.peer_uid, welcome_msg);

                // Announce to the chat room that a new user has connected.
                server_handle.announce_tcp(FromServerToPeer::UserConnect(user));
            }
            net_stream::server::event::Event::TcpMessage(tcp_message) => {
                let from: PeerUid = tcp_message.from;
                let user: &User = state.connected_users.get(&from).unwrap();

                match tcp_message.message {
                    FromPeerToServer::Message(s) => {
                        let message = Message {
                            from: user.clone(),
                            message: s,
                        };

                        // Announce the message to everybody in the chat room.
                        server_handle.announce_tcp(FromServerToPeer::NewMessage(message.clone()));

                        // Save the message to the server's state.
                        state.messages.push_back(message);
                        // But don't keep more than 10 messages.
                        while state.messages.len() > 10 {
                            state.messages.pop_front();
                        }
                    }
                }
            }
            net_stream::server::event::Event::PeerDisconnect(peer_disconnect) => {
                // Remove peer from server state.
                let user = state.connected_users.remove(&peer_disconnect.peer_uid).unwrap();

                // Announce to everybody in the chat room that this user has disconnected.
                server_handle.announce_tcp(FromServerToPeer::UserDisconnect(user));
            }
            net_stream::server::event::Event::UdpMessage(_) => {}
        }
    }

    Ok(())
}

async fn client(server_host: &str) -> anyhow::Result<()> {
    let (mut handle, mut events) = net_stream::client::connect::<MessageTypes>(server_host).await?;

    // Receive events from the server.
    tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            match ev {
                net_stream::client::event::Event::TcpMessage(msg) => match msg {
                    FromServerToPeer::Welcome(welcome_msg) => {
                        println!("You connected as: {}", welcome_msg.your_name);
                        println!(
                            "Users in the chat room: {}",
                            welcome_msg
                                .connected_users
                                .into_iter()
                                .map(|u| u.name)
                                .collect::<Vec<String>>()
                                .join(", ")
                        );
                        println!("Latest messages:");
                        for msg in welcome_msg.latest_messages {
                            println!("{}: {}", msg.from.name, msg.message);
                        }
                    }
                    FromServerToPeer::NewMessage(msg) => {
                        println!("{}: {}", msg.from.name, msg.message);
                    }
                    FromServerToPeer::UserConnect(user) => {
                        println!("New user connected: {}", user.name);
                    }
                    FromServerToPeer::UserDisconnect(user) => {
                        println!("User disconnected: {}", user.name);
                    }
                },

                // UDP events - Not in play for this example
                net_stream::client::event::Event::UdpMessage(_) => {}
                net_stream::client::event::Event::CanSendUdpMessages => {}
                net_stream::client::event::Event::CanReceiveUdpMessages => {}
            }
        }
    });

    // Use handle to send messages to the server.
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();
    loop {
        match reader.read_line(&mut line).await {
            Ok(0) => break, // end of input
            Ok(_) => {
                if let Err(err) = handle.send_message_tcp(FromPeerToServer::Message(line.clone())) {
                    log::error!("{err}");
                }
                line.clear();
            }
            Err(err) => {
                eprintln!("Error reading line: {}", err);
                break;
            }
        }
    }

    Ok(())
}

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Cli::command().debug_assert()
}
