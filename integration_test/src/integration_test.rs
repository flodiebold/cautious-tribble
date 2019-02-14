use std;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use failure::Error;
use git2;
use hyper;
use indexmap::IndexMap;
use nix;
use rand::{self, Rng};
use reqwest;
use tempfile;
use websocket;

use git_fixture;

use common::deployment::{AllDeployerStatus, RolloutStatus};
use common::repo::oid_to_id;
use common::transitions::TransitionStatusInfo;

pub struct IntegrationTest {
    executable_root: PathBuf,
    dir: tempfile::TempDir,
    processes: Vec<(TestService, Child)>,
    ports: HashMap<TestService, u16>,
    suffix: String,
    created_namespaces: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum TestService {
    Deployer,
    Transitioner,
    Aggregator,
}

fn executable_root() -> PathBuf {
    let mut root = env::current_exe()
        .unwrap()
        .parent()
        .expect("executable's directory")
        .to_path_buf();
    if root.ends_with("deps") {
        root.pop();
    }
    root
}

fn rand_chars(n: usize) -> String {
    let mut rng = rand::thread_rng();
    rng.sample_iter(&rand::distributions::Alphanumeric)
        .take(n)
        .collect::<String>()
        .to_lowercase()
}

impl IntegrationTest {
    pub fn new() -> IntegrationTest {
        let root = executable_root();
        let dir = tempfile::Builder::new()
            .prefix("dm-integration-")
            .tempdir()
            .unwrap();
        eprintln!("temp dir for test: {:?}", dir.path());
        // copy kube config, ignore errors
        let _ = fs::copy(
            #[allow(deprecated)] // the edge cases of home_dir don't really matter here
            env::home_dir().unwrap().join(".kube/config"),
            dir.path().join("kube_config"),
        );
        let mut rng = rand::thread_rng();
        let mut ports = HashMap::new();
        let start = rng.gen_range(1024, std::u16::MAX - 3);
        ports.insert(TestService::Deployer, start);
        ports.insert(TestService::Transitioner, start + 1);
        ports.insert(TestService::Aggregator, start + 2);
        IntegrationTest {
            dir,
            executable_root: root,
            processes: Vec::new(),
            ports,
            suffix: rand_chars(5),
            created_namespaces: Vec::new(),
        }
    }

    pub fn new_playground() -> IntegrationTest {
        let root = executable_root();
        let dir = tempfile::Builder::new()
            .prefix("playground_")
            .rand_bytes(2)
            .tempdir_in("")
            .unwrap();
        eprintln!("temp dir for test: {:?}", dir.path());
        // copy kube config, ignore errors
        let _ = fs::copy(
            #[allow(deprecated)] // the edge cases of home_dir don't really matter here
            env::home_dir().unwrap().join(".kube/config"),
            dir.path().join("kube_config"),
        );
        let mut ports = HashMap::new();
        ports.insert(TestService::Deployer, 9001);
        ports.insert(TestService::Transitioner, 9002);
        ports.insert(TestService::Aggregator, 9003);
        IntegrationTest {
            dir,
            executable_root: root,
            processes: Vec::new(),
            ports,
            suffix: String::new(),
            created_namespaces: Vec::new(),
        }
    }

    fn versions_repo_path(&self) -> PathBuf {
        self.dir.path().join("versions.git")
    }

    fn project_path(&self) -> PathBuf {
        self.executable_root
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned()
    }

    pub fn git_fixture(&self, data: &str) -> git_fixture::RepoFixture {
        let data = self.adapt_config(data);
        let template = git_fixture::RepoTemplate::from_string(&data).unwrap();
        template.create_in(&self.versions_repo_path()).unwrap()
    }

