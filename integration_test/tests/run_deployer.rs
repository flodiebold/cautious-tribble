extern crate integration_test;

use integration_test::*;

#[test]
fn run_deployer() {
    let mut test = IntegrationTest::new();
    let fixture = test.git_fixture(include_str!("./repo.yaml"));
    fixture.set_ref("refs/heads/master", "head").unwrap();
    test.run_deployer(include_str!("./config_no_deployers.yaml"))
        .wait_ready();
    test.finish()
}
