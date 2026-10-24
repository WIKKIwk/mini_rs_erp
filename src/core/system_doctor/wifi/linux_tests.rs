use std::{collections::VecDeque, sync::Mutex as StdMutex};

use super::*;

const DEVICES: &str = "eth0:ethernet:connected\nwlan0:wifi:connected\n";
const PREVIOUS: &str = "47d59e1b-831b-42f5-a62d-544c8a325a24";
const BSSID: &str = "02:11:22:33:44:55";

fn ap(ssid: &str, bssid: &str, security: &str, signal: i32, wpa: &str, rsn: &str) -> String {
    let escape = |value: &str| value.replace('\\', "\\\\").replace(':', "\\:");
    let hex: String = ssid.bytes().map(|byte| format!("{byte:02X}")).collect();
    format!(
        "{}:{}:{}:{}:{signal}:Infra:{}:{}\n",
        escape(ssid),
        hex,
        escape(bssid),
        security,
        wpa,
        rsn,
    )
}

fn open_ap() -> String {
    ap("Cafe", BSSID, "", 80, "(none)", "(none)")
}

fn target() -> OpenNetwork {
    OpenNetwork {
        ssid: "Cafe".into(),
        bssid: BSSID.into(),
        signal: 80,
    }
}

struct MockNmcli {
    outputs: StdMutex<VecDeque<Result<CommandOutput, WifiError>>>,
    commands: StdMutex<Vec<Vec<String>>>,
}

impl MockNmcli {
    fn new(outputs: Vec<Result<CommandOutput, WifiError>>) -> Arc<Self> {
        Arc::new(Self {
            outputs: StdMutex::new(outputs.into()),
            commands: StdMutex::new(Vec::new()),
        })
    }

    fn push(&self, output: Result<CommandOutput, WifiError>) {
        self.outputs.lock().unwrap().push_back(output);
    }

    fn commands(&self) -> Vec<Vec<String>> {
        self.commands.lock().unwrap().clone()
    }
}

#[async_trait]
impl NmcliRunner for MockNmcli {
    async fn run(&self, args: &[&str]) -> Result<CommandOutput, WifiError> {
        self.commands
            .lock()
            .unwrap()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        self.outputs
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected nmcli call in mocked test")
    }
}

fn success(output: impl Into<String>) -> Result<CommandOutput, WifiError> {
    Ok(CommandOutput {
        code: 0,
        stdout: output.into(),
        stderr: String::new(),
    })
}

fn adapter(mock: Arc<MockNmcli>, interface: Option<&str>) -> LinuxWifi {
    LinuxWifi {
        configured_interface: interface.map(str::to_owned),
        state: Mutex::new(AdapterState::default()),
        runner: mock,
    }
}

#[test]
fn scan_accepts_only_explicitly_open_infrastructure_networks() {
    let mut output = open_ap();
    for (bssid, security, wpa, rsn) in [
        ("02:11:22:33:44:56", "WEP", "(none)", "(none)"),
        ("02:11:22:33:44:57", "WPA2", "(none)", "psk"),
        ("02:11:22:33:44:58", "OWE", "(none)", "owe"),
        ("02:11:22:33:44:59", "", "(none)", "owe"),
        ("02:11:22:33:44:60", "unknown", "(none)", "(none)"),
        ("02:11:22:33:44:61", "", "", ""),
    ] {
        output.push_str(&ap("Blocked", bssid, security, 99, wpa, rsn));
    }
    output.push_str(
        &ap("AdHoc", "02:11:22:33:44:62", "", 99, "(none)", "(none)")
            .replace(":Infra:", ":Ad-Hoc:"),
    );
    let networks = parse_open_networks(&output).unwrap();
    assert_eq!(networks.len(), 1);
    assert_eq!(networks[0].bssid, BSSID);
}

#[test]
fn scan_preserves_ssid_colons_backslashes_and_spaces() {
    let ssid = "  Cafe:Test\\public  ";
    let networks = parse_open_networks(&ap(ssid, BSSID, "--", 65, "(none)", "(none)")).unwrap();
    assert_eq!(networks[0].ssid, ssid);
    assert_eq!(networks[0].signal, 65);
    assert!(fields("bad\\q:field").is_err());
    assert!(fields("trailing\\").is_err());
}

