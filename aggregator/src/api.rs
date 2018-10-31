use std::sync::Arc;
use std::thread;

use futures::future;
use serde_json;
use warp::{self, Filter, Future, Sink, Stream};

use common::aggregator::Message;

use super::ServiceState;

fn health(_state: Arc<ServiceState>) -> impl warp::Reply {
    warp::reply::json(&json!({}))
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let port = service_state.config.common.api_port.unwrap_or(9001);
        let service_state_1 = service_state.clone();
        let state = warp::any().map(move || service_state_1.clone());
        let health = warp::path("health")
            .and(warp::path::end())
            .and(warp::get2())
            .and(state.clone())
            .map(health);
        let ws_handler = warp::ws2().map(move |ws: warp::ws::Ws2| {
            let service_state = service_state.clone();
            ws.on_upgrade(move |websocket| {
                let (mut tx, rx) = websocket.split();
                let bus_rx = service_state.bus.lock().unwrap().add_rx();

                // TODO add a counter query parameter

                let full_status = service_state.full_status.read().unwrap().clone();
                if let Err(e) = tx.start_send(warp::ws::Message::text(
                    serde_json::to_string(&Message::FullStatus((*full_status).clone()))
                        .expect("could not serialize message"),
                )) {
                    debug!(
                        "Could not send Websocket message,\
                         other side probably closed the socket: {}",
                        e
                    );
                    let _ = tx.close();
                }

                // TODO make this async instead of spawning a thread for every client
                thread::spawn(move || {
                    for msg in bus_rx {
                        debug!("Waiting for WS message...");
                        if let Err(e) = tx.start_send(warp::ws::Message::text(
                            serde_json::to_string(&*msg).expect("could not serialize message"),
                        )) {
                            debug!(
                                "Could not send Websocket message,\
                                 other side probably closed the socket: {}",
                                e
                            );
                            let _ = tx.close();
                            break;
                        };
                    }
                    debug!("WS thread ending");
                });

                rx.for_each(|msg| {
                    trace!("Websocket message: {:?}", msg);

                    future::ok(())
                })
                .then(|r| {
                    if let Err(e) = r {
                        info!("Websocket closed with error: {}", e);
                    } else {
                        debug!("Websocket closed normally");
                    }
                    future::ok(())
                })
            })
        });
        let api = warp::path("api");
        let ws = api.and(warp::path::end()).and(ws_handler);
        let routes = health.or(ws);
        warp::serve(routes).run(([0, 0, 0, 0], port));
    })
}
