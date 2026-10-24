// Native macOS Wi-Fi bridge for SystemDoctor. Never reads keychain passwords,
// requests location authorization, changes radio power, or commits configuration.
// Compile explicitly with build_doctor_wifi_helper.sh; do not run via `swift`.
import CoreLocation
import CoreWLAN
import Darwin
import Foundation

struct HelperFailure: Error {
    let code: String
    let message: String
}

func fail(_ code: String, _ message: String) -> HelperFailure {
    HelperFailure(code: code, message: message)
}

func emit(_ object: [String: Any], exitCode: Int32 = 0) -> Never {
    if let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) {
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([10]))
    }
    exit(exitCode)
}

func validSSID(_ ssid: String) -> Bool {
    (1...32).contains(ssid.utf8.count) && !ssid.utf8.contains(0)
}

func validBSSID(_ bssid: String) -> Bool {
    let parts = bssid.split(separator: ":", omittingEmptySubsequences: false)
    return parts.count == 6 && parts.allSatisfy {
        $0.utf8.count == 2 && $0.utf8.allSatisfy {
            (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0)
        }
    }
}

// Parsing happens before constructing CWWiFiClient, so malformed invocations
// cannot scan, disconnect, or associate. All options are separate argv values.
func arguments() throws -> (String, [String: String]) {
    let args = Array(CommandLine.arguments.dropFirst())
    guard let command = args.first,
          ["status", "scan", "connect", "disconnect", "restore"].contains(command),
          args.count % 2 == 1 else {
        throw fail("invalid_configuration", "Expected status, scan, connect, disconnect, or restore and option/value pairs.")
    }
    var options: [String: String] = [:]
    for index in stride(from: 1, to: args.count, by: 2) {
        let key = args[index]
        guard ["--interface", "--ssid", "--bssid", "--security"].contains(key), options[key] == nil else {
            throw fail("invalid_configuration", "Unknown or repeated option.")
        }
        options[key] = args[index + 1]
    }
    let expected: Set<String>
    switch command {
    case "status", "scan": expected = ["--interface"]
    case "restore": expected = ["--interface", "--ssid", "--bssid", "--security"]
    default: expected = ["--interface", "--ssid", "--bssid"]
    }
    guard Set(options.keys).isSubset(of: expected) else {
        throw fail("invalid_configuration", "Unexpected option for this command.")
    }
    if let name = options["--interface"] {
        guard !name.isEmpty, name.utf8.count <= 32,
              name.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) }) else {
            throw fail("invalid_configuration", "Invalid Wi-Fi interface name.")
        }
    }
    if ["connect", "disconnect", "restore"].contains(command) {
        guard let ssid = options["--ssid"], validSSID(ssid),
              let bssid = options["--bssid"], validBSSID(bssid) else {
            throw fail("invalid_configuration", "A visible SSID and exact BSSID are required.")
        }
    }
    if command == "restore" {
        guard let value = options["--security"], let security = Int(value), (0...15).contains(security) else {
            throw fail("invalid_configuration", "Original security type is required for restore.")
        }
    }
    return (command, options)
}

func requireLocationAccess() throws {
    // CoreWLAN's public SDK contract requires Location Services for SSID/BSSID.
    // Fail closed; a server must not trigger a permission prompt unattended.
    guard CLLocationManager.locationServicesEnabled(),
          CLLocationManager().authorizationStatus == .authorizedAlways else {
        throw fail("permission_denied", "Enable Location Services and authorize the deployed helper's host in System Settings > Privacy & Security > Location Services before enabling Doctor Wi-Fi recovery.")
    }
}

func networkIdentity(_ network: CWNetwork) throws -> (String, String) {
    guard let ssid = network.ssid, validSSID(ssid),
          network.ssidData == Data(ssid.utf8),
          let bssid = network.bssid, validBSSID(bssid) else {
        throw fail("permission_denied", "CoreWLAN withheld network identity; check Location Services authorization. Hidden or undecodable SSIDs cannot be used.")
    }
    return (ssid, bssid.lowercased())
}

func scan(_ interface: CWInterface, name: String? = nil) throws -> Set<CWNetwork> {
    try requireLocationAccess()
    return try interface.scanForNetworks(withName: name, includeHidden: false)
}

func exactNetwork(_ interface: CWInterface, ssid: String, bssid: String) throws -> CWNetwork {
    for network in try scan(interface, name: ssid) {
        guard !network.ibss else { continue }
        let identity = try networkIdentity(network)
        if identity.0 == ssid && identity.1 == bssid.lowercased() {
            return network
        }
    }
    throw fail("unavailable", "The requested access point is no longer visible.")
}

func requireAssociationPermission(_ interface: CWInterface) throws {
    guard let config = interface.configuration() else {
        throw fail("unavailable", "Wi-Fi configuration is unavailable.")
    }
    guard !config.requireAdministratorForAssociation else {
        throw fail("permission_denied", "macOS requires administrator authorization for association; Doctor does not request elevated privileges.")
    }
}

func associateOpen(_ interface: CWInterface, ssid: String, bssid: String) throws {
    try requireAssociationPermission(interface)
    let network = try exactNetwork(interface, ssid: ssid, bssid: bssid)
    guard network.supportsSecurity(.none) else {
        throw fail("unavailable", "The requested access point is not an open network.")
    }
    // No CWMutableConfiguration/commitConfiguration: the helper never adds a
    // preferred profile or changes global remember/autojoin settings.
    try interface.associate(to: network, password: nil)
    guard interface.ssid() == ssid,
          interface.bssid()?.lowercased() == bssid.lowercased(),
          interface.security() == .none else {
        throw fail("command_failed", "macOS did not confirm the requested open access point.")
    }
}

