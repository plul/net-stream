pub trait StreamExt: futures::Stream + Sized {
    /// Chain stream with a terminator.
    fn chain_once<T>(self, item: T) -> futures::stream::Chain<Self, futures::stream::Once<std::future::Ready<T>>>
    where
        Self: futures::Stream<Item = T>,
    {
        futures::StreamExt::chain(self, futures::stream::once(std::future::ready(item)))
    }
}

impl<T> StreamExt for T where T: futures::Stream {}
