use std::sync::{atomic::Ordering, Arc};
use std::thread;

use failure::Error;
use futures::{channel::mpsc, future};
use futures_util::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use log::{debug, info, trace};
use serde_derive::Deserialize;
use serde_json::json;
use tokio::runtime::Runtime;
use warp::{self, ws::WebSocket, Filter, Future, Rejection};

use common::aggregator::{EnvName, Message, ResourceId};
use common::repo::Id;

use super::ServiceState;

fn health(_state: Arc<ServiceState>) -> impl warp::Reply {
    warp::reply::json(&json!({}))
}

#[derive(Debug)]
struct DeployError(failure::Error);
impl warp::reject::Reject for DeployError {}

async fn deploy(
    state: Arc<ServiceState>,
    body: DeploymentData,
) -> Result<impl warp::Reply, Rejection> {
    info!("deploy {:?}", body);
    // TODO this should be done by another thread...
    // TODO return commit ID
    let _result_commit =
        do_deploy(state, body).map_err(|e| warp::reject::custom(DeployError(e)))?;
    Ok(warp::reply())
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let rt = Runtime::new().expect("Could not create runtime");
        let port = service_state.env.api_port.unwrap_or(9001);
        let service_state_1 = Arc::clone(&service_state);
        let state = warp::any().map(move || Arc::clone(&service_state_1));
        let health = warp::path("health")
            .and(warp::path::end())
            .and(warp::get())
            .and(state.clone())
            .map(health);
        let service_state_1 = service_state.clone();
        let ws_handler = warp::ws().map(move |ws: warp::ws::Ws| {
            let service_state = service_state_1.clone();
            ws.on_upgrade(move |websocket| user_connected(websocket, service_state))
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
        rt.spawn(warp::serve(routes).run(([0, 0, 0, 0], port)));

        rt.shutdown_on_idle();
    })
}

fn user_connected(
    websocket: WebSocket,
    service_state: Arc<ServiceState>,
) -> impl Future<Output = ()> {
    let (mut ws_tx, rx) = websocket.split();

    // Create a channel to allow different threads to send messages on the websocket
    let (tx, mut chan_rx) = mpsc::channel(20);

    let client_id = service_state.client_counter.fetch_add(1, Ordering::SeqCst);

    // Send messages from the channel to the websocket
    warp::spawn(
        async move {
            while let Some(msg) = chan_rx.next().await {
                let text = match serde_json::to_string(&msg) {
                    Ok(t) => t,
                    Err(err) => {
                        info!("Error sending WebSocket message to {}: {}", client_id, err);
                        continue;
                    }
                };

                if let Err(err) = ws_tx.send(warp::ws::Message::text(text)).await {
                    info!(
                        "WebSocket sender for {} closed with error: {}",
                        client_id, err
                    );
                    break;
                }
            }
        }, /*
           // hopefully we can use async/await soon...
           chan_rx
               .then(move |msg: Result<Message, ()>| {
                   Ok(Some(warp::ws::Message::text(text)))
               })
               .take_while(|o| Ok(o.is_some()))
               .filter_map(|o| o)
               .map_err::<warp::Error, _>(|_e: ()| unreachable!())
               .forward(ws_tx)
               .map_err(move |err| {
               })
               .map(|_| ()),
                   */
    );

    // TODO add a counter query parameter
    let full_status = service_state.full_status.read().unwrap().clone();
    let tx2 = tx.clone();
    warp::spawn(async move {
        if let Err(err) = tx2
            .clone()
            .send(Message::FullStatus((*full_status).clone()))
            .await
        {
            info!(
                "Could not send first message to WebSocket {}: {}",
                client_id, err
            );
        }
    });

    {
        let mut receivers = service_state.receivers.write().unwrap();
        receivers.push((client_id, tx));
    }

    rx.try_for_each(move |msg| {
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
        future::ready(())
    })
}

// TODO move this stuff to a better place, and clean it up
use common::git::{self, TreeZipper};
use common::repo;
use common::transitions::{Lock, Locks};

use failure::ResultExt;
use git2::Signature;

#[derive(Debug, Deserialize)]
struct ResourceDeploymentData {
    resource: ResourceId,
    version_id: Option<Id>,
    locked: Option<bool>,
    env: EnvName,
}

#[derive(Debug, Deserialize)]
struct DeploymentData {
    message: String,
    resources: Vec<ResourceDeploymentData>,
}

fn do_deploy(service_state: Arc<ServiceState>, data: DeploymentData) -> Result<Id, Error> {
    let repo = repo::GitResourceRepo::open(service_state.env.common.clone())?;

    let head_commit = repo.repo.find_commit(repo.head)?;
    let tree = head_commit.tree()?;
    let mut zip = TreeZipper::from(&repo.repo, tree.clone());
    for deployment in data.resources {
        zip.descend(&deployment.env.0)?;

        if let Some(version_id) = deployment.version_id {
            zip.descend("version")?;

            zip.rebuild(|b| {
                // FIXME instead find the actual location of the version file for the resource
                b.insert(
                    format!("{}.yaml", deployment.resource.0),
                    repo::id_to_oid(version_id),
                    0o100644,
                )?;
                Ok(())
            })?;

            zip.ascend()?;
        }

        if let Some(locked) = deployment.locked {
            update_locks(&mut zip, &repo.repo, &deployment.env, |locks| {
                if locked {
                    // lock
                    locks
                        .resource_locks
                        .entry(deployment.resource.0.clone())
                        .or_insert_with(Lock::default)
                        .add_reason("locked from ui");
                } else {
                    // unlock
                    if let Some(locks) = locks.resource_locks.get_mut(&deployment.resource.0) {
                        locks.remove_reason("locked from ui");
                    }
                }
            })?;
        }

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

fn update_locks<'repo>(
    tree: &mut TreeZipper<'repo>,
    repo: &'repo git2::Repository,
    env: &EnvName,
    mut f: impl FnMut(&mut Locks),
) -> Result<(), Error> {
    let mut locks = if let Some(blob) = tree.get_blob("locks.yaml")? {
        serde_yaml::from_slice(blob.content())
            .with_context(|_| format!("deserializing locks.yaml for env {} failed", env.0))?
    } else {
        Locks::default()
    };

    f(&mut locks);
    // TODO clean locks (remove resources locks that are empty)

    let mut serialized = serde_yaml::to_vec(&locks).context("serializing locks file failed")?;
    serialized.extend("\n".as_bytes());

    let blob = repo.blob(&serialized).context("writing blob failed")?;

    tree.rebuild(|builder| {
        builder
            .insert("locks.yaml", blob, 0o100644)
            .context("updating locks file failed")?;
        Ok(())
    })?;

    Ok(())
}
