//! A node's target as the node service answers it (`docs/boundary-api.md`, 「bmo-node」): a hub node by its hub's
//! address, the file its token is kept in and the name its agent attaches as, and an ssh node by its login, each
//! read beside the fields the engine does not use.

use bmo_voice::units::node::{Reach, Target};
use serde_json::json;

#[test]
fn a_hub_node_and_an_ssh_node_read_as_the_node_service_answers_them() {
  let hub: Target = serde_json::from_value(json!({
    "nodeName": "node-b", "kind": "hub", "role": "voice", "host": "", "port": 0, "user": "", "keyPath": null,
    "shell": null, "sshOptions": [], "sshArgs": [], "hubId": "hub-a", "hubAddress": "http://hub.invalid",
    "hubTokenFile": "~/hub.env", "agentName": "node-b-agent"
  }))
  .expect("a hub node reads");

  assert_eq!(
    hub.reach,
    Reach::Hub {
      hub_url: "http://hub.invalid".to_owned(),
      token_file: "~/hub.env".to_owned(),
      agent: "node-b-agent".to_owned(),
    }
  );
  assert_eq!(hub.role.as_deref(), Some("voice"));

  let ssh: Target = serde_json::from_value(json!({
    "nodeName": "node-c", "kind": "ssh", "host": "node-c.invalid", "port": 22, "user": "someone",
    "keyPath": null, "shell": "posix", "sshOptions": ["BatchMode=yes"],
    "sshArgs": ["-o", "BatchMode=yes", "-p", "22", "someone@node-c.invalid"]
  }))
  .expect("an ssh node reads");

  assert!(matches!(ssh.reach, Reach::Ssh { port: 22, .. }));

  let Reach::Ssh { ssh_args, .. } = ssh.reach else {
    panic!("an ssh node");
  };

  assert_eq!(
    ssh_args,
    ["-o", "BatchMode=yes", "-p", "22", "someone@node-c.invalid"],
    "the login the engine opens is the target's own arguments"
  );
}
