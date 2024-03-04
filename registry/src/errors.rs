use actix_web::{HttpResponse, ResponseError};
use log::error;
use serde::Serialize;
use thiserror::Error;

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Error)]
pub enum Errors {
    #[error("io error")]
    Io(#[from] std::io::Error),

    #[error("user yaml error")]
    UserYaml(serde_yaml::Error),

    #[error("reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[error("package name invalid")]
    PackageName(#[from] pesde::package_name::PackageNameValidationError),

    #[error("config error")]
    Config(#[from] pesde::index::ConfigError),

    #[error("create package version error")]
    CreatePackageVersion(#[from] pesde::index::CreatePackageVersionError),

    #[error("commit and push error")]
    CommitAndPush(#[from] pesde::index::CommitAndPushError),

    #[error("index package error")]
    IndexPackage(#[from] pesde::index::IndexPackageError),

    #[error("error parsing query")]
    QueryParser(#[from] tantivy::query::QueryParserError),
}

impl ResponseError for Errors {
    fn error_response(&self) -> HttpResponse {
        match self {
            Errors::UserYaml(_) | Errors::PackageName(_) | Errors::QueryParser(_) => {}
            Errors::CreatePackageVersion(err) => match err {
                pesde::index::CreatePackageVersionError::MissingScopeOwnership => {
                    return HttpResponse::Unauthorized().json(ErrorResponse {
                        error: "You do not have permission to publish this scope".to_string(),
                    });
                }
                _ => error!("{err:?}"),
            },
            err => {
                error!("{err:?}");
            }
        }

        match self {
            Errors::UserYaml(err) => HttpResponse::BadRequest().json(ErrorResponse {
                error: format!("Error parsing YAML file: {}", err.to_string()),
            }),
            Errors::PackageName(err) => HttpResponse::BadRequest().json(ErrorResponse {
                error: format!("Invalid package name: {}", err.to_string()),
            }),
            Errors::QueryParser(err) => HttpResponse::BadRequest().json(ErrorResponse {
                error: format!("Error parsing query: {}", err.to_string()),
            }),
            _ => HttpResponse::InternalServerError().finish(),
        }
    }
}
