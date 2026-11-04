// Native desktop integration invoked only by an authenticated physical keypad action.
// No transcript text is read, logged, or sent. Never submits a prompt.
import AppKit
import ApplicationServices

let codexBundle = "com.openai.codex"
let claudeBundle = "com.anthropic.claudefordesktop"
func fail(_ text: String) -> Never {
    fputs("Keypad: \(text)\n", stderr)
    exit(1)
}
func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
    return value
}
func string(_ element: AXUIElement, _ key: String) -> String {
    (attribute(element, key) as? String) ?? ""
}
func elements(_ element: AXUIElement) -> [AXUIElement] {
    (attribute(element, kAXChildrenAttribute) as? [AXUIElement]) ?? []
}
func walk(_ root: AXUIElement) -> [AXUIElement] {
    var result: [AXUIElement] = [], stack = [(root, 0)]
    while let (item, depth) = stack.popLast(), result.count < 6000 {
        result.append(item)
        if depth < 80 { stack.append(contentsOf: elements(item).reversed().map { ($0, depth + 1) }) }
    }
    return result
}
func pauseForAppUpdate() {
    // NSWorkspace activation/focus updates arrive on the main run loop.
    // Sleeping this thread leaves its foreground-app cache stale after opening a link.
    RunLoop.current.run(until: Date().addingTimeInterval(0.15))
}
func normalizedTitle(_ text: String) -> String {
    text.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
}
func headingText(_ element: AXUIElement, depth: Int = 0) -> [String] {
    let own = [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute].map { string(element, $0) }.filter { !$0.isEmpty }
    if !own.isEmpty || depth >= 2 { return own }
    return elements(element).flatMap { headingText($0, depth: depth + 1) }
}
func voiceLabelMatches(_ candidate: String, labels: [String]) -> Bool {
    let candidate = normalizedTitle(candidate).lowercased()
    return labels.contains { candidate == normalizedTitle($0).lowercased() }
}
func recordingLabels(for bundle: String) -> [String] {
    // The session composer exposes Dictate. Start Voice / Start voice chat
    // can launch the pet or a separate conversation, so never use them here.
    bundle == codexBundle ? ["Dictate"] : ["Start voice mode", "Start voice chat", "Press and hold to record"]
}
func voiceControlMatches(_ element: AXUIElement, labels: [String]) -> Bool {
    // Native controls can expose the accessible name as AXValue or as a static
    // text child, whereas Electron usually exposes it directly as AXTitle.
    let own = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute, kAXValueAttribute]
        .map { string(element, $0) }
    if own.contains(where: { voiceLabelMatches($0, labels: labels) }) { return true }
    return elements(element).contains { child in
        ["AXStaticText", "AXGroup"].contains(string(child, kAXRoleAttribute)) &&
        headingText(child, depth: 1).contains { voiceLabelMatches($0, labels: labels) }
    }
}
func supportsRecording(_ bundle: String) -> Bool {
    [codexBundle, claudeBundle].contains(bundle)
}
func focusedWindow(_ root: AXUIElement) -> AXUIElement? {
    guard let value = attribute(root, kAXFocusedWindowAttribute),
          CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
}
func canonicalContext(_ entries: [String]) -> [String] {
    Array(Set(entries.filter { !$0.isEmpty })).sorted()
}
func windowContext(_ nodes: [AXUIElement]) -> [String] {
    // Accessibility hydration can reorder nodes or add empty web areas. Neither
    // changes the selected session. Compare only nonempty, canonical identity.
    canonicalContext(nodes.filter { ["AXWindow", "AXWebArea"].contains(string($0, kAXRoleAttribute)) }.flatMap { node in
        [kAXTitleAttribute, kAXURLAttribute, kAXDocumentAttribute].compactMap { key -> String? in
            let value = attribute(node, key)
            let text = (value as? URL)?.absoluteString ?? (value as? String) ?? ""
            return text.isEmpty ? nil : key + ":" + text
        }
    })
}

/// Some hosted web controls acknowledge AXPress without delivering a click.
/// Click only the identified control, and reject overlays or stale geometry.
func clickControl(_ control: AXUIElement, root: AXUIElement, validate: () -> Bool = { true }) -> Bool {
    guard let rawPosition = attribute(control, kAXPositionAttribute),
          let rawSize = attribute(control, kAXSizeAttribute),
          CFGetTypeID(rawPosition) == AXValueGetTypeID(), CFGetTypeID(rawSize) == AXValueGetTypeID() else { return false }
    var position = CGPoint.zero, size = CGSize.zero
    guard AXValueGetValue(unsafeBitCast(rawPosition, to: AXValue.self), .cgPoint, &position),
          AXValueGetValue(unsafeBitCast(rawSize, to: AXValue.self), .cgSize, &size),
          position.x.isFinite, position.y.isFinite, size.width.isFinite, size.height.isFinite,
          size.width > 0, size.height > 0 else { return false }
    let point = CGPoint(x: position.x + size.width / 2, y: position.y + size.height / 2)
    var hit: AXUIElement?
    guard AXUIElementCopyElementAtPosition(root, Float(point.x), Float(point.y), &hit) == .success else { return false }
    var matches = false
    for _ in 0..<8 {
        guard let node = hit else { break }
        if CFEqual(node, control) { matches = true; break }
        guard let parent = attribute(node, kAXParentAttribute), CFGetTypeID(parent) == AXUIElementGetTypeID() else { break }
        hit = unsafeBitCast(parent, to: AXUIElement.self)
    }
    guard matches, validate(),
          let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left),
          let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left) else { return false }
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
    return true
}
