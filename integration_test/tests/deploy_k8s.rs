extern crate integration_test;

use integration_test::*;

#[test]
fn deploy_k8s() {
    let mut test = IntegrationTest::new();
    test.create_namespace("dev-");
    let fixture = test.git_fixture(include_str!("./repo.yaml"));
    fixture.set_ref("refs/heads/master", "head").unwrap();
    test.run_deployer(include_str!("./config_k8s.yaml"))
        .wait_ready()
        .wait_env_rollout_done("dev", "head");
    let mut url = test.get_service_url("dev-", "s1");
    url.push_str("/answer");
    eprintln!("Requesting {}...", url);
    let response = reqwest::get(&url).unwrap()
        .text().unwrap();
    assert_eq!("23", response.trim());
    test.finish()
}
