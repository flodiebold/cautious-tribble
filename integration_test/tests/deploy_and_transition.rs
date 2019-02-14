extern crate integration_test;

use integration_test::*;

#[test]
fn minikube_deploy_and_transition() {
    let mut test = IntegrationTest::new();
    test.create_namespace("dev-").create_namespace("prod-");
    let fixture = test.git_fixture(include_str!("./repo_k8s.yaml"));
    fixture.set_ref("refs/heads/master", "head1").unwrap();
    test.run_deployer()
        .run_transitioner()
        .wait_ready()
        .wait_env_rollout_done("dev")
        .wait_transition("prod", 1)
        .wait_env_rollout_done("prod");

    let url = format!("{}/answer", test.get_service_url("prod-", "s1-service"));
    eprintln!("Requesting {}...", url);
    let response = retrying_request(|| reqwest::get(&url))
        .and_then(|mut r| r.text())
        .unwrap();
    assert_eq!("23", response.trim());

    // update service
    fixture.apply("refs/heads/master", "head2").unwrap();
    test.wait_env_rollout_done("dev")
        .wait_transition("prod", 2)
        .wait_env_rollout_done("prod");
    let url = format!("{}/answer", test.get_service_url("prod-", "s1-service"));
    eprintln!("Requesting {}...", url);
    let response = retrying_request(|| reqwest::get(&url))
        .and_then(|mut r| r.text())
        .unwrap();
    assert_eq!("42", response.trim());

    test.finish()
}
