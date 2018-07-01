//! Hubcaps provides a set of building blocks for interacting with the Github API
//!
//! # Examples
//!
//!  Typical use will require instantiation of a Github client. Which requires
//! a user agent string, set of `hubcaps::Credentials` and a tokio_core `Handle`.
//!
//! ```no_run
//! extern crate hubcaps;
//! extern crate hyper;
//! extern crate tokio_core;
//!
//! use tokio_core::reactor::Core;
//! use hubcaps::{Credentials, Github};
//!
//! fn main() {
//!   let mut core = Core::new().expect("reactor fail");
//!   let github = Github::new(
//!     String::from("user-agent-name"),
//!     Credentials::Token(
//!       String::from("personal-access-token")
//!     ),
//!     &core.handle()
//!   );
//! }
//! ```
//!
//! Github enterprise users will want to create a client with the
//! [Github#host](struct.Github.html#method.host) method
//!
//! Access to various services are provided via methods on instances of the `Github` type.
//!
//! The convention for executing operations typically looks like
//! `github.repo(.., ..).service().operation(OperationOptions)` where operation may be `create`,
//! `delete`, etc.
//!
//! Services and their types are packaged under their own module namespace.
//! A service interface will provide access to operations and operations may access options types
//! that define the various parameter options available for the operation. Most operation option
//! types expose `builder()` methods for a builder oriented style of constructing options.
//!
//! ## Entity listings
//!
//! Many of Github's APIs return a collection of entities with a common interface for supporting pagination
//! Hubcaps supports two types of interfaces for working with listings. `list(...)` interfaces return the first
//! ( often enough ) list of entities. Alternatively for listings that require > 30 items you may wish to
//! use the `iter(..)` variant which returns a `futures::Stream` over all entities in a paginated set.
//!
//! # Errors
//!
//! Operations typically result in a `hubcaps::Future` with an error type pinned to
//! [hubcaps::Error](errors/struct.Error.html).
//!
//! ## Rate Limiting
//!
//! A special note should be taken when accounting for Github's
//! [API Rate Limiting](https://developer.github.com/v3/rate_limit/)
//! A special case
//! [hubcaps::ErrorKind::RateLimit](errors/enum.ErrorKind.html#variant.RateLimit)
//! will be returned from api operations when the rate limit
//! associated with credentials has been exhausted. This type will include a reset
//! Duration to wait before making future requests.
//!
//! This crate uses the `log` crate's debug log interface to log x-rate-limit
//! headers received from Github.
//! If you are attempting to test your access patterns against
//! Github's rate limits, enable debug looking and look for "x-rate-limit"
//! log patterns sourced from this crate
//!
#![allow(missing_docs)] // todo: make this a deny eventually

#![feature(non_modrs_mods)]

#[macro_use]
extern crate failure;
extern crate futures;
extern crate hyper;
#[cfg(feature = "tls")]
extern crate hyper_tls;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio;
extern crate url;

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::{future, stream, IntoFuture, Stream as StdStream};
use hyper::{Client, Method, StatusCode};
#[cfg(feature = "tls")]
use hyper_tls::HttpsConnector;
use serde::de::DeserializeOwned;
use tokio::reactor::Handle;
use url::Url;

pub mod activity;
pub mod branches;
pub mod comments;
pub mod deployments;
pub mod errors;
pub mod gists;
pub mod git;
pub mod hooks;
pub mod issues;
pub mod keys;
pub mod labels;
pub mod organizations;
pub mod pull_commits;
pub mod pulls;
pub mod rate_limit;
pub mod releases;
pub mod repositories;
pub mod review_comments;
pub mod search;
pub mod stars;
pub mod statuses;
pub mod teams;
pub mod users;

pub use errors::Error;

use activity::Activity;
use gists::{Gists, UserGists};
use organizations::{Organization, Organizations, UserOrganizations};
use rate_limit::RateLimit;
use repositories::{OrganizationRepositories, Repositories, Repository, UserRepositories};
use search::Search;
use users::Users;

const DEFAULT_HOST: &str = "https://api.github.com";

/// A type alias for `Futures` that may return `hubcaps::Errors`
pub struct Future<T>;
impl<T> futures::Future for Future<T> {
    type Item = T;
    type Error = errors::Error;
}

/// A type alias for `Streams` that may result in `hubcaps::Errors`
pub type Stream<T> = Box<StdStream<Item = T, Error = Error>>;

