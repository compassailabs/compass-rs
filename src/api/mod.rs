pub mod balance;
pub mod chat;
pub mod debug;
pub mod earnings;
pub mod error;
pub mod funded;
pub mod health;
pub mod markets;
pub mod policy;
pub mod position;
pub mod recover;
pub mod send_to_wallet;
pub mod session;
pub mod strategy;
pub mod withdraw;

use axum::Router;

use crate::state::AppState;

pub fn router(enable_debug_api: bool) -> Router<AppState> {
    let mut r = Router::new()
        .merge(health::router())
        .merge(strategy::router())
        .merge(policy::router())
        .merge(chat::router())
        .merge(session::router())
        .merge(balance::router())
        .merge(earnings::router())
        .merge(funded::router())
        .merge(markets::router())
        .merge(position::router())
        .merge(send_to_wallet::router())
        .merge(withdraw::router());

    if enable_debug_api {
        r = r.merge(debug::router()).merge(recover::router());
    }
    r
}
