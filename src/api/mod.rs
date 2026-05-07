pub mod handlers;
pub mod routes;

pub mod server {
    pub use super::routes::run;
}

pub use routes::{routes, run, AppState};