const HEADER_X_GITHUB_REQUEST_ID:&'static str = "X-GitHub-Request-Id";
const HEADER_X_RATELIMIT_LIMIT:&'static str = "X-RateLimit-Limit";
const HEADER_X_RATELIMIT_REMAINING:&'static str = "X-RateLimit-Remaining";
const HEADER_X_RATELIMIT_RESET:&'static str = "X-RateLimit-Reset";

const CONTENT_TYPE_APPLICATION_JSON:&'static str = "application/json";

/// enum representation of Github list sorting options
#[derive(Clone, Debug, PartialEq)]
pub enum SortDirection {
    /// Sort in ascending order (the default)
    Asc,
    /// Sort in descending order
    Desc,
}

impl fmt::Display for SortDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        }.fmt(f)
    }
}

impl Default for SortDirection {
    fn default() -> SortDirection {
        SortDirection::Asc
    }
}

/// Various forms of authentication credentials supported by Github
#[derive(Debug, PartialEq, Clone)]
pub enum Credentials {
    /// Oauth token string
    /// https://developer.github.com/v3/#oauth2-token-sent-in-a-header
    Token(String),
    /// Oauth client id and secret
    /// https://developer.github.com/v3/#oauth2-keysecret
    Client(String, String),
}

/// Entry point interface for interacting with Github API
#[derive(Clone, Debug)]
pub struct Github<C>
where
    C: Clone + hyper::client::connect::Connect,
{
    host: String,
    agent: String,
    client: Client<C>,
    credentials: Option<Credentials>,
}

#[cfg(feature = "tls")]
impl Github<HttpsConnector<hyper::client::connect::HttpConnector>> {
    pub fn new<A, C>(agent: A, credentials: C, handle: &Handle) -> Self
    where
        A: Into<String>,
        C: Into<Option<Credentials>>,
    {
        Self::host(DEFAULT_HOST, agent, credentials, handle)
    }

    pub fn host<H, A, C>(host: H, agent: A, credentials: C, handle: &Handle) -> Self
    where
        H: Into<String>,
        A: Into<String>,
        C: Into<Option<Credentials>>,
    {
        let connector = HttpsConnector::new(4, handle).unwrap();
        let http = Client::configure()
            .connector(connector)
            .keep_alive(true)
            .build(handle);
        Self::custom(host, agent, credentials, http)
    }
}

