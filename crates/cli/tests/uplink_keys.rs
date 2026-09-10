use std::process::{Command, Output};

const PRIVATE: &str = "3iAcr2-GUIpQfQsOaabe-eD6uK8O53exaB9pKnVfotI";
const PUBLIC: &str = "XRb2vVZJepyosoYqoXX24-lMYpkj9SekAq8ViT7RC1Q";

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_yas"));
    for name in [
        "YAS_UPLINK_IDENTITY",
        "YAS_UPLINK_TOKEN",
        "YAS_UPLINK_CLIENT_TOKEN",
        "YAS_UPLINK_CLIENT_KEYS",
    ] {
        command.env_remove(name);
    }
    command
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "YAS exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn keygen_prints_matching_compact_keys_without_a_file_argument() {
    let output = cli().arg("uplink-keygen").output().unwrap();
    assert_success(&output);
    let keys: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let private = keys["private_key"].as_str().unwrap();
    let public = keys["public_key"].as_str().unwrap();
    assert_eq!(private.len(), 43);
    assert_eq!(public.len(), 43);
    assert_eq!(
        yas_uplink::Identity::from_base64(private)
            .unwrap()
            .public_key()
            .to_string(),
        public
    );
}

#[test]
fn environment_identity_is_accepted_and_hidden_from_help_and_errors() {
    for command in ["uplink", "uplink-keygen", "uplink-public-key", "uplink-url"] {
        let help = cli()
            .env("YAS_UPLINK_IDENTITY", PRIVATE)
            .env("YAS_UPLINK_CLIENT_TOKEN", "client-token-secret")
            .args([command, "--help"])
            .output()
            .unwrap();
        assert_success(&help);
        for output in [&help.stdout, &help.stderr] {
            let text = String::from_utf8_lossy(output);
            assert!(!text.contains(PRIVATE));
            assert!(!text.contains("client-token-secret"));
        }
    }

    // Reaching the token check proves the environment supplied a valid private
    // key. With no control token, the command exits before any network I/O.
    let output = cli()
        .env("YAS_UPLINK_IDENTITY", PRIVATE)
        .env_remove("YAS_UPLINK_TOKEN")
        .args(["uplink", "https://relay.invalid", "--allow-client", PUBLIC])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("YAS_UPLINK_TOKEN is not set"),
        "{}: {stderr}",
        output.status
    );
    assert!(!stderr.contains(PRIVATE));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PRIVATE));
}

#[test]
fn private_output_can_be_loaded_directly_to_derive_its_public_key() {
    let generated = cli().args(["uplink-keygen", "--private"]).output().unwrap();
    assert_success(&generated);
    assert!(generated.stderr.is_empty());
    let output = String::from_utf8(generated.stdout).unwrap();
    let private = output.trim_end_matches('\n');
    assert_eq!(private.len(), 43);
    assert_eq!(output, format!("{private}\n"));

    let derived = cli()
        .env("YAS_UPLINK_IDENTITY", private)
        .arg("uplink-public-key")
        .output()
        .unwrap();
    assert_success(&derived);
    assert!(derived.stderr.is_empty());
    let expected = yas_uplink::Identity::from_base64(private)
        .unwrap()
        .public_key();
    assert_eq!(
        String::from_utf8(derived.stdout).unwrap(),
        format!("{expected}\n")
    );
}

#[test]
fn public_key_requires_an_existing_valid_environment_identity() {
    let output = cli()
        .env("YAS_UPLINK_IDENTITY", PRIVATE)
        .arg("uplink-public-key")
        .output()
        .unwrap();
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{PUBLIC}\n")
    );
    assert!(output.stderr.is_empty());

    for value in [None, Some(""), Some("invalid-private-key-secret")] {
        let mut command = cli();
        if let Some(value) = value {
            command.env("YAS_UPLINK_IDENTITY", value);
        }
        let output = command.arg("uplink-public-key").output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("YAS_UPLINK_IDENTITY"),
            "{}: {error}",
            output.status
        );
        assert!(!error.contains("invalid-private-key-secret"));
    }
}

#[test]
fn url_encodes_the_client_token_and_derives_the_server_pin_locally() {
    const TOKEN: &str = "client +/&?#=% space";
    for explicit in [false, true] {
        let mut command = cli();
        command
            .env("YAS_UPLINK_IDENTITY", PRIVATE)
            .env("YAS_UPLINK_TOKEN", "producer-token-secret")
            .env(
                "YAS_UPLINK_CLIENT_TOKEN",
                if explicit { "ignored-token" } else { TOKEN },
            )
            .args(["uplink-url", "https://relay.invalid:8443/base/path"]);
        if explicit {
            command.args(["--client-token", TOKEN]);
        }
        let output = command.output().unwrap();
        assert_success(&output);
        assert!(output.stderr.is_empty());
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(!output.contains(PRIVATE));
        assert!(!output.contains("producer-token-secret"));
        assert!(!output.contains("ignored-token"));
        let uri = output.trim_end().strip_prefix("uplink:").unwrap();
        let url = url::Url::parse(uri).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("relay.invalid"));
        assert_eq!(url.port(), Some(8443));
        assert_eq!(url.path(), "/base/path");
        assert!(url.query().is_none());
        let fields: Vec<_> = url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
            .into_owned()
            .collect();
        assert_eq!(
            fields,
            [
                ("token".into(), TOKEN.into()),
                ("server".into(), PUBLIC.into())
            ]
        );
    }
}

#[test]
fn url_rejects_invalid_inputs_without_printing_credentials() {
    let secret = "routing-secret";
    for control in [
        "not-a-url",
        "http://relay.invalid",
        "https://routing-secret@relay.invalid",
        "https://user:routing-secret@relay.invalid",
        "https://relay.invalid?token=routing-secret",
        "https://relay.invalid#identity=routing-secret",
    ] {
        let output = cli()
            .env("YAS_UPLINK_IDENTITY", PRIVATE)
            .env("YAS_UPLINK_CLIENT_TOKEN", secret)
            .args(["uplink-url", control])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("control URL"), "{}: {error}", output.status);
        assert!(!error.contains(secret));
        assert!(!error.contains(PRIVATE));
    }

    for token in [None, Some(""), Some(" "), Some("routing-secret\n")] {
        let mut command = cli();
        command
            .env("YAS_UPLINK_IDENTITY", PRIVATE)
            .env("YAS_UPLINK_TOKEN", "producer-token-secret");
        if let Some(token) = token {
            command.env("YAS_UPLINK_CLIENT_TOKEN", token);
        }
        let output = command
            .args(["uplink-url", "https://relay.invalid"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains(secret));
        assert!(!error.contains(PRIVATE));
        assert!(!error.contains("producer-token-secret"));
    }

    let output = cli()
        .env("YAS_UPLINK_CLIENT_TOKEN", secret)
        .args(["uplink-url", "https://relay.invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("YAS_UPLINK_IDENTITY"),
        "{}: {error}",
        output.status
    );
    assert!(!error.contains(secret));
}
