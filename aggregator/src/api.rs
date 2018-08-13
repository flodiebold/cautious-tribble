use std::sync::Arc;
use std::thread;

use futures::future;
use serde_json;
use warp::{self, ws::Message, Filter, Future, Sink, Stream};

use super::ServiceState;

fn health(_state: &ServiceState) -> impl warp::Reply {
    warp::reply::json(&json!({}))
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let port = service_state.config.common.api_port.unwrap_or(9001);
        let service_state_1 = service_state.clone();
        let state = warp::any().map(move || &*service_state_1);
        let health = warp::get(warp::path("health").and(warp::index()))
            .and(state.clone())
            .map(health);
        let ws_handler = warp::ws(move |websocket| {
            let (tx, rx) = websocket.split();
            let bus_rx = service_state.bus.lock().unwrap().add_rx();

            // TODO make this async instead of spawning a thread for every client
            thread::spawn(move || {
                let mut tx = tx;
                for msg in bus_rx {
                    if let Err(e) = tx.start_send(Message::text(
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
            });

            rx.for_each(|msg| {
                trace!("Websocket message: {:?}", msg);

                future::ok(())
            }).then(|r| {
                if let Err(e) = r {
                    info!("Websocket closed with error: {}", e);
                } else {
                    debug!("Websocket closed normally");
                }
                future::ok(())
            })
        });
        let ws = warp::index().and(ws_handler);
        let routes = health.or(ws);
        warp::serve(routes).run(([0, 0, 0, 0], port));
    })
}
