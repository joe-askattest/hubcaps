//! Stars interface

use hyper::client::connect::Connect;
use hyper::StatusCode;
use futures::Future as StdFuture;

use {Error, Future, Github};

pub struct Stars<C>
where
    C: Clone + Connect,
{
    github: Github<C>,
}

impl<C: Clone + Connect> Stars<C> {
    #[doc(hidden)]
    pub fn new(github: Github<C>) -> Self {
        Self { github }
    }

    /// Returns whether or not authenticated user has started a repo
    pub fn is_starred<O, R>(&self, owner: O, repo: R) -> Future<bool>
    where
        O: Into<String>,
        R: Into<String>,
    {
        self.github
            .get::<()>(&format!("/user/starred/{}/{}", owner.into(), repo.into()))
            .map(|_| true)
            .or_else(|err| match err {
                Error::Fault {
                    code: StatusCode::NotFound,
                    ..
                } => Ok(false),
                Error::Deserialisation(_) => Ok(true),
                _ => err.into(),
            })
    }

    /// star a repo
    pub fn star<O, R>(&self, owner: O, repo: R) -> Future<()>
    where
        O: Into<String>,
        R: Into<String>,
    {
        self.github.put_no_response(
            &format!("/user/starred/{}/{}", owner.into(), repo.into()),
            None,
        )
    }

    /// unstar a repo
    pub fn unstar<O, R>(&self, owner: O, repo: R) -> Future<()>
    where
        O: Into<String>,
        R: Into<String>,
    {
        self.github
            .delete(&format!("/user/starred/{}/{}", owner.into(), repo.into()))
    }
}