impl<C> Github<C>
where
    C: Clone + hyper::client::connect::Connect,
{
    pub fn custom<H, A, CR>(host: H, agent: A, credentials: CR, http: Client<C>) -> Self
    where
        H: Into<String>,
        A: Into<String>,
        CR: Into<Option<Credentials>>,
    {
        Self {
            host: host.into(),
            agent: agent.into(),
            client: http,
            credentials: credentials.into(),
        }
    }

    pub fn rate_limit(&self) -> RateLimit<C> {
        RateLimit::new(self.clone())
    }

    /// Return a reference to user activity
    pub fn activity(&self) -> Activity<C> {
        Activity::new(self.clone())
    }

    /// Return a reference to a Github repository
    pub fn repo<O, R>(&self, owner: O, repo: R) -> Repository<C>
    where
        O: Into<String>,
        R: Into<String>,
    {
        Repository::new(self.clone(), owner, repo)
    }

    /// Return a reference to the collection of repositories owned by and
    /// associated with an owner
    pub fn user_repos<S>(&self, owner: S) -> UserRepositories<C>
    where
        S: Into<String>,
    {
        UserRepositories::new(self.clone(), owner)
    }

    /// Return a reference to the collection of repositories owned by the user
    /// associated with the current authentication credentials
    pub fn repos(&self) -> Repositories<C> {
        Repositories::new(self.clone())
    }

    pub fn org<O>(&self, org: O) -> Organization<C>
    where
        O: Into<String>,
    {
        Organization::new(self.clone(), org)
    }

    /// Return a reference to the collection of organizations that the user
    /// associated with the current authentication credentials is in
    pub fn orgs(&self) -> Organizations<C> {
        Organizations::new(self.clone())
    }

    /// Return a reference to an interface that provides access
    /// to user information.
    pub fn users(&self) -> Users<C> {
        Users::new(self.clone())
    }

    /// Return a reference to the collection of organizations a user
    /// is publicly associated with
    pub fn user_orgs<U>(&self, user: U) -> UserOrganizations<C>
    where
        U: Into<String>,
    {
        UserOrganizations::new(self.clone(), user)
    }

    /// Return a reference to an interface that provides access to a user's gists
    pub fn user_gists<O>(&self, owner: O) -> UserGists<C>
    where
        O: Into<String>,
    {
        UserGists::new(self.clone(), owner)
    }

    /// Return a reference to an interface that provides access to the
    /// gists belonging to the owner of the token used to configure this client
    pub fn gists(&self) -> Gists<C> {
        Gists::new(self.clone())
    }

    /// Return a reference to an interface that provides access to search operations
    pub fn search(&self) -> Search<C> {
        Search::new(self.clone())
    }

    /// Return a reference to the collection of repositories owned by and
    /// associated with an organization
    pub fn org_repos<O>(&self, org: O) -> OrganizationRepositories<C>
    where
        O: Into<String>,
    {
        OrganizationRepositories::new(self.clone(), org)
    }

    fn request<Out, M>(
        &self,
        method: Method,
        uri: String,
        optional_body: Option<&M>,
    ) -> Future<(Option<Link>, Out)>
    where
        Out: DeserializeOwned,
        M: serde::ser::Serialize + Sized,
    {
        let url = if let Some(Credentials::Client(ref id, ref secret)) = self.credentials {
            let mut parsed = Url::parse(&uri).unwrap();
            parsed
                .query_pairs_mut()
                .append_pair("client_id", id)
                .append_pair("client_secret", secret);
            parsed.to_string().parse().into_future()
        } else {
            uri.parse().into_future()
        };

        let instance = self.clone();
        let response = url.map_err(Error::from).and_then(move |url| {
            let mut req = Request::new(method, url);
            {
                let headers = req.headers_mut();
                headers.set(hyper::header::USER_AGENT, instance.agent.clone());
                headers.set(hyper::header::CONTENT_TYPE, CONTENT_TYPE_APPLICATION_JSON);
                if let Some(Credentials::Token(ref token)) = instance.credentials {
                    headers.set(Authorization(format!("token {}", token)))
                }
            }

            if let Some(body) = optional_body {
                let body_str = to_json_str(body)?;
                req.set_body(body_str)
            }
            instance.client.request(req).map_err(Error::from)
        });

        Box::new(response.and_then(move |response| {
            let headers = response.headers();

            for value in headers.get_all(&HEADER_X_GITHUB_REQUEST_ID) {
                debug!("x-github-request-id: {}", value);
            }
            for value in headers.get_all(&HEADER_X_RATELIMIT_LIMIT) {
                debug!("x-rate-limit-limit: {}", value.0);
            }

            let remaining = headers.get(HEADER_X_RATELIMIT_REMAINING).map(|val| val.0);
            let reset = response.headers().get_all(&HEADER_X_RATELIMIT_RESET).map(|val| val.0);

            for value in remaining {
                debug!("x-rate-limit-remaining: {}", value)
            }
            for value in reset {
                debug!("x-rate-limit-reset: {}", value)
            }

            let status = response.status();
            // handle redirect common with renamed repos
            if StatusCode::MovedPermanently == status || StatusCode::TemporaryRedirect == status {
                if let Some(location) = response.headers().get::<Location>() {
                    debug!("redirect location {:?}", location);
                    return self.request(method, location.to_string(), optional_body);
                }
            }

            let link = response.headers().get::<Link>().map(|l| l.clone());
            response.body().concat2().map_err(Error::from).and_then(
                move |response_body| {
                    if status.is_success() {
                        debug!(
                            "response payload {}",
                            String::from_utf8_lossy(&response_body)
                        );
                        serde_json::from_slice::<Out>(&response_body)
                            .map(|out| (link, out))
                            .map_err(|err| Error::Deserialisation(err))
                    } else {
                        let error = match (remaining, reset) {
                            (Some(remaining), Some(reset)) if remaining == 0 => {
                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();

                                Error::RateLimit {
                                    reset_seconds: (reset as u64 - now),
                                }
                            },
                            _ => Error::Fault {
                                code: status,
                                error: serde_json::from_slice(&response_body)?,
                            },
                        };
                        Err(error.into())
                    }
                },
            )
        }))
    }

    fn request_entity<D, M>(
        &self,
        method: Method,
        uri: String,
        body: Option<&M>,
    ) -> Future<D>
    where
        D: DeserializeOwned + 'static,
        M: serde::ser::Serialize + Sized,
    {
        self.request(method, uri, body)
            .map(|(_, entity)| entity)
    }

    fn get<D>(&self, uri: &str) -> Future<D>
    where
        D: DeserializeOwned + 'static,
    {
        self.request_entity(Method::Get, self.host.clone() + uri, None)
    }

    fn get_pages<D>(&self, uri: &str) -> Future<(Option<Link>, D)>
    where
        D: DeserializeOwned + 'static,
    {
        self.request(Method::Get, self.host.clone() + uri, None)
    }

    fn delete(&self, uri: &str) -> Future<()> {
        self.request_entity::<(), Option<()>>(
            Method::Delete,
            self.host.clone() + uri,
            None,
        ).or_else(|err| match err.kind() {
            Error::Deserialisation(_) => Ok(()),
            otherwise => Err(otherwise.into()),
        })
    }

    fn post<D, M>(&self, uri: &str, message: Option<&M>) -> Future<D>
    where
        D: DeserializeOwned + 'static,
        M: serde::ser::Serialize + Sized,
    {
        self.request_entity(
            Method::Post,
            self.host.clone() + uri,
            Some(message),
        )
    }

    fn patch_media<D, M>(&self, uri: &str, message: Option<&M>) -> Future<D>
    where
        D: DeserializeOwned + 'static,
        M: serde::ser::Serialize + Sized,
    {
        self.request_entity(Method::Patch, self.host.clone() + uri, Some(message))
    }

    fn patch<D, M>(&self, uri: &str, message: Option<&M>) -> Future<D>
    where
        D: DeserializeOwned + 'static,
        M: serde::ser::Serialize + Sized,
    {
        self.patch_media(uri, message, MediaType::Json)
    }

    fn put_no_response<M>(&self, uri: &str, message: Option<&M>) -> Future<()>
    where
        M: serde::ser::Serialize + Sized,
    {
        self.put(uri, message).or_else(|err| match err.kind() {
            Error::Deserialisation(_) => Ok(()),
            err => Err(err.into()),
        })
    }

    fn put<D, M>(&self, uri: &str, message: Option<&M>) -> Future<D>
    where
        D: DeserializeOwned + 'static,
        M: serde::ser::Serialize + Sized,
    {
        self.request_entity(
            Method::Put,
            self.host.clone() + uri,
            Some(message),
            MediaType::Json,
        )
    }
}

