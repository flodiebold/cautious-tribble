use gotham::handler::HandlerFuture;
use std::io;
use std::sync::Arc;

use gotham;
use gotham::handler::{Handler, IntoHandlerFuture, NewHandler};
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

#[derive(Clone)]
struct StatusResource(Arc<ServiceState>);

impl NewHandler for StatusResource {
    type Instance = Self;

    fn new_handler(&self) -> io::Result<Self> {
        Ok(self.clone())
    }
}

impl Handler for StatusResource {
    fn handle(self, state: State) -> Box<HandlerFuture> {
        let latest_status = self.0.latest_status.get();
        let res = response::create_response(
            &state,
            StatusCode::Ok,
            Some((
                serde_json::to_string(&*latest_status)
                    .expect("serialized status")
                    .into_bytes(),
                mime::APPLICATION_JSON,
            )),
        );
        (state, res).into_handler_future()
    }
}

fn router(service_state: Arc<ServiceState>) -> Router {
    build_simple_router(|route| {
        route.get("/health").to(health);
        route
            .get("/status")
            .to_new_handler(StatusResource(service_state.clone()));
    })
}

pub fn start(service_state: Arc<ServiceState>) {
    gotham::start("0.0.0.0:9001", router(service_state));
}
