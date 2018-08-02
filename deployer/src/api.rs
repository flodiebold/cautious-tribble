use std::sync::Arc;
use std::thread;

use warp::{self, Filter};

use super::ServiceState;

fn health(_state: Arc<ServiceState>) -> impl warp::Reply {
    warp::reply::json(&())
}

fn status(state: Arc<ServiceState>) -> impl warp::Reply {
    let latest_status = state.latest_status.get();
    warp::reply::json(&*latest_status)
}

pub fn start(service_state: Arc<ServiceState>) {
    thread::spawn(move || {
        let port = service_state.config.common.api_port.unwrap_or(9001);
        let state = warp::any().map(move || service_state.clone());
        let health = warp::get(warp::path("health").and(warp::index()))
            .and(state.clone())
            .map(health);
        let status = warp::get(warp::path("status").and(warp::index()))
            .and(state)
            .map(status);
        let routes = health.or(status);
        warp::serve(routes).run(([0, 0, 0, 0], port));
    });
}
