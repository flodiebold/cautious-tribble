use std::thread;
use std::io;
use std::sync::Arc;

use gotham;
use gotham::handler::{HandlerFuture, Handler, IntoHandlerFuture, NewHandler};
use gotham::http::response;
use gotham::router::{Router, builder::{build_simple_router, DefineSingleRoute, DrawRoutes}};
use gotham::state::State;
use hyper::{Response, StatusCode};
use mime;
use serde_json;

use super::ServiceState;

fn health(state: State) -> (State, Response) {
    let res = response::create_response(
        &state,
        StatusCode::Ok,
        Some((String::from("{}").into_bytes(), mime::APPLICATION_JSON)),
    );
    (state, res)
}

fn router(service_state: Arc<ServiceState>) -> Router {
    build_simple_router(|route| {
        route.get("/health").to(health);
    })
}

pub fn start(service_state: Arc<ServiceState>) {
    thread::spawn(move || {
        let port = service_state.config.common.api_port.unwrap_or(9001);
        gotham::start(("0.0.0.0", port), router(service_state));
    });
}
