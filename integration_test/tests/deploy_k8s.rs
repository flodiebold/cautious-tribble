extern crate integration_test;

use integration_test::*;

#[test]
fn deploy_k8s() {
    let mut test = IntegrationTest::new();
    test.create_namespace("dev-")
        .kubectl_apply("dev-", include_str!("./s1-service.yaml"));
    let fixture = test.git_fixture(include_str!("./repo.yaml"));
    fixture.set_ref("refs/heads/master", "head").unwrap();
    test.run_deployer(include_str!("./config_k8s.yaml"))
        .wait_ready()
        .wait_env_rollout_done("dev");
    let url = format!("{}/answer", test.get_service_url("dev-", "s1"));
    eprintln!("Requesting {}...", url);
    let response = retrying_request(|| reqwest::get(&url))
        .and_then(|mut r| r.text())
        .unwrap();
    assert_eq!("23", response.trim());
    test.finish()
}
