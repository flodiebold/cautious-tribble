use std::sync::Arc;
use std::thread;

use failure::Error;
use futures::future;
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
        let service_state_1 = service_state.clone();
        let state = warp::any().map(move || service_state_1.clone());
        let health = warp::path("health")
            .and(warp::path::end())
            .and(warp::get2())
            .and(state.clone())
            .map(health);
        let service_state_1 = service_state.clone();
        let ws_handler = warp::ws2().map(move |ws: warp::ws::Ws2| {
            let service_state = service_state_1.clone();
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
    let repo = repo::GitResourceRepo::open(
        &service_state.env.common.versions_checkout_path,
        service_state.env.common.versions_url.clone(),
    )?;

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
