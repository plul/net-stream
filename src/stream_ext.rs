pub trait StreamExt: futures::Stream + Sized {
    /// Chains a single ready item to the end of the stream.
    fn chain_ready<T>(self, item: T) -> futures::stream::Chain<Self, futures::stream::Once<std::future::Ready<T>>>
    where
        Self: futures::Stream<Item = T>,
    {
        self.chain_future(std::future::ready(item))
    }

    /// Chains a single future to the end of the stream.
    fn chain_future<T, F>(self, fut: F) -> futures::stream::Chain<Self, futures::stream::Once<F>>
    where
        Self: futures::Stream<Item = T>,
        F: core::future::Future<Output = T>,
    {
        futures::StreamExt::chain(self, futures::stream::once(fut))
    }
}

impl<T> StreamExt for T where T: futures::Stream {}
