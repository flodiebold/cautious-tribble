use std::sync::{atomic::Ordering, Arc};
use std::thread;

use failure::Error;
use futures::{future, sync::mpsc};
use log::{debug, info, trace};
use serde_derive::Deserialize;
use serde_json::json;
use warp::{self, Filter, Future, Rejection, Sink, Stream};

use common::aggregator::{EnvName, Message, ResourceId};
use common::repo::Id;

use super::ServiceState;

fn health(_state: Arc<ServiceState>) -> impl warp::Reply {
    warp::reply::json(&json!({}))
}

fn deploy(state: Arc<ServiceState>, body: DeploymentData) -> Result<impl warp::Reply, Rejection> {
    info!("deploy {:?}", body);
    // TODO this should be done by another thread...
    // TODO return commit ID
    let _result_commit = do_deploy(state, body).map_err(|e| warp::reject::custom(e.compat()))?;
    Ok(warp::reply())
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let port = service_state.env.api_port.unwrap_or(9001);
        let service_state_1 = Arc::clone(&service_state);
        let state = warp::any().map(move || Arc::clone(&service_state_1));
        let health = warp::path("health")
            .and(warp::path::end())
            .and(warp::get2())
            .and(state.clone())
            .map(health);
        let service_state_1 = service_state.clone();
        let ws_handler = warp::ws2().map(move |ws: warp::ws::Ws2| {
            let service_state = service_state_1.clone();
            ws.on_upgrade(move |websocket| {
                let (ws_tx, rx) = websocket.split();

                // Create a channel to allow different threads to send messages on the websocket
                let (tx, chan_rx) = mpsc::channel(20);

                let client_id = service_state.client_counter.fetch_add(1, Ordering::SeqCst);

                // Send messages from the channel to the websocket
                warp::spawn(
                    // hopefully we can use async/await soon...
                    chan_rx
                        .then(move |msg: Result<Message, ()>| {
                            let text = match serde_json::to_string(
                                &msg.expect("channel doesn't yield errors"),
                            ) {
                                Ok(t) => t,
                                Err(err) => {
                                    info!(
                                        "Error sending WebSocket message to {}: {}",
                                        client_id, err
                                    );
                                    return Ok(None);
                                }
                            };
                            Ok(Some(warp::ws::Message::text(text)))
                        })
                        .take_while(|o| Ok(o.is_some()))
                        .filter_map(|o| o)
                        .map_err::<warp::Error, _>(|_e: ()| unreachable!())
                        .forward(ws_tx)
                        .map_err(move |err| {
                            info!(
                                "WebSocket sender for {} closed with error: {}",
                                client_id, err
                            );
                        })
                        .map(|_| ()),
                );

                // TODO add a counter query parameter
                let full_status = service_state.full_status.read().unwrap().clone();
                warp::spawn(
                    tx.clone()
                        .send(Message::FullStatus((*full_status).clone()))
                        .map_err(move |err| {
                            info!(
                                "Could not send first message to WebSocket {}: {}",
                                client_id, err
                            );
                        })
                        .map(|_| ()),
                );

                {
                    let mut receivers = service_state.receivers.write().unwrap();
                    receivers.push((client_id, tx));
                }

                rx.for_each(move |msg| {
                    trace!("Websocket message for {}: {:?}", client_id, msg);

                    future::ok(())
                })
                .then(move |r| {
                    if let Err(e) = r {
                        info!("Websocket {} closed with error: {}", client_id, e);
                    } else {
                        debug!("Websocket {} closed normally", client_id);
                    }
                    // remove connection from the receivers
                    let mut receivers = service_state.receivers.write().unwrap();
                    receivers.retain(|(id, _)| *id != client_id);
                    future::ok(())
                })
            })
        });
        let api = warp::path("api");
        let ws = api.and(warp::path::end()).and(ws_handler);
        let deploy = api
            .and(warp::path("deploy"))
            .and(warp::path::end())
            .and(state.clone())
            .and(warp::body::json())
            .and_then(deploy);
        let ui = warp::fs::dir(
            service_state
                .env
                .ui_path
                .clone()
                .unwrap_or("/ui/dist".into()),
        );
        let routes = health.or(ws).or(deploy).or(ui);
        warp::serve(routes).run(([0, 0, 0, 0], port));
    })
}

// TODO move this stuff to a better place, and clean it up
use common::git::{self, TreeZipper};
use common::repo;

use git2::Signature;

#[derive(Debug, Deserialize)]
struct SingleDeploymentData {
    resource: ResourceId,
    version_id: Id,
    env: EnvName,
}

#[derive(Debug, Deserialize)]
struct DeploymentData {
    message: String,
    deployments: Vec<SingleDeploymentData>,
}

fn do_deploy(service_state: Arc<ServiceState>, data: DeploymentData) -> Result<Id, Error> {
    let repo = repo::GitResourceRepo::open(service_state.env.common.clone())?;

    let head_commit = repo.repo.find_commit(repo.head)?;
    let tree = head_commit.tree()?;
    let mut zip = TreeZipper::from(&repo.repo, tree.clone());
    for deployment in data.deployments {
        zip.descend(&deployment.env.0)?;
        zip.descend("version")?;

        zip.rebuild(|b| {
            // FIXME instead find the actual location of the version file for the resource
            b.insert(
                format!("{}.yaml", deployment.resource.0),
                repo::id_to_oid(deployment.version_id),
                0o100644,
            )?;
            Ok(())
        })?;

        zip.ascend()?;
        zip.ascend()?;
    }

    let new_tree = zip.into_inner().expect("new tree should not be None");

    if new_tree.id() == tree.id() {
        // nothing changed
        // TODO don't make a commit
    }

    let signature = Signature::now("DM Aggregator", "n/a")?;

    let message = data.message;

    let commit = repo.repo.commit(
        Some("refs/dm_head"),
        &signature,
        &signature,
        &message,
        &new_tree,
        &[&head_commit],
    )?;

    info!("Made commit {}. Pushing...", commit);

    git::push(&repo.repo, &service_state.env.common.versions_url)?;

    info!("Pushed.");

    Ok(repo::oid_to_id(commit))
}
