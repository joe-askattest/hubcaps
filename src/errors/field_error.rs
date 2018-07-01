
#[derive(Debug, Deserialize, PartialEq)]
pub struct FieldError {
    pub resource: String,
    pub field: Option<String>,
    pub code: String,
    pub message: Option<String>,
    pub documentation_url: Option<String>,
}