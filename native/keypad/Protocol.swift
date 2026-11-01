import Foundation

enum KeypadLink { case usb, bluetooth, offline }
func preferredLink(usb: Bool, bluetooth: Bool) -> KeypadLink {
    usb ? .usb : (bluetooth ? .bluetooth : .offline)
}

func inputCommand(_ bytes: [UInt8]) -> Int? {
    guard (bytes.count == 32 || bytes.count == 8), Array(bytes.prefix(6)) == [0xfe,84,67,79,1,2],
          (1...11).contains(bytes[6]), bytes.dropFirst(8).allSatisfy({ $0 == 0 }) else { return nil }
    return Int(bytes[6])
}
func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value),
          var line = String(data: data, encoding: .utf8) else { return }
    line += "\n"
    FileHandle.standardOutput.write(Data(line.utf8))
}
