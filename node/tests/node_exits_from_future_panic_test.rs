// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

mod utils;

use crate::utils::CommandConfig;
#[cfg(not(target_os = "windows"))]
use std::process;
#[cfg(not(target_os = "windows"))]
use std::thread;
#[cfg(not(target_os = "windows"))]
use std::time::Duration;

#[cfg(not(target_os = "windows"))]
#[test]
fn node_exits_from_future_panic_integration() {
    let panic_config = CommandConfig::new().pair("--crash-point", "panic");
    let mut node = utils::SubstratumNode::start_standard(Some(panic_config));

    let exit_code = node.wait_for_exit().unwrap().status.code();
    assert_eq!(Some(101), exit_code);
}

#[cfg(target_os = "windows")]
#[test]
fn node_exits_from_future_panic_integration() {
    let panic_config = CommandConfig::new().pair("--crash-point", "panic");
    let mut node = utils::SubstratumNode::start_standard(Some(panic_config));

    let exit_code = node.wait_for_exit().unwrap().status.code();
    // Sometimes 1, sometimes 101
    assert_ne!(exit_code, Some(0));
}

#[test]
fn node_logs_panic_integration() {
    let panic_config = CommandConfig::new().pair("--crash-point", "panic");
    let mut node = utils::SubstratumNode::start_standard(Some(panic_config));

    node.wait_for_log("stack backtrace", Some(1000));
}

#[cfg(target_os = "linux")]
const STAT_FORMAT_PARAM_NAME: &str = "-c";

#[cfg(target_os = "macos")]
const STAT_FORMAT_PARAM_NAME: &str = "-f";

#[cfg(not(target_os = "windows"))]
#[test]
fn node_logfile_does_not_belong_to_root_integration() {
    let logfile_path = utils::SubstratumNode::path_to_logfile();
    match std::fs::remove_file(&logfile_path) {
        Ok(_) => (),
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(ref e) => panic!("{:?}", e),
    }

    let mut node = utils::SubstratumNode::start_standard(None);

    thread::sleep(Duration::from_secs(2));
    node.kill().unwrap();
    let mut command = process::Command::new("stat");
    command.args(vec![
        STAT_FORMAT_PARAM_NAME,
        "%u:%g",
        logfile_path.display().to_string().as_str(),
    ]);
    let output = command.output().unwrap();
    let stdout = String::from_utf8(output.clone().stdout).unwrap();
    let stderr = String::from_utf8(output.clone().stderr).unwrap();
    assert_eq!(stderr, "".to_string());
    let ids: Vec<&str> = stdout.split(":").into_iter().collect();
    assert_ne!(
        ids[0],
        "0".to_string(),
        "stat didn't say UID for {:?} was not 0: {:?}",
        &logfile_path,
        &output
    );
    assert_ne!(
        ids[1],
        "0".to_string(),
        "stat didn't say GID for {:?} was not 0: {:?}",
        &logfile_path,
        &output
    );
}
