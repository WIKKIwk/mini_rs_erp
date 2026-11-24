fn ssid_matches_hex(ssid: &str, hex: &str) -> bool {
    hex.len() == ssid.len() * 2
        && ssid
            .bytes()
            .zip(hex.as_bytes().chunks(2))
            .all(|(byte, hex)| {
                hex.len() == 2
                    && hex.iter().all(u8::is_ascii_hexdigit)
                    && std::str::from_utf8(hex)
                        .ok()
                        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                        == Some(byte)
            })
}

fn parse_open_networks(output: &str) -> Result<Vec<OpenNetwork>, WifiError> {
    let mut networks = BTreeMap::<String, Option<OpenNetwork>>::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let row = fields(line)?;
        if row.len() != 8 {
            return Err(WifiError::InvalidOutput);
        }
        if !valid_bssid(&row[2]) {
            continue;
        }
        let bssid = row[2].to_ascii_uppercase();
        let signal = row[4].parse::<i32>().ok().filter(|v| (0..=100).contains(v));
        // SECURITY also catches WEP's privacy flag. WPA/RSN flags independently
        // reject OWE/unknown secured variants even if SECURITY is unrecognized.
        let open = valid_ssid(&row[0])
            && ssid_matches_hex(&row[0], &row[1])
            && matches!(row[3].as_str(), "" | "--")
            && row[5] == "Infra"
            && row[6] == "(none)"
            && row[7] == "(none)"
            && signal.is_some();
        if !open {
            // An ambiguous BSSID with any secured/malformed observation is not
            // eligible, regardless of the order of observations in this scan.
            networks.insert(bssid, None);
            continue;
        }
        let network = OpenNetwork {
            ssid: row[0].clone(),
            bssid: bssid.clone(),
            signal: signal.unwrap(),
        };
        match networks.entry(bssid) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(network));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let Some(existing) = entry.get_mut() {
                    if existing.ssid != network.ssid {
                        entry.insert(None);
                    } else {
                        existing.signal = existing.signal.max(network.signal);
                    }
                }
            }
        }
    }
    let mut networks: Vec<_> = networks.into_values().flatten().collect();
    networks.sort_by(|a, b| b.signal.cmp(&a.signal).then(a.bssid.cmp(&b.bssid)));
    Ok(networks)
}