#[test]
fn scan_rejects_hidden_invalid_and_ambiguous_access_points() {
    let mut output = ap("", BSSID, "", 50, "(none)", "(none)");
    output.push_str(&ap(
        "Multicast",
        "01:11:22:33:44:55",
        "",
        90,
        "(none)",
        "(none)",
    ));
    output.push_str(&ap(
        "BadSignal",
        "02:11:22:33:44:56",
        "",
        101,
        "(none)",
        "(none)",
    ));
    output.push_str(&open_ap().replace("43616665", "43616666"));
    assert!(parse_open_networks(&output).unwrap().is_empty());
    let protected = ap("Cafe", BSSID, "WPA2", 90, "(none)", "psk");
    assert!(
        parse_open_networks(&(open_ap() + &protected))
            .unwrap()
            .is_empty()
    );
    assert!(
        parse_open_networks(&(protected + &open_ap()))
            .unwrap()
            .is_empty()
    );
    assert!(parse_open_networks("missing:fields").is_err());
}

#[test]
fn scan_sorts_signal_and_deduplicates_bssid_without_losing_distinct_aps() {
    let output = open_ap()
        + &ap("Cafe", BSSID, "", 85, "(none)", "(none)")
        + &ap("Cafe", "02:11:22:33:44:56", "", 95, "(none)", "(none)");
    let networks = parse_open_networks(&output).unwrap();
    assert_eq!(networks.len(), 2);
    assert_eq!(networks[0].signal, 95);
    assert_eq!(networks[1].signal, 85);
}

#[test]
fn interface_selection_never_chooses_ethernet_or_unmanaged_devices() {
    assert_eq!(select_interface(DEVICES, None).unwrap(), "wlan0");
    assert!(matches!(
        select_interface(DEVICES, Some("eth0")),
        Err(WifiError::InvalidConfiguration)
    ));
    assert!(matches!(
        select_interface("eth0:ethernet:connected\n", None),
        Err(WifiError::Unavailable)
    ));
    assert!(matches!(
        select_interface("wlan0:wifi:unmanaged\n", None),
        Err(WifiError::Unavailable)
    ));
    assert_eq!(
        select_interface("wlan0:wifi:disconnected\nwlan1:wifi:connected\n", None).unwrap(),
        "wlan1"
    );
}

#[tokio::test]
async fn current_connection_is_a_validated_saved_profile_uuid() {
    let mock = MockNmcli::new(vec![success(DEVICES), success(format!("{PREVIOUS}\n"))]);
    assert_eq!(
        adapter(mock, None)
            .current_connection()
            .await
            .unwrap()
            .as_deref(),
        Some(PREVIOUS)
    );
    let mock = MockNmcli::new(vec![success(DEVICES), success("--\n")]);
    assert!(
        adapter(mock, None)
            .current_connection()
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn connect_uses_only_own_temporary_profile_and_exact_bssid() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success(PREVIOUS),
        success(open_ap()),
        success("added"),
        success("activated"),
    ]);
    let adapter = adapter(mock.clone(), None);
    adapter.connect_open(&target()).await.unwrap();
    let commands = mock.commands();
    let add = &commands[3];
    assert_eq!(
        &add[..6],
        &["connection", "add", "save", "no", "type", "wifi"]
    );
    assert!(
        add.windows(2)
            .any(|pair| pair == ["connection.autoconnect", "no"])
    );
    assert!(
        add.windows(2)
            .any(|pair| pair == ["802-11-wireless.bssid", BSSID])
    );
    assert!(
        !add.iter()
            .any(|arg| arg.contains("security") || arg.contains("password"))
    );
    let uuid = add[add.iter().position(|arg| arg == "connection.uuid").unwrap() + 1].clone();
    assert!(valid_uuid(&uuid));
    assert_eq!(
        commands[4],
        vec![
            "connection",
            "up",
            "uuid",
            &uuid,
            "ifname",
            "wlan0",
            "ap",
            BSSID
        ]
    );
    mock.push(success("deleted"));
    adapter.disconnect_attempt().await.unwrap();
    assert_eq!(
        mock.commands()[5],
        vec!["connection", "delete", "uuid", &uuid]
    );
    adapter.disconnect_attempt().await.unwrap();
    assert_eq!(mock.commands().len(), 6);
}

#[tokio::test]
async fn revalidation_blocks_ap_that_has_become_secured() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success(PREVIOUS),
        success(ap("Cafe", BSSID, "WPA2", 80, "(none)", "psk")),
    ]);
    assert!(matches!(
        adapter(mock.clone(), None).connect_open(&target()).await,
        Err(WifiError::Unavailable)
    ));
    assert_eq!(mock.commands().len(), 3);
    assert!(
        mock.commands()
            .iter()
            .all(|command| command.first().map(String::as_str) != Some("connection"))
    );
}

#[tokio::test]
async fn timed_out_activation_deletes_only_owned_profile() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success(PREVIOUS),
        success(open_ap()),
        success("added"),
        Err(WifiError::TimedOut),
        success("deleted"),
    ]);
    assert!(matches!(
        adapter(mock.clone(), None).connect_open(&target()).await,
        Err(WifiError::TimedOut)
    ));
    let commands = mock.commands();
    assert_eq!(commands[4][3], commands[5][3]);
    assert_ne!(commands[5][3], PREVIOUS);
    assert_eq!(&commands[5][..3], &["connection", "delete", "uuid"]);
}

