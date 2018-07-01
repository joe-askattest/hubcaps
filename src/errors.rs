//! Client errors

use hyper;
use hyper::StatusCode;
use serde_json;

use std::io;

mod client_error;
mod field_error;

pub use self::client_error::ClientError;
pub use self::field_error::FieldError;

#[derive(Debug, Fail)]
pub enum Error {
    // Client side error returned for faulty requests
    #[fail(display="{}: '{}'", code, error)]
    Fault {
        code: StatusCode,
        error: ClientError,
    },

    // Error returned when a credential's rate limit has been exhausted. Wait for the reset duration before issuing more requests
    #[fail(display="Rate limit exhausted. Will reset in {} seconds", reset_seconds)]
    RateLimit {
        reset_seconds: u64,
    },

    #[fail(display="Deserialisation Error, {}", _0)]
    Deserialisation(serde_json::error::Error),

    #[fail(display="Http Error, {}", _0)]
    Http(hyper::Error),

    #[fail(display="IO Error, {}", _0)]
    IO(io::Error),
}

#[cfg(test)]
mod tests {
    use serde_json;
    use super::{ClientError,  FieldError};

    #[test]
    fn deserialize_client_field_errors() {
        for (json, expect) in vec![
            // see https://github.com/softprops/hubcaps/issues/31
            (
                r#"{"message": "Validation Failed","errors":
                [{
                    "resource": "Release",
                    "code": "custom",
                    "message": "Published releases must have a valid tag"
                }]}"#,
                ClientError {
                    message: "Validation Failed".to_owned(),
                    errors: Some(vec![
                        FieldError {
                            resource: "Release".to_owned(),
                            code: "custom".to_owned(),
                            field: None,
                            message: Some(
                                "Published releases \
                                 must have a valid tag"
                                    .to_owned(),
                            ),
                            documentation_url: None,
                        },
                    ]),
                },
            ),
        ] {
            assert_eq!(serde_json::from_str::<ClientError>(json).unwrap(), expect);
        }
    }
}