fn next_link(l: Link) -> Option<String> {
    l.values()
        .into_iter()
        .find(|v| v.rel().unwrap_or(&[]).get(0) == Some(&RelationType::Next))
        .map(|v| v.link().to_owned())
}

/// "unfold" paginated results of a list of github entities
fn unfold<C, D, I>(
    github: Github<C>,
    first: Future<(Option<Link>, D)>,
    into_items: fn(D) -> Vec<I>,
) -> Stream<I>
where
    D: DeserializeOwned + 'static,
    I: 'static,
    C: Clone + Connect,
{
    first
        .map(move |(link, payload)| {
            let mut items = into_items(payload);
            items.reverse();
            stream::unfold::<_, _, Future<(I, (Option<Link>, Vec<I>))>, _>(
                (link, items),
                move |(link, mut items)| match items.pop() {
                    Some(item) => Some(Box::new(future::ok((item, (link, items))))),
                    _ => link.and_then(next_link).map(|url| {
                        let url = Url::parse(&url).unwrap();
                        let uri = [url.path(), url.query().unwrap_or_default()].join("?");
                        Box::new(github.get_pages(uri.as_ref()).map(move |(link, payload)| {
                            let mut items = into_items(payload);
                            items.reverse();
                            (items.remove(0), (link, items))
                        })) as Future<(I, (Option<Link>, Vec<I>))>
                    }),
                },
            )
        })
        .into_stream()
        .flatten()
}

pub fn to_json_str<T>( input: &T ) -> Result<String, Error>
where T: serde::ser::Serialize + Sized {
    serde_json::to_string(input).map_err(|err| Error::Deserialisation(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sort_direction() {
        let default: SortDirection = Default::default();
        assert_eq!(default, SortDirection::Asc)
    }
}