#[tokio::test]
async fn successful_fallback_retains_original_uuid_for_later_recovery_episode() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success(PREVIOUS),
        success(open_ap()),
        success("added"),
        success("activated"),
    ]);
    let adapter = adapter(mock.clone(), None);
    adapter.connect_open(&target()).await.unwrap();
    let commands = mock.commands();
    let owned_uuid = commands[4][3].clone();
    mock.push(success(DEVICES));
    mock.push(success(owned_uuid));
    assert_eq!(
        adapter.current_connection().await.unwrap().as_deref(),
        Some(PREVIOUS)
    );
}

#[tokio::test]
async fn disconnected_between_candidates_does_not_erase_original_uuid() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success(PREVIOUS),
        success(open_ap()),
        success("added"),
        Err(WifiError::TimedOut),
        success("deleted"),
        success(DEVICES),
        success("--"),
        success(open_ap()),
        success("added"),
        success("activated"),
    ]);
    let adapter = adapter(mock.clone(), None);
    assert!(adapter.connect_open(&target()).await.is_err());
    adapter.connect_open(&target()).await.unwrap();
    let owned_uuid = mock.commands()[10][3].clone();
    mock.push(success(DEVICES));
    mock.push(success(owned_uuid));
    assert_eq!(
        adapter.current_connection().await.unwrap().as_deref(),
        Some(PREVIOUS)
    );
}

#[tokio::test]
async fn restore_is_uuid_and_wifi_interface_scoped() {
    let mock = MockNmcli::new(vec![
        success(DEVICES),
        success("802-11-wireless\n"),
        success("restored"),
    ]);
    adapter(mock.clone(), None)
        .restore_connection(PREVIOUS)
        .await
        .unwrap();
    assert_eq!(
        mock.commands()[2],
        vec!["connection", "up", "uuid", PREVIOUS, "ifname", "wlan0"]
    );
    let mock = MockNmcli::new(vec![success(DEVICES), success("802-3-ethernet\n")]);
    assert!(matches!(
        adapter(mock.clone(), None)
            .restore_connection(PREVIOUS)
            .await,
        Err(WifiError::InvalidConfiguration)
    ));
    assert_eq!(mock.commands().len(), 2);
}

#[tokio::test]
async fn no_wifi_or_bad_configuration_cannot_mutate_network() {
    let mock = MockNmcli::new(vec![success("eth0:ethernet:connected\n")]);
    assert!(matches!(
        adapter(mock.clone(), None).connect_open(&target()).await,
        Err(WifiError::Unavailable)
    ));
    assert_eq!(mock.commands().len(), 1);
    let mock = MockNmcli::new(vec![]);
    assert!(matches!(
        adapter(mock.clone(), Some("--help")).scan_open().await,
        Err(WifiError::InvalidConfiguration)
    ));
    assert!(mock.commands().is_empty());
    let mut invalid = target();
    invalid.bssid = "02:11:22:33:44:55 --ask".into();
    assert!(matches!(
        adapter(mock.clone(), None).connect_open(&invalid).await,
        Err(WifiError::InvalidConfiguration)
    ));
    assert!(mock.commands().is_empty());
}

#[tokio::test]
async fn command_output_read_is_bounded() {
    let output = vec![b'x'; MAX_OUTPUT_BYTES as usize + 1];
    assert!(matches!(
        read_bounded(output.as_slice()).await,
        Err(WifiError::InvalidOutput)
    ));
    assert_eq!(read_bounded(b"small".as_slice()).await.unwrap(), b"small");
}

#[test]
fn errors_are_typed_without_retaining_network_or_command_details() {
    let permission = CommandOutput {
        code: 1,
        stdout: String::new(),
        stderr: "Not authorized to control networking.".into(),
    };
    assert!(matches!(
        permission.checked(),
        Err(WifiError::PermissionDenied)
    ));
    for (code, expected) in [(3, "timeout"), (8, "unavailable"), (10, "unavailable")] {
        let result = CommandOutput {
            code,
            stdout: String::new(),
            stderr: String::new(),
        }
        .checked();
        match expected {
            "timeout" => assert!(matches!(result, Err(WifiError::TimedOut))),
            _ => assert!(matches!(result, Err(WifiError::Unavailable))),
        }
    }
    assert!(matches!(
        io_error(std::io::Error::from(std::io::ErrorKind::NotFound)),
        WifiError::Unavailable
    ));
    assert!(valid_uuid(&new_uuid()));
}
