extern crate failure;
extern crate nix;
extern crate rand;
pub extern crate reqwest;
extern crate tempfile;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

extern crate common;
extern crate git_fixture;

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use failure::Error;
use rand::Rng;

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
}

impl IntegrationTest {
    pub fn new() -> IntegrationTest {
        let mut root = env::current_exe()
            .unwrap()
            .parent()
            .expect("executable's directory")
            .to_path_buf();
        if root.ends_with("deps") {
            root.pop();
        }
        let dir = tempfile::TempDir::new().unwrap();
        // copy kube config, ignore errors
        let _ = fs::copy(
            env::home_dir().unwrap().join(".kube/config"),
            dir.path().join("kube_config"),
        );
        let mut rng = rand::thread_rng();
        let mut ports = HashMap::new();
        ports.insert(TestService::Deployer, rng.gen());
        IntegrationTest {
            dir,
            executable_root: root,
            processes: Vec::new(),
            ports,
            suffix: rng.gen_ascii_chars()
                .take(5)
                .collect::<String>()
                .to_lowercase(),
            created_namespaces: Vec::new(),
        }
    }

    pub fn git_fixture(&self, data: &str) -> git_fixture::RepoFixture {
        let template = git_fixture::RepoTemplate::from_string(data).unwrap();
        template
            .create_in(&self.dir.path().join("versions.git"))
            .unwrap()
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
            .args(&[
                "--kubeconfig",
                "./kube_config",
                "--namespace",
                &namespace
            ])
            .args(args)
            .current_dir(self.dir.path())
            .status()
            .unwrap();

        if !status.success() {
            panic!(
                "kubectl exited with code {}",
                status
            );
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
        *self.ports.get(&service).unwrap()
    }

    fn adapt_config(&self, config: &str, service: TestService) -> String {
        config
            .replace("%%api_port%%", &self.get_port(service).to_string())
            .replace("%%suffix%%", &self.suffix)
    }

    pub fn run_deployer(&mut self, config: &str) -> &mut Self {
        let config = self.adapt_config(config, TestService::Deployer);
        let config_path = self.dir.path().join("deployer.yaml");
        {
            let mut file = File::create(&config_path).unwrap();
            file.write_all(config.as_bytes()).unwrap();
            file.flush().unwrap();
        }
        let child = Command::new(self.executable_root.join("deployer"))
            .arg("--config")
            .arg(&config_path)
            .current_dir(self.dir.path())
            .env_clear()
            .env("RUST_LOG", "info")
            .env("RUST_BACKTRACE", "1")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        self.processes.push((TestService::Deployer, child));
        self
    }

    pub fn wait_ready(&mut self) -> &mut Self {
        for _ in 0..50 {
            eprintln!("checking health...");
            // TODO don't hard-code deployer here
            if check_health(&format!(
                "http://127.0.0.1:{}/health",
                self.get_port(TestService::Deployer)
            )) {
                return self;
            }
            eprintln!("not ok");

            for &mut (ref name, ref mut child) in self.processes.iter_mut() {
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
        std::thread::sleep(std::time::Duration::from_millis(2000));
        for _ in 0..500 {
            eprintln!("checking rollout status...");

            if let Ok(status) = get_deployer_status(&format!(
                "http://127.0.0.1:{}/status",
                self.get_port(TestService::Deployer)
            )) {
                if let Some(env_status) = status.deployers.get(env) {
                    // TODO check deployed version
                    if env_status.rollout_status == RolloutStatus::Clean {
                        eprintln!("rollout status is {:?}!", env_status);
                        return self;
                    } else {
                        eprintln!("rollout status is {:?}...", env_status.rollout_status);
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

// TODO move these to common crate?
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum RolloutStatus {
    InProgress,
    Clean,
    Failed { message: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeployerStatus {
    deployed_version: String,
    last_successfully_deployed_version: Option<String>,
    rollout_status: RolloutStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AllDeployerStatus {
    deployers: ::std::collections::BTreeMap<String, DeployerStatus>,
}

fn get_deployer_status(url: &str) -> Result<AllDeployerStatus, Error> {
    Ok(reqwest::get(url)?.json()?)
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

    response.status() == reqwest::StatusCode::Ok
}

fn terminate_child(child: &Child) -> Result<(), Error> {
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    Ok(nix::sys::signal::kill(
        pid,
        nix::sys::signal::Signal::SIGTERM,
    )?)
}

type ReqwestResult<T> = std::result::Result<T, reqwest::Error>;

fn should_retry<T>(result: &ReqwestResult<T>) -> bool {
    match result {
        &Ok(_) => false,
        &Err(ref error) => {
            match error.get_ref().and_then(|e| e.downcast_ref::<std::io::Error>()).map(|e| e.kind()) {
                Some(std::io::ErrorKind::BrokenPipe) | Some(std::io::ErrorKind::ConnectionRefused) => {
                    true
                }
                _ => {
                    false
                }
            }
        }
    }
}

pub fn retrying_request<T, F: Fn() -> ReqwestResult<T>>(f: F) -> ReqwestResult<T> {
    let mut result = f();
    let mut retries = 0;
    // retry connection refuseds and broken pipes
    while should_retry(&result) && retries < 10 {
        result = f();
        retries += 1;
    }
    result
}
