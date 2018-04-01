extern crate failure;
extern crate nix;
extern crate rand;
pub extern crate reqwest;
extern crate tempfile;

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
            suffix: rng.gen_ascii_chars().take(5).collect::<String>().to_lowercase(),
            created_namespaces: Vec::new(),
        }
    }

    pub fn git_fixture(&self, data: &str) -> git_fixture::RepoFixture {
        let template = git_fixture::RepoTemplate::from_string(data).unwrap();
        template
            .create_in(&self.dir.path().join("versions.git"))
            .unwrap()
    }

    pub fn create_namespace(&mut self, name: &str) -> &mut Self {
        let mut namespace = name.to_owned();
        namespace.push_str(&self.suffix);
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
            .args(&["service", "--url", "-n", &namespace, svc])
            .output()
            .expect("running minikube");

        if !output.status.success() {
            panic!("minikube exited with code {} and output {}", output.status, String::from_utf8_lossy(&output.stderr));
        }

        eprintln!("minikube stderr: {}", String::from_utf8_lossy(&output.stderr));

        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    pub fn wait_env_rollout_done(&mut self, env: &str) -> &mut Self {
        // TODO
        std::thread::sleep_ms(3000);
        self
    }

    pub fn teardown_namespaces(&mut self) {
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
