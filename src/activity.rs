//! Activity interface

use hyper::client::connect::Connect;

use Github;
use stars::Stars;

pub struct Activity<C>
where
    C: Clone + Connect,
{
    github: Github<C>,
}

impl<C: Clone + Connect> Activity<C> {
    #[doc(hidden)]
    pub fn new(github: Github<C>) -> Self {
        Self { github }
    }
    /// return a reference to starring operations
    pub fn stars(&self) -> Stars<C> {
        Stars::new(self.github.clone())
    }
}
