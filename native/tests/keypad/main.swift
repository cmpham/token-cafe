import Foundation

for command in 1...11 {
        var frame = [UInt8](repeating: 0, count: 32)
        frame.replaceSubrange(0..<8, with: [0xfe,84,67,79,1,2,UInt8(command),255])
        precondition(inputCommand(frame) == command)
        precondition(inputCommand(Array(frame.prefix(8))) == command)
        frame[5] = 0; precondition(inputCommand(frame) == nil)
        frame[5] = 2; frame[31] = 1; precondition(inputCommand(frame) == nil)
    }
    precondition(inputCommand([]) == nil)
    precondition(inputCommand([UInt8](repeating: 0, count: 32)) == nil)
    precondition(preferredLink(usb: true, bluetooth: true) == .usb)
    precondition(preferredLink(usb: false, bluetooth: true) == .bluetooth)
    precondition(preferredLink(usb: true, bluetooth: false) == .usb)
    precondition(preferredLink(usb: false, bluetooth: false) == .offline)
    print("PASS: USB/Bluetooth command parser and transport priority (50 checks; no device interaction)")
    exit(0)
