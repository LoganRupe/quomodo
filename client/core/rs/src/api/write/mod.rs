mod action;
mod alert;
mod alerter;
mod api_key;
mod onboarding_key;
mod permissions;
mod procedure;
mod provider;
mod repo;
mod resource;
mod server;
mod stack;
mod sync;
mod tags;
mod user;
mod user_group;

pub use action::*;
pub use alert::*;
pub use alerter::*;
pub use api_key::*;
pub use onboarding_key::*;
pub use permissions::*;
pub use procedure::*;
pub use provider::*;
pub use repo::*;
pub use resource::*;
pub use server::*;
pub use stack::*;
pub use sync::*;
pub use tags::*;
pub use user::*;
pub use user_group::*;

#[cfg(feature = "utoipa")]
pub mod openapi;

pub trait KomodoWriteRequest: mogh_resolver::HasResponse {}
