extern crate integration_test;

use integration_test::*;

#[test]
fn deploy_and_transition_mock() {
    let mut test = IntegrationTest::new();
    let fixture = test.git_fixture(include_str!("./repo_mock.yaml"));
    fixture.set_ref("refs/heads/master", "head1").unwrap();
    test.run_deployer()
        .run_transitioner()
        .wait_ready()
        .wait_env_rollout_done("dev")
        .wait_transition("prod", 1)
        .wait_env_rollout_done("prod");

    // update service
    fixture.apply("refs/heads/master", "head2").unwrap();
    test.wait_env_rollout_done("dev")
        .wait_transition("prod", 2)
        .wait_env_rollout_done("prod");

    test.finish()
}