    pub fn create_namespace(&mut self, namespace: &str) -> &mut Self {
        let namespace = format!("{}{}", namespace, self.suffix);
        let status = Command::new("kubectl")
            .args(&[
                "--kubeconfig",
                "./kube_config",
                "create",
                "namespace",
                &namespace,
            ])
            .current_dir(self.dir.path())
            .status()
            .unwrap();

        if !status.success() {
            panic!(
                "kubectl create namespace {} exited with code {}",
                namespace, status
            );
        }

        self.created_namespaces.push(namespace);

        self
    }

    pub fn kubectl(&mut self, namespace: &str, args: &[&str]) -> &mut Self {
        let namespace = format!("{}{}", namespace, self.suffix);
        let status = Command::new("kubectl")
            .args(&["--kubeconfig", "./kube_config", "--namespace", &namespace])
            .args(args)
            .current_dir(self.dir.path())
            .status()
            .unwrap();

        if !status.success() {
            panic!("kubectl exited with code {}", status);
        }

        self
    }

    pub fn kubectl_apply(&mut self, namespace: &str, content: &str) -> &mut Self {
        let namespace = format!("{}{}", namespace, self.suffix);
        let yaml_path = self.dir.path().join("kubectl_apply.yaml");
        {
            let mut file = File::create(&yaml_path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
            file.flush().unwrap();
        }
        let status = Command::new("kubectl")
            .args(&[
                "--kubeconfig",
                "./kube_config",
                "--namespace",
                &namespace,
                "apply",
                "-f",
            ])
            .arg(yaml_path)
            .current_dir(self.dir.path())
            .status()
            .unwrap();

        if !status.success() {
            panic!("kubectl apply exited with code {}", namespace);
        }

        self
    }

    fn get_port(&self, service: TestService) -> u16 {
        self.ports[&service]
    }

    fn adapt_config(&self, config: &str) -> String {
        config.replace("%%suffix%%", &self.suffix)
    }

    pub fn run_deployer(&mut self) -> &mut Self {
        let service = TestService::Deployer;
        let child = Command::new(self.executable_root.join("deployer"))
            .current_dir(self.dir.path())
            .env_clear()
            .env("RUST_LOG", "warn,deployer=debug")
            .env("RUST_BACKTRACE", "1")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("API_PORT", self.get_port(service).to_string())
            .env("VERSIONS_URL", "./versions.git")
            .env(
                "VERSIONS_CHECKOUT_PATH",
                format!("./versions_checkout_{:?}", service),
            )
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        self.processes.push((TestService::Deployer, child));
        self
    }

    pub fn run_transitioner(&mut self) -> &mut Self {
        let service = TestService::Transitioner;
        let child = Command::new(self.executable_root.join("transitioner"))
            .current_dir(self.dir.path())
            .env_clear()
            .env("RUST_LOG", "warn,transitioner=debug")
            .env("RUST_BACKTRACE", "1")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("API_PORT", self.get_port(service).to_string())
            .env("VERSIONS_URL", "./versions.git")
            .env(
                "VERSIONS_CHECKOUT_PATH",
                format!("./versions_checkout_{:?}", service),
            )
            .env(
                "DEPLOYER_URL",
                format!("http://localhost:{}", self.get_port(TestService::Deployer)),
            )
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        self.processes.push((TestService::Transitioner, child));
        self
    }

    pub fn run_aggregator(&mut self) -> &mut Self {
        let service = TestService::Aggregator;
        let child = Command::new(self.executable_root.join("aggregator"))
            .current_dir(self.dir.path())
            .env_clear()
            .env("RUST_LOG", "warn,aggregator=debug")
            .env("RUST_BACKTRACE", "1")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("UI_PATH", self.project_path().join("ui/dist"))
            .env("API_PORT", self.get_port(service).to_string())
            .env("VERSIONS_URL", "./versions.git")
            .env(
                "VERSIONS_CHECKOUT_PATH",
                format!("./versions_checkout_{:?}", service),
            )
            .env(
                "DEPLOYER_URL",
                format!("http://localhost:{}", self.get_port(TestService::Deployer)),
            )
            .env(
                "TRANSITIONER_URL",
                format!(
                    "http://localhost:{}",
                    self.get_port(TestService::Transitioner)
                ),
            )
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        self.processes.push((TestService::Aggregator, child));
        self
    }

    pub fn wait_ready(&mut self) -> &mut Self {
        for _ in 0..50 {
            eprintln!("checking health...");
            let ok =
                self.processes.iter().map(|(k, _)| k).all(|k| {
                    check_health(&format!("http://127.0.0.1:{}/health", self.get_port(*k)))
                });
            if ok {
                return self;
            }
            eprintln!("not ok");

            for (name, child) in &mut self.processes {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("Process {:?} exited with code {}", name, status);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        panic!("wait_ready timed out");
    }

    pub fn get_service_url(&self, namespace: &str, svc: &str) -> String {
        let mut namespace = namespace.to_owned();
        namespace.push_str(&self.suffix);
        let output = Command::new("minikube")
            .args(&[
                "service",
                "--url",
                "--interval",
                "1",
                "--wait",
                "60",
                "-n",
                &namespace,
                svc,
            ])
            .output()
            .expect("running minikube");

        if !output.status.success() {
            panic!(
                "minikube exited with code {} and output {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        eprintln!(
            "minikube stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    pub fn wait_env_rollout_done(&mut self, env: &str) -> &mut Self {
        for _ in 0..500 {
            eprintln!("checking rollout status...");
            let current_hash = self.versions_head_hash();

            if let Ok(status) = get_deployer_status(&format!(
                "http://127.0.0.1:{}/status",
                self.get_port(TestService::Deployer)
            )) {
                eprintln!("full env status: {:?}", status);
                if let Some(env_status) = status.deployers.get(env) {
                    if env_status.deployed_version != oid_to_id(current_hash) {
                        eprintln!(
                            "current version not yet deployed -- expecting {}, got {}",
                            current_hash, env_status.deployed_version
                        );
                    } else if env_status.rollout_status == RolloutStatus::Clean {
                        eprintln!("rollout status is {:?}!", env_status);
                        return self;
                    } else {
                        eprintln!("rollout status is {:?}...", env_status);
                    }
                } else {
                    eprintln!("env does not exist (yet)...");
                }
            } else {
                eprintln!("status request failed");
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        panic!("wait_ready timed out");
    }

    pub fn wait_transition(&mut self, transition: &str, count: usize) -> &mut Self {
        for _ in 0..500 {
            match get_transitioner_status(&format!(
                "http://127.0.0.1:{}/status",
                self.get_port(TestService::Transitioner)
            )) {
                Ok(status) => {
                    eprintln!("full transitioner status: {:?}", status);

                    let successful_transitions = status
                        .get(transition)
                        .map(|status| status.successful_runs.clone())
                        .unwrap_or_default();

                    if successful_transitions.len() >= count {
                        let run = &successful_transitions[successful_transitions.len() - count];
                        eprintln!(
                            "transition {} ran at {}, resulting in version {:?}",
                            transition, run.time, run.committed_version
                        );
                        return self;
                    }

                    // if the transition is failed, we could stop early here
                }
                Err(e) => {
                    eprintln!("transitioner status request failed: {}", e);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        panic!("wait_transitioner_commit timed out");
    }

    pub fn connect_to_aggregator_socket(&mut self) -> &mut Self {
        let url = format!(
            "ws://127.0.0.1:{}/api",
            self.get_port(TestService::Aggregator)
        );
        let client = websocket::ClientBuilder::new(&url)
            .expect("Aggregator websocket client creation failed")
            .connect_insecure()
            .expect("Aggregator websocket connect failed");

        std::thread::spawn(move || {
            let mut client = client;
            for msg in client.incoming_messages() {
                if let Ok(msg) = msg {
                    match msg {
                        websocket::OwnedMessage::Text(text) => {
                            eprintln!("Aggregator message: {}", text);
                        }
                        _ => {
                            eprintln!("Unknown aggregator message: {:?}", msg);
                        }
                    }
                } else {
                    break;
                }
            }
        });

        self
    }

    pub fn teardown_namespaces(&mut self) {
        if !should_teardown_namespaces() {
            return;
        }
        for namespace in self.created_namespaces.drain(..) {
            let _ = Command::new("kubectl")
                .args(&[
                    "--kubeconfig",
                    "./kube_config",
                    "delete",
                    "namespace",
                    &namespace,
                ])
                .current_dir(self.dir.path())
                .status();
        }
    }

    fn versions_head_hash(&self) -> git2::Oid {
        let repo = git2::Repository::open(self.versions_repo_path()).unwrap();
        repo.refname_to_id("refs/heads/master").unwrap()
    }

    pub fn finish(mut self) {
        self.teardown_namespaces();
        eprintln!("Stopping processes...");
        for (name, mut child) in self.processes.drain(..) {
            terminate_child(&child).unwrap();
            let status = child.wait().unwrap();
            // TODO implement proper signal handling in the services, then
            // remove the handling for signal 15 here
            if !status.success() && status.signal() != Some(15) {
                panic!("Process {:?} exited with code {}", name, status)
            }
        }
    }
}

impl Drop for IntegrationTest {
    fn drop(&mut self) {
        self.teardown_namespaces();
        for (_, mut child) in self.processes.drain(..) {
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
            }
        }
    }
}

fn get_deployer_status(url: &str) -> Result<AllDeployerStatus, Error> {
    Ok(reqwest::get(url)?.error_for_status()?.json()?)
}

fn get_transitioner_status(url: &str) -> Result<IndexMap<String, TransitionStatusInfo>, Error> {
    Ok(reqwest::get(url)?.error_for_status()?.json()?)
}

fn should_teardown_namespaces() -> bool {
    env::var("NAMESPACE_CLEANUP")
        .map(|v| v != "NoThanks")
        .unwrap_or(true)
}

fn check_health(url: &str) -> bool {
    let response = match reqwest::get(url) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("request error: {:?}", e);
            return false;
        }
    };

    eprintln!("response status: {:?}", response.status());

    response.status() == reqwest::StatusCode::OK
}

fn terminate_child(child: &Child) -> Result<(), Error> {
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)?;
    Ok(())
}

type ReqwestResult<T> = std::result::Result<T, reqwest::Error>;

fn should_retry<T>(result: &ReqwestResult<T>) -> bool {
    use std::io::ErrorKind::*;
    match result {
        Ok(_) => false,
        Err(error) => {
            // FIXME this is really ugly
            match error
                .get_ref()
                .and_then(|e| e.downcast_ref::<std::io::Error>())
                .map(|e| e.kind())
            {
                Some(BrokenPipe) | Some(ConnectionRefused) | Some(WouldBlock) => return true,
                _ => {}
            };

            match error
                .get_ref()
                .and_then(|e| e.downcast_ref::<hyper::Error>())
                .and_then(|e| e.cause2())
                .and_then(|e| e.downcast_ref::<std::io::Error>())
                .map(|e| e.kind())
            {
                Some(BrokenPipe) | Some(ConnectionRefused) | Some(WouldBlock) => return true,
                _ => {}
            };
            false
        }
    }
}

pub fn get<T: reqwest::IntoUrl>(url: T) -> ReqwestResult<reqwest::Response> {
    reqwest::ClientBuilder::new()
        .timeout(std::time::Duration::from_millis(1000))
        .build()?
        .get(url)
        .send()
}

pub fn retrying_request<T, F: Fn() -> ReqwestResult<T>>(f: F) -> ReqwestResult<T> {
    let mut result = f();
    let mut retries = 0;
    // retry connection refuseds and broken pipes
    while should_retry(&result) && retries < 50 {
        result = f();
        retries += 1;
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    result
}
