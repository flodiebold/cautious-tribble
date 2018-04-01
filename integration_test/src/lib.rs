extern crate tempfile;
extern crate nix;
extern crate reqwest;
extern crate failure;

extern crate git_fixture;
extern crate common;

use std::io::Write;
use std::fs::File;
use std::path::PathBuf;
use std::env;
use std::process::{Child, Command, Stdio};
use std::os::unix::process::ExitStatusExt;

use failure::Error;

pub struct IntegrationTest {
    executable_root: PathBuf,
    dir: tempfile::TempDir,
    processes: Vec<(String, Child)>,
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
        IntegrationTest {
            dir,
            executable_root: root,
            processes: Vec::new(),
        }
    }

    pub fn git_fixture(&self, data: &str) -> git_fixture::RepoFixture {
        let template = git_fixture::RepoTemplate::from_string(data).unwrap();
        template.create_in(&self.dir.path().join("versions.git")).unwrap()
    }

    pub fn run_deployer(&mut self, config: &str) -> &mut Self {
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
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        self.processes.push(("deployer".to_owned(), child));
        self
    }

    pub fn wait_ready(&mut self) {
        for _ in 0..50 {
            eprintln!("checking health...");
            // TODO don't hard-code deployer here
            if check_health("http://127.0.0.1:9001/health") {
                return;
            }
            eprintln!("not ok");

            for &mut (ref name, ref mut child) in self.processes.iter_mut() {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("Process {} exited with code {}", name, status);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        panic!("wait_ready timed out");
    }

    pub fn finish(mut self) {
        eprintln!("Test finished, stopping processes...");
        for (name, mut child) in self.processes.drain(..) {
            terminate_child(&child).unwrap();
            let status = child.wait().unwrap();
            // TODO implement proper signal handling in the services, then
            // remove the handling for signal 15 here
            if !status.success() && status.signal() != Some(15) {
                panic!("Process {} exited with code {}", name, status)
            }
        }
    }
}

impl Drop for IntegrationTest {
    fn drop(&mut self) {
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
    Ok(nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)?)
}
