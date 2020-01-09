extern crate integration_test;

use integration_test::*;

#[test]
fn minikube_deploy_k8s() {
    if skip_minikube_tests() {
        return;
    }
    let mut test = IntegrationTest::new();
    test.create_namespace("dev-");
    let fixture = test.git_fixture(include_str!("./repo_k8s.yaml"));
    fixture.set_ref("refs/heads/master", "head1").unwrap();
    test.run_deployer()
        .wait_ready()
        .wait_env_rollout_done("dev");

    let url = format!("{}/answer", test.get_service_url("dev-", "s1-service"));
    eprintln!("Requesting {}...", url);
    let response = retrying_request(|| reqwest::get(&url))
        .and_then(|mut r| r.text())
        .unwrap();
    assert_eq!("23", response.trim());

    // update service
    fixture.set_ref("refs/heads/master", "head2").unwrap();
    test.wait_env_rollout_done("dev");
    let url = format!("{}/answer", test.get_service_url("dev-", "s1-service"));
    eprintln!("Requesting {}...", url);
    let response = retrying_request(|| reqwest::get(&url))
        .and_then(|mut r| r.text())
        .unwrap();
    assert_eq!("42", response.trim());

    test.finish()
}
