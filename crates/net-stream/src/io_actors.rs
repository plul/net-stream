pub mod read_actor;
pub mod write_actor;

#[derive(Debug, Clone, Copy, derive_more::From)]
pub enum ActorShutdown<T> {
    Controlled(T),

    #[from]
    Erratic(ErraticActorShutdown),
}
impl<T> From<Result<T, ErraticActorShutdown>> for ActorShutdown<T> {
    fn from(result: Result<T, ErraticActorShutdown>) -> Self {
        match result {
            Ok(t) => Self::Controlled(t),
            Err(err) => Self::Erratic(err),
        }
    }
}
impl<T> From<Result<T, tokio::task::JoinError>> for ActorShutdown<T> {
    fn from(result: Result<T, tokio::task::JoinError>) -> Self {
        match result {
            Ok(t) => Self::Controlled(t),
            Err(err) => Self::Erratic(ErraticActorShutdown::from(err)),
        }
    }
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub enum ErraticActorShutdown {
    /// Actor task was cancelled
    #[display(fmt = "Actor task was cancelled")]
    Cancelled,

    /// Actor task panicked
    #[display(fmt = "Actor task panicked")]
    Panic,
}

impl From<tokio::task::JoinError> for ErraticActorShutdown {
    fn from(err: tokio::task::JoinError) -> Self {
        if err.is_cancelled() {
            return Self::Cancelled;
        }
        if err.is_panic() {
            return Self::Panic;
        }
        unreachable!()
    }
}
