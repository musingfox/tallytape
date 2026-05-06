use std::path::Path;
use std::process::Command;

#[test]
fn install_hook_outputs_valid_json_snippet() {
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");
    let output = Command::new(bin)
        .arg("install-hook")
        .output()
        .expect("failed to run tallytape-writer install-hook");

    // exit status must be success
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // stderr must be empty
    assert!(
        output.stderr.is_empty(),
        "expected empty stderr, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // stdout must parse as JSON
    let stdout = String::from_utf8(output.stdout).expect("stdout is not valid UTF-8");
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is not valid JSON");

    // matcher == ""
    assert_eq!(
        json["hooks"]["SessionEnd"][0]["matcher"],
        serde_json::Value::String(String::new()),
        "matcher should be empty string"
    );

    // type == "command"
    assert_eq!(
        json["hooks"]["SessionEnd"][0]["hooks"][0]["type"],
        serde_json::Value::String("command".to_string()),
        "type should be \"command\""
    );

    // command is a string with an absolute path
    let command = json["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
        .as_str()
        .expect("command should be a string");
    assert!(
        Path::new(command).is_absolute(),
        "command should be an absolute path, got: {command}"
    );
}
