import Foundation
import CoreBluetooth

// One encrypted GATT connection to this keypad; no Bluetooth keyboard capture.
final class BluetoothLink: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    static let service = CBUUID(string: "f6cafe10-7f67-4cd9-9b7d-58381d8de601")
    static let identity = CBUUID(string: "f6cafe11-7f67-4cd9-9b7d-58381d8de601")
    static let display = CBUUID(string: "f6cafe12-7f67-4cd9-9b7d-58381d8de601")
    static let input = CBUUID(string: "f6cafe13-7f67-4cd9-9b7d-58381d8de601")
    var central: CBCentralManager!
    var peripheral: CBPeripheral?
    var display: CBCharacteristic?
    var input: CBCharacteristic?
    var identified = false
    var subscribed = false
    var lastWrite = Date.distantPast
    var lastAttempt = Date.distantPast
    var lastSend = Date.distantPast
    var pending: [Data] = []
    var writing = false
    var writeStarted = Date.distantPast
    var sequence: UInt8 = 0
    var lastInputSequence: UInt8?
    var excluded = Set<UUID>()
    var state = "bluetooth-connecting"
    var onInput: (Int) -> Void = { _ in }
    var ready: Bool { identified && subscribed && Date().timeIntervalSince(lastWrite) < 5 }
    override init() {
        super.init()
        central = CBCentralManager(delegate: self, queue: .main)
    }
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        if central.state == .poweredOn { state = "bluetooth-connecting"; scan() }
        else {
            state = central.state == .unauthorized ? "bluetooth-permission-required" : "bluetooth-unavailable"
            clear()
        }
    }
    func scan() {
        guard central.state == .poweredOn, peripheral == nil else { return }
        if let connected = central.retrieveConnectedPeripherals(withServices: [Self.service]).first(where: { !excluded.contains($0.identifier) }) {
            connect(connected); return
        }
        central.scanForPeripherals(withServices: [Self.service], options: nil)
    }
    func centralManager(_ central: CBCentralManager, didDiscover p: CBPeripheral, advertisementData: [String: Any], rssi: NSNumber) {
        guard peripheral == nil, !excluded.contains(p.identifier) else { return }
        connect(p)
    }
    func connect(_ p: CBPeripheral) {
        central.stopScan(); peripheral = p; p.delegate = self; lastAttempt = Date()
        central.connect(p, options: nil)
    }
    func centralManager(_ central: CBCentralManager, didConnect p: CBPeripheral) {
        guard p == peripheral else { return }
        emit(["status":"bluetooth-connected"])
        p.discoverServices([Self.service])
    }
    func centralManager(_ central: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) { if p == peripheral { clear() } }
    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) { if p == peripheral { clear() } }
    func clear() {
        peripheral = nil; display = nil; input = nil; identified = false; subscribed = false
        pending.removeAll(); writing = false; lastWrite = .distantPast; lastSend = .distantPast
        lastInputSequence = nil; lastAttempt = Date()
    }
    func fail() {
        if let p = peripheral { central.cancelPeripheralConnection(p) }
        clear()
    }
    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard p == peripheral else { return }
        guard error == nil, let service = p.services?.first(where: { $0.uuid == Self.service }) else { fail(); return }
        p.discoverCharacteristics([Self.identity,Self.display,Self.input], for: service)
    }
    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        guard p == peripheral else { return }
        guard error == nil else { fail(); return }
        display = service.characteristics?.first(where: { $0.uuid == Self.display && $0.properties.contains(.write) })
        input = service.characteristics?.first(where: { $0.uuid == Self.input && $0.properties.contains(.notify) })
        guard display != nil, input != nil,
              let identity = service.characteristics?.first(where: { $0.uuid == Self.identity }) else { fail(); return }
        // This encrypted read initiates normal macOS pairing when necessary.
        p.readValue(for: identity)
    }
    func peripheral(_ p: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        guard p == peripheral else { return }
        guard error == nil, let data = characteristic.value else { fail(); return }
        if characteristic.uuid == Self.identity {
            guard data == Data("TCM2651A6626B5AE5931".utf8), let input else {
                excluded.insert(p.identifier); fail(); return
            }
            emit(["status":"bluetooth-identified"])
            identified = true; p.setNotifyValue(true, for: input)
        } else if characteristic.uuid == Self.input, ready, data.count == 8,
                  let command = inputCommand(Array(data)), lastInputSequence != data[7] {
            lastInputSequence = data[7]; onInput(command)
        }
    }
    func peripheral(_ p: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic, error: Error?) {
        guard p == peripheral else { return }
        guard error == nil, characteristic.isNotifying else { fail(); return }
        subscribed = true
    }
    func peripheral(_ p: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic, error: Error?) {
        guard p == peripheral else { return }
        guard characteristic.uuid == Self.display, error == nil else { fail(); return }
        writing = false; lastWrite = Date(); sendNext()
    }
    func sendNext() {
        guard !writing, !pending.isEmpty, let p = peripheral, let display else { return }
        writing = true; writeStarted = Date()
        p.writeValue(pending.removeFirst(), for: display, type: .withResponse)
    }
    func refresh(_ rows: [String]) {
        if peripheral == nil {
            if Date().timeIntervalSince(lastAttempt) > 3 { lastAttempt = Date(); scan() }
            return
        }
        if writing && Date().timeIntervalSince(writeStarted) > 5 { fail(); return }
        if !subscribed && Date().timeIntervalSince(lastAttempt) > 60 { fail(); return }
        guard identified, subscribed, !writing, pending.isEmpty,
              rows.allSatisfy({ $0.utf8.count == 16 }), Date().timeIntervalSince(lastSend) >= 2 else { return }
        sequence &+= 1
        pending = (0..<4).map { row in Data([1,sequence,UInt8(row)] + Array(rows[row].utf8) + [0]) }
        lastSend = Date(); sendNext()
    }
}
