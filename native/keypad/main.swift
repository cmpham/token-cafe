// Dedicated Pipar transport. Never monitors another keyboard or emits Mac keys.
import Foundation
import IOKit.hid
import CoreBluetooth
import Darwin

func number(_ device: IOHIDDevice, _ key: String) -> Int {
    (IOHIDDeviceGetProperty(device, key as CFString) as? NSNumber)?.intValue ?? 0
}
func string(_ device: IOHIDDevice, _ key: String) -> String {
    IOHIDDeviceGetProperty(device, key as CFString) as? String ?? ""
}
final class Transport {
    let manager = IOHIDManagerCreate(kCFAllocatorDefault, IOOptionBits(0x8) /* kIOHIDManagerOptionIndependentDevices: manage each device explicitly */)
    var devices: [IOHIDDevice] = []
    var bluetooth: BluetoothLink!
    var usbReady: Bool { rawReady && Date().timeIntervalSince(lastAck) < 5 }
    func updateStatus() {
        let link = preferredLink(usb: usbReady, bluetooth: bluetooth.ready)
        let state = link == .usb ? (bluetooth.ready ? "usb-bluetooth-ready" : "oled-ready") : (link == .bluetooth ? "bluetooth-ready" : bluetooth.state)
        if reportedState != state { emit(["status":state]); reportedState = state }
    }
    var lastInputSequence: UInt8?
    var raw: IOHIDDevice?
    var rawReady = false
    var sequence: UInt8 = 0
    var rows = [String](repeating: "", count: 4)
    var inputBuffer = Data()
    let report = UnsafeMutablePointer<UInt8>.allocate(capacity: 64)
    var lastSend = Date.distantPast
    var lastProbe = Date.distantPast
    var lastAck = Date.distantPast
    var reportedState = ""
    func start() {
        bluetooth = BluetoothLink()
        bluetooth.onInput = { [weak self] command in
            guard let self, preferredLink(usb: self.usbReady, bluetooth: self.bluetooth.ready) == .bluetooth else { return }
            emit(["command":command])
        }
        IOHIDManagerSetDeviceMatching(manager, [kIOHIDVendorIDKey: 0x1d50, kIOHIDProductIDKey: 0x6170, kIOHIDPrimaryUsagePageKey: 0xff60] as CFDictionary)
        let context = Unmanaged.passUnretained(self).toOpaque()
        IOHIDManagerRegisterDeviceMatchingCallback(manager, { context, _, _, device in
            Unmanaged<Transport>.fromOpaque(context!).takeUnretainedValue().add(device)
        }, context)
        IOHIDManagerRegisterDeviceRemovalCallback(manager, { context, _, _, device in
            Unmanaged<Transport>.fromOpaque(context!).takeUnretainedValue().remove(device)
        }, context)
        IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.defaultMode.rawValue)
        let result = IOHIDManagerOpen(manager, IOOptionBits(kIOHIDOptionsTypeNone))
        if result != kIOReturnSuccess { emit(["status":"hid-unavailable", "code":result]) }
        // Input is only the companion's four display rows, never keyboard text.
        FileHandle.standardInput.readabilityHandler = { handle in
            let data = handle.availableData
            if data.isEmpty { exit(0) }
            DispatchQueue.main.async { self.consume(data) }
        }
        Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { _ in self.refresh() }
    }
    func add(_ device: IOHIDDevice) {
        // VID/PID alone is not enough: pin this user's physical keypad.
        guard string(device, kIOHIDSerialNumberKey) == "651A6626B5AE5931" else { return }
        guard number(device, kIOHIDPrimaryUsagePageKey) == 0xff60 else { return }
        let result = IOHIDDeviceOpen(device, IOOptionBits(kIOHIDOptionsTypeNone))
        guard result == kIOReturnSuccess else {
            emit(["status":"usb-open-failed", "code":result]); return
        }
        devices.append(device)
        let context = Unmanaged.passUnretained(self).toOpaque()
        IOHIDDeviceScheduleWithRunLoop(device, CFRunLoopGetMain(), CFRunLoopMode.defaultMode.rawValue)
        raw = device; rawReady = false; lastProbe = .distantPast; lastInputSequence = nil
        reportedState = ""
        IOHIDDeviceRegisterInputReportCallback(device, report, 64, { context, _, _, _, _, bytes, length in
            Unmanaged<Transport>.fromOpaque(context!).takeUnretainedValue().ack(Array(UnsafeBufferPointer(start: bytes, count: length)))
        }, context)
        emit(["status":"connected", "interface":"display"])
    }

    func remove(_ device: IOHIDDevice) {
        guard devices.contains(where: { CFEqual($0, device) }) else { return }
        devices.removeAll { CFEqual($0, device) }; lastInputSequence = nil
        if let r = raw, CFEqual(r, device) { raw = nil; rawReady = false }
        emit(["status":"disconnected"])
    }
    func consume(_ data: Data) {
        inputBuffer.append(data)
        if inputBuffer.count > 16384 { inputBuffer.removeAll(); return }
        while let end = inputBuffer.firstIndex(of: 10) {
            let line = inputBuffer[..<end]; inputBuffer.removeSubrange(...end)
            guard let value = try? JSONSerialization.jsonObject(with: line) as? [String: Any],
                  let next = value["rows"] as? [String], next.count == 4,
                  next.allSatisfy({ $0.utf8.count == 16 && $0.utf8.allSatisfy { (32...126).contains($0) } }) else { continue }
            if next != rows { rows = next; lastSend = .distantPast; bluetooth.lastSend = .distantPast }
        }
    }
    func write(_ bytes: [UInt8]) -> Bool {
        guard let device = raw else { return false }
        return bytes.withUnsafeBufferPointer {
            IOHIDDeviceSetReport(device, kIOHIDReportTypeOutput, 0, $0.baseAddress!, $0.count) == kIOReturnSuccess
        }
    }
    func ack(_ bytes: [UInt8]) {
        if let command = inputCommand(bytes) {
            guard rawReady, Date().timeIntervalSince(lastAck) < 5,
                  lastInputSequence != bytes[7] else { return }
            lastInputSequence = bytes[7]
            emit(["command": command]); return
        }
        guard bytes.count == 32, Array(bytes.prefix(5)) == [0xfe,84,67,79,1],
              bytes[5] <= 1, bytes[6] == 1 else { return }
        guard bytes[7] == 1 else {
            if reportedState != "firmware-required" { emit(["status":"firmware-required"]); reportedState = "firmware-required" }
            return
        }
        rawReady = true; lastAck = Date()
        updateStatus()
    }
    func refresh() {
        bluetooth.refresh(rows)
        updateStatus()
        guard raw != nil else { return }
        if !rawReady || Date().timeIntervalSince(lastAck) > 5 {
            rawReady = false
            if Date().timeIntervalSince(lastProbe) > 2 {
                lastProbe = Date(); var probe = [UInt8](repeating: 0, count: 32)
                probe.replaceSubrange(0..<6, with: [0xfe,84,67,79,1,0]); _ = write(probe)
            }
            return
        }
        guard rows.allSatisfy({ $0.utf8.count == 16 }), Date().timeIntervalSince(lastSend) >= 2 else { return }
        sequence &+= 1
        for row in 0..<4 {
            var packet = [UInt8](repeating: 0, count: 32)
            packet.replaceSubrange(0..<9, with: [0xfe,84,67,79,1,1,1,sequence,UInt8(row)])
            packet.replaceSubrange(9..<25, with: rows[row].utf8)
            if !write(packet) { rawReady = false; return }
        }
        lastSend = Date()
    }
}
guard CommandLine.arguments.count == 1 else {
    fputs("Usage: keypad-hid (launched by Token Café); tests: bash tools/test_keypad.sh\n", stderr)
    exit(64)
}
// Only one companion may own the keypad and its display at a time.
let lockFD = open("/tmp/token-cafe-keypad-\(getuid()).lock", O_CREAT | O_RDWR | O_NOFOLLOW, 0o600)
guard lockFD >= 0, flock(lockFD, LOCK_EX | LOCK_NB) == 0 else {
    emit(["status":"already-owned"]); exit(1)
}
let transport = Transport()
transport.start()
RunLoop.main.run()