func restoreSaved(_ interface: CWInterface, name: String, ssid: String, bssid: String, security: Int) throws {
    if interface.ssid() == ssid && interface.security().rawValue == security {
        if security != 0 || interface.bssid()?.lowercased() == bssid.lowercased() { return }
    }
    if security == 0 {
        try associateOpen(interface, ssid: ssid, bssid: bssid)
        return
    }
    try requireAssociationPermission(interface)
    guard let config = interface.configuration(),
          config.networkProfiles.array.compactMap({ $0 as? CWNetworkProfile }).contains(where: {
              $0.ssidData == Data(ssid.utf8) && $0.security != .none && $0.security != .unknown
          }) else {
        throw fail("unavailable", "Original secured network has no matching saved macOS profile; credentials are not extracted by Doctor.")
    }
    let visible = try exactNetwork(interface, ssid: ssid, bssid: bssid)
    guard let expected = CWSecurity(rawValue: security), visible.supportsSecurity(expected), !visible.supportsSecurity(.none) else {
        throw fail("unavailable", "Original network security no longer matches its saved profile.")
    }
    // networksetup delegates credential use to macOS. Some service accounts
    // cannot access the saved credential; the result is verified, never assumed.
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/sbin/networksetup")
    process.arguments = ["-setairportnetwork", name, ssid]
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try process.run()
    let deadline = Date().addingTimeInterval(15)
    while process.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.1) }
    if process.isRunning {
        kill(process.processIdentifier, SIGKILL)
        process.waitUntilExit()
        throw fail("timed_out", "Timed out restoring the saved macOS connection.")
    }
    process.waitUntilExit()
    guard process.terminationStatus == 0,
          interface.ssid() == ssid, interface.security().rawValue == security else {
        throw fail("command_failed", "macOS could not restore the original saved connection without interactive authorization.")
    }
}

do {
    let (command, options) = try arguments()
    try requireLocationAccess()
    let client = CWWiFiClient.shared()
    let interface = options["--interface"].flatMap { client.interface(withName: $0) }
        ?? (options["--interface"] == nil ? client.interface() : nil)
    guard let interface = interface, let name = interface.interfaceName,
          options["--interface"] == nil || options["--interface"] == name,
          interface.powerOn() else {
        throw fail("unavailable", "The selected Wi-Fi interface is unavailable or powered off.")
    }
    var result: [String: Any] = ["ok": true, "interface": name]
    switch command {
    case "status":
        if let ssid = interface.ssid() {
            guard validSSID(ssid), interface.ssidData() == Data(ssid.utf8),
                  let bssid = interface.bssid(), validBSSID(bssid),
                  (0...15).contains(interface.security().rawValue) else {
                throw fail("permission_denied", "Current Wi-Fi identity is unavailable; refusing to switch networks without a restore snapshot.")
            }
            result["connection"] = ["interface": name, "ssid": ssid,
                                    "bssid": bssid.lowercased(), "security": interface.security().rawValue]
        } else {
            guard interface.bssid() == nil, interface.security() == .unknown else {
                throw fail("permission_denied", "Current Wi-Fi identity is redacted; check Location Services authorization.")
            }
            result["connection"] = NSNull()
        }
    case "scan":
        var networks: [[String: Any]] = []
        for network in try scan(interface) where !network.ibss && network.supportsSecurity(.none) {
            let (ssid, bssid) = try networkIdentity(network)
            networks.append(["ssid": ssid, "bssid": bssid, "signal": network.rssiValue])
        }
        networks.sort {
            let left = $0["signal"] as! Int, right = $1["signal"] as! Int
            return left != right ? left > right : ($0["bssid"] as! String) < ($1["bssid"] as! String)
        }
        result["networks"] = Array(networks.prefix(512))
    case "connect":
        try associateOpen(interface, ssid: options["--ssid"]!, bssid: options["--bssid"]!)
    case "disconnect":
        if let ssid = interface.ssid() {
            guard validSSID(ssid), interface.ssidData() == Data(ssid.utf8),
                  let bssid = interface.bssid(), validBSSID(bssid),
                  (0...15).contains(interface.security().rawValue) else {
                throw fail("permission_denied", "Current Wi-Fi identity is unavailable; refusing to disconnect an unidentified connection.")
            }
            // A failed association can leave the original network connected;
            // an administrator may also have selected another AP. Both are
            // successful cleanup no-ops, preserving that verified connection.
            if ssid == options["--ssid"], bssid.lowercased() == options["--bssid"]!.lowercased(),
               interface.security() == .none {
                interface.disassociate()
                guard interface.ssid() != ssid || interface.bssid()?.lowercased() != bssid.lowercased() else {
                    throw fail("command_failed", "macOS did not confirm disassociation from the attempted access point.")
                }
            }
        } else if interface.bssid() != nil || interface.security() != .unknown {
            throw fail("permission_denied", "Current Wi-Fi identity is redacted; check Location Services authorization.")
        }
    case "restore":
        try restoreSaved(interface, name: name, ssid: options["--ssid"]!,
                         bssid: options["--bssid"]!, security: Int(options["--security"]!)!)
    default: throw fail("invalid_configuration", "Unknown command.")
    }
    emit(result)
} catch let error as HelperFailure {
    emit(["ok": false, "error": error.code, "message": error.message], exitCode: 1)
} catch {
    let native = error as NSError
    let code: String
    if native.domain == CWErrorDomain, native.code == -3930 {
        code = "permission_denied"
    } else if native.code == -3905 {
        code = "timed_out"
    } else {
        code = "command_failed"
    }
    emit(["ok": false, "error": code, "message": "CoreWLAN operation failed; check macOS Wi-Fi permissions and service-account access."], exitCode: 1)
}
