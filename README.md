<p align="center">
  <img src="assets/logo.png" alt="Logo" width="20%">
</p>

# NetStream - Typed streams for Rust client/server TCP and UDP networking

NetStream is a Rust library that provides networking abstractions for typed TCP and UDP streams. It simplifies the process of writing server and client implementations that can send and receive data over a network in a type-safe way.

With NetStream, you can define the messages that your server and client will send and receive using Rust structs. The library handles all the low-level networking details, so you can focus on writing high-level, type-safe code that's easy to reason about.
Framing and serialization is handled automatically. Currently, messages are serialized with [bincode](https://github.com/bincode-org/bincode), but this is an implementation detail subject to change.

NetStream can be used to build multiplayer games, distributed systems, or anything in between. This is currently a hobby project and is not recommended for production use. That being said, feedback, contributions, and bug reports from the community are welcome.

## Usage

To get started with NetStream, first define your message types by implementing the `net_stream::MessageTypes` trait for a struct:

```rust
#[derive(Debug, Clone)]
struct MessageTypes;

// These types represent the messages sent and received over TCP and UDP, respectively.
impl net_stream::MessageTypes for MessageTypes {
    // Message types for server/client communication over TCP.
    type TcpToServer = TcpToServer;
    type TcpFromServer = TcpFromServer;

    // No UDP messages used in this example.
    type UdpToServer = ();
    type UdpFromServer = ();
}
```

`TcpToServer` represents the message type sent from the client (peer) to the server over TCP. In this example, it has a single variant `Message(String)`, which represents a chat message sent by the client.

```rust
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
enum TcpToServer {
    /// Send message to chat room.
    Message(String),
}
```

`TcpFromServer` represents the message type sent from the server to the client (peer) over TCP.
In this example, it's an enum to represent different events occurring on a chat server, such as a new user connecting or a new message.

```rust
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
enum TcpFromServer {
    /// Welcome message.
    Welcome(WelcomeMessage),

    /// New user connected.
    UserConnect(User),

    /// User disconnected.
    UserDisconnect(User),

    /// New message posted to the chat room.
    NewMessage(String),
}
```

Create structs for the `WelcomeMessage` and `User` types.
By defining these message types and implementing the `net_stream::MessageTypes` trait, you create a typed interface for your server and client implementations. Serialization and framing is handled for you automatically.

### Usage: Server

Start server:

```rust
let config = net_stream::server::Config::default();
let (server_handle, mut events) = net_stream::server::start::<MessageTypes>(socket_addr, socket_addr, config).await?;
```

Handle events in a loop:

```rust
// Main server loop.
while let Some(ev) = events.next().await {
    // Handle server event
    match ev {
        net_stream::server::event::Event::NewPeer(new_peer) => {
            // New client has connected to the server.
        }
        net_stream::server::event::Event::TcpMessage(tcp_message) => {
            // Message received from one of the connected clients over TCP.
        }
        net_stream::server::event::Event::UdpMessage(udp_message) => {
            // Message received from one of the connected clients over UDP.
        }
        net_stream::server::event::Event::PeerDisconnect(peer_disconnect) => {
            // A client has voluntarily disconnected or has been dropped by the server.
        }
    }
}
```

Use the `server_handle` to perform actions, like sending messages to connected clients:

```rust
// Send a message to a single connected client on TCP.
server_handle.message_peer_tcp(new_peer.peer_uid, TcpFromServer::Welcome(WelcomeMessage {
    // (...)
}));
```

Or sending a message to all connected clients:

```rust
// Broadcast a message to all connected clients
server_handle.announce_tcp(TcpFromServer::NewMessage(String::from("message")));
```

### Usage: Client

Connect to the server:

```rust
let (mut handle, mut events) = net_stream::client::connect::<MessageTypes>("my.server.com").await?;
```

Handle messages received from the server:

```rust
while let Some(ev) = events.next().await {
    match ev {
        net_stream::client::event::Event::TcpMessage(msg) => {
            // Handle message received from server over TCP.
        }
        net_stream::client::event::Event::UdpMessage(msg) => {
            // Handle message received from server over UDP.
        }
        net_stream::client::event::Event::CanSendUdpMessages => {
            // This event simply indicates that a test message sent from client to server over UDP has reached the server.
            // This lets you know that there is no firewall blocking the UDP messages in this direction.
        }
        net_stream::client::event::Event::CanReceiveUdpMessages => {
            // This event simply indicates that a test message sent from server to client over UDP has reached the client.
            // This lets you know that there is no firewall blocking the UDP messages in this direction.
        }
    }
}
```

Use the `handle` to send messages to the server:

```rust
if let Err(err) = handle.send_message_tcp(TcpToServer::Message(String::from("Hello!"))) {
    log::error!("{:?}", err);
}
```

See [examples](./examples) for further details.
